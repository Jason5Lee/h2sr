use futures_util::future::try_join;
use std::fs;
use std::{convert::Infallible, fmt::Display, net::SocketAddr};
use tokio_socks::tcp::Socks5Stream;

use hyper::{
    service::{make_service_fn, service_fn},
    upgrade::Upgraded,
};
use hyper::{Body, Client, Method, Request, Response, Server};

use anyhow::anyhow;
use h2sr::Pattern;
use serde::Deserialize;
use std::borrow::Cow;
use std::path::Path;
use tokio::net::{lookup_host, TcpStream};

use ansi_term::Colour::Red;

type HttpClient = Client<hyper::client::HttpConnector>;

enum Connection {
    Direct,
    Socks5,
}

impl Connection {
    // Create a TCP connection to host:port, build a tunnel between the connection and
    // the upgraded connection
    async fn tunnel(
        self,
        auth: &str,
        upgraded: Upgraded,
        socks5addr: SocketAddr,
    ) -> anyhow::Result<()> {
        match self {
            Connection::Direct => {
                Ok(Self::tunnel_stream(auth, upgraded, &mut TcpStream::connect(auth).await?).await)
            }
            Connection::Socks5 => Ok(Self::tunnel_stream(
                auth,
                upgraded,
                &mut *Socks5Stream::connect(socks5addr, auth).await?,
            )
            .await),
        }
    }

    async fn tunnel_stream(auth: &str, upgraded: Upgraded, server: &mut TcpStream) {
        // Proxying data
        let amounts = {
            let (mut server_rd, mut server_wr) = server.split();
            let (mut client_rd, mut client_wr) = tokio::io::split(upgraded);

            let client_to_server = tokio::io::copy(&mut client_rd, &mut server_wr);
            let server_to_client = tokio::io::copy(&mut server_rd, &mut client_wr);

            try_join(client_to_server, server_to_client).await
        };

        // Print message when done
        match amounts {
            Ok((from_client, from_server)) => {
                println!(
                    "[{}] client wrote {} bytes and received {} bytes",
                    auth, from_client, from_server
                );
            }
            Err(e) => {
                log_error(auth, e);
            }
        };
    }
}

fn log_error(auth: &str, log: impl Display) {
    if !auth.is_empty() {
        eprint!("[{}] ", auth);
    }
    eprintln!("{}: {}", Red.paint("error"), log);
}

pub struct Env {
    pub listen: SocketAddr,
    pub socks5addr: SocketAddr,
    pub proxy: Pattern,
    pub block: Pattern,
}

#[derive(Deserialize)]
pub struct Config {
    pub listen: SocketAddr,
    pub socks5addr: SocketAddr,
    pub proxy: Vec<String>,
    pub block: Vec<String>,
}

async fn load_env() -> Env {
    let matches = clap::App::new("h2sr")
        // .author("Jason5Lee <jason5lee@hotmail.com>")
        .about("Http-to-socks5 proxy router")
        .version("0.1.1")
        .arg(
            clap::Arg::with_name("config")
                .short("c")
                .long("config")
                .value_name("PATH")
                .help("The config file path, if it isn't present, the program will find \".h2sr.toml\" in the user directory")
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
        socks5addr: config.socks5addr,
        proxy,
        block,
    }
}

#[tokio::main]
async fn main() {
    let env: &'static _ = Box::leak(Box::new(load_env().await));
    fdlimit::raise_fd_limit();
    let client = HttpClient::new();

    let make_service = make_service_fn(move |_| {
        let client = client.clone();
        async move { Ok::<_, Infallible>(service_fn(move |req| proxy(env, client.clone(), req))) }
    });

    let server = Server::bind(&env.listen).serve(make_service);

    println!("Listening on http://{}", env.listen);

    if let Err(e) = server.await {
        log_error("", e)
    }
}

async fn proxy(
    env: &'static Env,
    client: HttpClient,
    req: Request<Body>,
) -> Result<Response<Body>, hyper::Error> {
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
        match connect(env, req).await {
            Ok(()) => Ok(Response::new(Body::empty())),
            Err((code, auth, err)) => {
                let err = err.to_string();
                log_error(&auth, &err);
                Ok(Response::builder()
                    .status(code)
                    .body(err.into())
                    .expect("failed to create http response"))
            }
        }
    } else {
        client.request(req).await
    }
}

async fn connect(
    env: &'static Env,
    req: Request<Body>,
) -> Result<(), (http::StatusCode, String, anyhow::Error)> {
    let uri = req.uri();
    let auth = uri.authority().ok_or_else(|| {
        (
            http::StatusCode::BAD_REQUEST,
            String::new(),
            anyhow!("CONNECT host is illegal: '{}'", uri),
        )
    })?;

    let host = auth.host().as_bytes();
    let connection = if env.block.contain_host(host) {
        println!("BLOCK {}", auth);
        return Ok(());
    } else if env.proxy.contain_host(host) {
        println!("PROXY {}", auth);
        Connection::Socks5
    } else {
        let auth = auth.as_str();
        match lookup_host(auth).await {
            Err(e) => return Err((http::StatusCode::BAD_GATEWAY, auth.to_string(), e.into())),
            Ok(mut host) => match host.next() {
                None => {
                    return Err((
                        http::StatusCode::BAD_GATEWAY,
                        auth.to_string(),
                        anyhow!("no ip found"),
                    ))
                }
                Some(ip) => {
                    if env.block.contain_ip(&ip.ip()) {
                        println!("BLOCK {}", auth);
                        return Ok(());
                    } else if env.proxy.contain_ip(&ip.ip()) {
                        println!("PROXY {}", auth);
                        Connection::Socks5
                    } else {
                        println!("DIRECT {}", auth);
                        Connection::Direct
                    }
                }
            },
        }
    };

    let auth = auth.to_string();
    let socks5 = env.socks5addr;
    tokio::task::spawn(async move {
        match hyper::upgrade::on(req).await {
            Ok(upgraded) => {
                if let Err(e) = connection.tunnel(&auth, upgraded, socks5).await {
                    log_error(&auth, e);
                };
            }
            Err(e) => log_error(&auth, e),
        }
    });
    Ok(())
}
