use futures_util::future::try_join;
use h2sr::ipgeo::GeoIPList;
use ipnet::{IpNet, Ipv4Net, Ipv6Net};
use protobuf::Message;
use std::io::Write;
use termcolor::{Color, ColorChoice, ColorSpec, StandardStream, WriteColor};
use std::{
    convert::{Infallible, TryFrom},
    fmt::Display,
    io::BufReader,
    net::SocketAddr,
    path::PathBuf,
};
use std::{fs, net::IpAddr};
use tokio_socks::tcp::Socks5Stream;

use hyper::{
    service::{make_service_fn, service_fn},
    upgrade::Upgraded,
};
use hyper::{Body, Client, Method, Request, Response, Server};

use anyhow::anyhow;
use h2sr::{Domains, Ips};
use once_cell::unsync;
use serde::Deserialize;
use std::path::Path;
use tokio::net::{lookup_host, TcpStream};

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
                println!("DIRECT: {}", auth);
                Ok(Self::tunnel_stream(auth, upgraded, &mut TcpStream::connect(auth).await?).await)
            }
            Connection::Socks5 => {
                println!("PROXY: {}", auth);
                Ok(Self::tunnel_stream(
                    auth,
                    upgraded,
                    &mut *Socks5Stream::connect(socks5addr, auth).await?,
                )
                .await)
            }
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
                log_error(auth, e).expect(ERROR_WHILE_LOGGING);
            }
        };
    }
}

const ERROR_WHILE_LOGGING: &'static str = "error while logging";
fn log_error(auth: &str, log: impl Display) -> std::io::Result<()> {
    let stderr = StandardStream::stderr(ColorChoice::Auto);
    let mut lock = stderr.lock();
    if !auth.is_empty() {
        write!(lock, "[{}] ", auth)?;
    }
    lock.set_color(ColorSpec::new().set_fg(Some(Color::Red)).set_bold(true))?;
    write!(lock, "error")?;
    lock.reset()?;
    writeln!(lock, ": {}", log)
}

enum DomainRule {
    Direct(Domains),
    Proxy(Domains),
}

impl DomainRule {
    fn get_connection(&self, uri: &[u8]) -> Option<Connection> {
        match self {
            DomainRule::Direct(domains) => {
                if domains.contain_host(uri) {
                    Some(Connection::Direct)
                } else {
                    None
                }
            }
            DomainRule::Proxy(domains) => {
                if domains.contain_host(uri) {
                    Some(Connection::Socks5)
                } else {
                    None
                }
            }
        }
    }
}

enum IpRule {
    Direct(Ips),
    Proxy(Ips),
}

impl IpRule {
    fn get_connection(&self, ip: IpAddr) -> Connection {
        match self {
            IpRule::Direct(ips) => {
                if ips.contain_ip(ip) {
                    Connection::Direct
                } else {
                    Connection::Socks5
                }
            }
            IpRule::Proxy(ips) => {
                if ips.contain_ip(ip) {
                    Connection::Socks5
                } else {
                    Connection::Direct
                }
            }
        }
    }
}
struct Env {
    listen: SocketAddr,
    socks5addr: SocketAddr,
    blockdomains: Domains,
    blockips: Ips,
    domain_rule: DomainRule,
    ip_rule: IpRule,
}

#[derive(Deserialize)]
struct Config {
    listen: SocketAddr,
    socks5addr: SocketAddr,
    proxydomains: Option<Vec<String>>,
    proxyips: Option<Vec<String>>,
    directdomains: Option<Vec<String>>,
    directips: Option<Vec<String>>,
    #[serde(default = "Vec::new")]
    blockips: Vec<String>,
    #[serde(default = "Vec::new")]
    blockdomains: Vec<String>,
}

