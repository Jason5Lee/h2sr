# h2sr

Http-to-socks5 proxy router.

**WARNING**: this program is mainly for my personal need. Issues are welcomed but I may not consider every feature reuquests.

This app serves a HTTP proxy, forward the connection to a socks5 proxy, direct connection or block, depends on your config.

You can download the prebuilt binaries for macOS, Linux and Windows in [github release](https://github.com/Jason5Lee/h2sr/releases).

## Usage

```
h2sr 0.1.1
Http-to-socks5 proxy router

USAGE:
    h2sr [OPTIONS]

FLAGS:
    -h, --help       Prints help information
    -V, --version    Prints version information

OPTIONS:
    -c, --config <PATH>    The config file path, if it isn't present, the program will find ".h2sr.toml" in the user
                           directory
```

## Config file format

```toml
listen = "127.0.0.1:8080" # Address the http proxy listen to.
socks5addr = "127.0.0.1:1086" # Address of the socks5 proxy.
proxy = [
  "the.domain.suffix.you.want.to.be.proxied.com",
  "3.3.3.3/24" # CIDR ipv4 range
]
block = [
  "the.domain.suffix.you.want.to.be.blocked.com",
  "3.4.3.3/24" # CIDR ip range
]

```
