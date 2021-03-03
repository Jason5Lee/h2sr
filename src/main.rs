use futures_util::future::try_join;
use std::fs;
use std::{convert::Infallible, net::SocketAddr};

use hyper::service::{make_service_fn, service_fn};
use hyper::{Body, Client, Method, Request, Response, Server};

use anyhow::anyhow;
use h2sr::Pattern;
use serde::Deserialize;
use std::borrow::Cow;
use std::path::Path;
use tokio::net::tcp::{ReadHalf, WriteHalf};
use tokio::net::{lookup_host, TcpStream};

type HttpClient = Client<hyper::client::HttpConnector>;

#[derive(Clone, Copy)]
pub struct Socks5Proxy(SocketAddr);

impl Socks5Proxy {
  pub async fn connect(&self, auth: &str, req: Request<Body>) {
    match tokio_socks::tcp::Socks5Stream::connect(self.0, &*auth).await {
      Ok(mut stream) => tunnel(auth, req, stream.split()).await,
      Err(e) => eprintln!("{}: socks5 connect error: {:?}", auth, e),
    }
  }
}
pub struct Env {
  pub listen: SocketAddr,
  pub to: Socks5Proxy,
  pub proxy: Pattern,
  pub block: Pattern,
}

#[derive(Deserialize)]
pub struct Config {
  pub listen: SocketAddr,
  pub to: SocketAddr,
  pub proxy: Vec<String>,
  pub block: Vec<String>,
}

async fn load_env() -> Env {
  let matches = clap::App::new("Http socks router")
    .author("Jason D.H. Lee <jason5lee@hotmail.com>")
    .arg(
      clap::Arg::with_name("config")
        .short("c")
        .long("config")
        .value_name("FILE")
        .help("The config file, default \".h2sr.toml\" in user directory.")
        .takes_value(true),
    )
    .get_matches();

  let config: Cow<'_, Path> = matches
    .value_of("config")
    .map(|path| Path::new(path).into())
    .unwrap_or_else(|| {
      let user_dir = directories::UserDirs::new().expect("unable to get user directory");
      let mut path = user_dir.home_dir().to_owned();
      path.push(".h2sr.toml");
      path.into()
    });
  let config = fs::read(config).expect("unable to read config file");

  let config: Config = toml::from_slice(&config).expect("config file error");

  let proxy =
    Pattern::from_strs(config.proxy.iter().map(|s| &*s as &str)).expect("proxy config error");
  let block =
    Pattern::from_strs(config.block.iter().map(|s| &*s as &str)).expect("block config error");

  Env {
    listen: config.listen,
    to: Socks5Proxy(config.to),
    proxy,
    block,
  }
}
// To try this example:
// 1. cargo run --example http_proxy
// 2. config http_proxy in command line
//    $ export http_proxy=http://127.0.0.1:8100
//    $ export https_proxy=http://127.0.0.1:8100
// 3. send requests
//    $ curl -i https://www.some_domain.com/
#[tokio::main]
async fn main() {
  let env: &'static _ = Box::leak(Box::new(load_env().await));

  let client = HttpClient::new();

  let make_service = make_service_fn(move |_| {
    let client = client.clone();
    async move { Ok::<_, Infallible>(service_fn(move |req| proxy(env, client.clone(), req))) }
  });

  let server = Server::bind(&env.listen).serve(make_service);

  println!("Listening on http://{}", env.listen);

  if let Err(e) = server.await {
    eprintln!("server error: {}", e);
  }
}

async fn proxy(
  env: &'static Env,
  client: HttpClient,
  req: Request<Body>,
) -> anyhow::Result<Response<Body>> {
  // println!("req: {:?}", req);

  if Method::CONNECT == req.method() {
    // Received an HTTP request like:
    // ```
    // CONNECT www.domain.com:443 HTTP/1.1
    // Host: www.domain.com:443
    // Proxy-Connection: Keep-Alive
    // ```
    //
    // When HTTP method is CONNECT we should return an empty body
    // then we can eventually upgrade the connection and talk a new protocol.
    //
    // Note: only after client received an empty body with STATUS_OK can the
    // connection be upgraded, so we can't return a response inside
    // `on_upgrade` future.
    let uri = req.uri();
    let auth = uri
      .authority()
      .ok_or_else(|| anyhow!("CONNECT host is not socket addr: {:?}", uri))?;
    let host = auth.host().as_bytes();
    let auth = auth.to_string();
    let to = &env.to;
    if env.block.contain_host(host) {
      println!("BLOCK {}", auth);
    } else if env.proxy.contain_host(host) {
      println!("PROXY {}", auth);
      tokio::task::spawn(async move { to.connect(&auth, req).await });
    } else {
      tokio::task::spawn(async move {
        match lookup_host(&auth).await {
          Err(e) => eprintln!("lookup host {} error: {:?}.", auth, e),
          Ok(mut host) => match host.next() {
            None => eprintln!("lookup host {} error: no ip found.", auth),
            Some(ip) => {
              if env.proxy.contain_ip(&ip.ip()) {
                println!("PROXY {}", auth);
                to.connect(&auth, req).await;
              } else {
                println!("DIRECT {}", auth);
                match TcpStream::connect(&*auth).await {
                  Ok(mut stream) => tunnel(&auth, req, stream.split()).await,
                  Err(e) => eprintln!("{}: direct connect error: {:?}", auth, e),
                }
              }
            }
          },
        }
      });
    };

    Ok(Response::new(Body::empty()))
  } else {
    Ok(client.request(req).await?)
  }
}

// Create a TCP connection to host:port, build a tunnel between the connection and
// the upgraded connection
async fn tunnel(
  auth: &str,
  req: Request<Body>,
  (mut server_rd, mut server_wr): (ReadHalf<'_>, WriteHalf<'_>),
) {
  match hyper::upgrade::on(req).await {
    Ok(upgraded) => {
      // Proxying data
      let amounts = {
        let (mut client_rd, mut client_wr) = tokio::io::split(upgraded);

        let client_to_server = tokio::io::copy(&mut client_rd, &mut server_wr);
        let server_to_client = tokio::io::copy(&mut server_rd, &mut client_wr);

        try_join(client_to_server, server_to_client).await
      };

      // Print message when done
      match amounts {
        Ok((from_client, from_server)) => {
          println!(
            "{}: client wrote {} bytes and received {} bytes",
            auth, from_client, from_server
          );
        }
        Err(e) => {
          eprintln!("{}: tunnel error: {}", auth, e);
        }
      };
    }
    Err(e) => eprintln!("{}: upgrade error: {}", auth, e),
  }
}