const ILLEGAL_CIDR: &'static str = "illegal CIDR";
fn cidr_to_ipnet(cider: &h2sr::ipgeo::CIDR) -> IpNet {
    let ip = cider.get_ip();
    let prefix = cider.get_prefix() as u8;
    if let Ok(ipv6_bytes) = <[u8; 16]>::try_from(ip) {
        IpNet::V6(Ipv6Net::new(ipv6_bytes.into(), prefix).expect(ILLEGAL_CIDR))
    } else if let Ok(ipv4_bytes) = <[u8; 4]>::try_from(ip) {
        IpNet::V4(Ipv4Net::new(ipv4_bytes.into(), prefix).expect(ILLEGAL_CIDR))
    } else {
        panic!("{}", ILLEGAL_CIDR)
    }
}
const GEO_PREFIX: &'static str = "geo:";
fn to_ipnets_vec<'a, F: Fn() -> GeoIPList>(
    ip_strings: impl Iterator<Item = &'a String>,
    geoip_list: &unsync::Lazy<GeoIPList, F>,
) -> Vec<IpNet> {
    let mut result = Vec::new();
    for ip_string in ip_strings {
        let ip_str = ip_string.as_str();
        if ip_str.starts_with(GEO_PREFIX) {
            let geo = &ip_str[GEO_PREFIX.len()..];
            let mut matched_ipnet = geoip_list
                .get_entry()
                .iter()
                .filter(|geoip| geoip.get_country_code().eq_ignore_ascii_case(geo))
                .flat_map(|geoip| geoip.get_cidr().iter().map(cidr_to_ipnet))
                .collect::<Vec<_>>();
            if matched_ipnet.len() == 0 {
                let mut stderr = StandardStream::stderr(ColorChoice::Auto);
                (|| -> std::io::Result<()> {
                    stderr.set_color(ColorSpec::new().set_fg(Some(Color::Yellow)).set_bold(true))?;
                    write!(stderr, "warning")?;
                    stderr.reset()?;
                    writeln!(stderr, ": geo `{}` not found", geo)
                })().expect(ERROR_WHILE_LOGGING);
            }
            result.append(&mut matched_ipnet);
        } else if let Ok(ipnet) = ip_str.parse::<IpNet>() {
            result.push(ipnet)
        } else {
            panic!("illegal ip: `{}`", ip_str)
        }
    }
    ;;
    result
}
fn load_geoip(geoip_path: &Path) -> GeoIPList {
    let mut buf_reader =
        BufReader::new(fs::File::open(geoip_path).expect("unable to open geoip.dat file"));
    let mut proto_in = protobuf::CodedInputStream::from_buffered_reader(&mut buf_reader);
    GeoIPList::parse_from(&mut proto_in).expect("error while parsing geoip.dat")
}
fn load_env() -> Env {
    let mut h2sr_dir: PathBuf = directories::UserDirs::new()
        .expect("unable to get user directory")
        .home_dir()
        .to_path_buf();
    h2sr_dir.push(".h2sr");

    let config: Config = {
        let mut path = h2sr_dir.clone();
        path.push("config.toml");
        path.shrink_to_fit();
        let bytes = fs::read(path).expect("unable to read config.toml file");
        toml::from_slice(&bytes).expect("config file error")
    };
    let geoip = {
        let mut path = h2sr_dir;
        path.push("geoip.dat");
        path.shrink_to_fit();
        unsync::Lazy::<GeoIPList, _>::new(move || load_geoip(&path))
    };

    let blockdomains = Domains::from_strs(config.blockdomains.iter().map(|v| v.as_str()))
        .expect("error while parsing `blockdomains`");

    let blockips = Ips::from_ipnets(to_ipnets_vec(config.blockips.iter(), &geoip).into_iter())
        .expect("error while parsing `blockips`");

    let domain_rule = match (config.directdomains, config.proxydomains) {
        (Some(directdomains), None) => DomainRule::Direct(
            Domains::from_strs(directdomains.iter().map(|s| s.as_str()))
                .expect("error while parsing `directdomains`"),
        ),
        (None, Some(proxydomains)) => DomainRule::Proxy(
            Domains::from_strs(proxydomains.iter().map(|s| s.as_str()))
                .expect("error while parsing `proxydomains`"),
        ),

        _ => panic!("only exact one of `directdomains` and `proxydoamins` should be set"),
    };

    let ip_rule = match (config.directips, config.proxyips) {
        (Some(directips), None) => IpRule::Direct(
            Ips::from_ipnets(to_ipnets_vec(directips.iter(), &geoip).into_iter())
                .expect("error while parsing `directips`"),
        ),
        (None, Some(proxyips)) => IpRule::Proxy(
            Ips::from_ipnets(to_ipnets_vec(proxyips.iter(), &geoip).into_iter())
                .expect("error while parsing `proxyips`"),
        ),

        _ => panic!("only exact one of `directips` and `proxyips` should be set"),
    };
    Env {
        listen: config.listen,
        socks5addr: config.socks5addr,
        blockdomains,
        blockips,
        domain_rule,
        ip_rule,
    }
}

#[tokio::main]
async fn main() {
    let env: &'static _ = Box::leak(Box::new(load_env()));
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
            .expect(ERROR_WHILE_LOGGING);
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
                log_error(&auth, &err).expect(ERROR_WHILE_LOGGING);
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
    let connection = if let Some(ipaddr) = std::str::from_utf8(host)
        .ok()
        .and_then(|s| s.parse::<IpAddr>().ok())
    {
        if env.blockips.contain_ip(ipaddr) {
            println!("BLOCK {}", auth);
            return Ok(());
        } else {
            env.ip_rule.get_connection(ipaddr)
        }
    } else if env.blockdomains.contain_host(host) {
        println!("BLOCK {}", auth);
        return Ok(());
    } else if let Some(connection) = env.domain_rule.get_connection(host) {
        connection
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
                    if env.blockips.contain_ip(ip.ip()) {
                        println!("BLOCK {}", auth);
                        return Ok(());
                    } else {
                        env.ip_rule.get_connection(ip.ip())
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
                    log_error(&auth, e).expect(ERROR_WHILE_LOGGING);
                };
            }
            Err(e) => log_error(&auth, e).expect(ERROR_WHILE_LOGGING),
        }
    });
    Ok(())
}
