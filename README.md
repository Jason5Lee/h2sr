# h2sr

Http-to-socks5 proxy router.

**WARNING**: this program is mainly for my personal need. Issues are welcomed but I may not consider every feature reuquests.

This app serves a HTTP proxy, forward the connection to a socks5 proxy, direct connection or block, depends on your config.

You can download the prebuilt binaries for macOS, Linux and Windows in [github release](https://github.com/Jason5Lee/h2sr/releases).

## Usage

Put the configuration file at `$HOME/.h2sr/config.toml` . 

Optionally put the [`geoip.dat`](https://github.com/v2fly/geoip/releases) file at `$HOME/.h2sr/geoip.dat` .

## Config file format

```toml
listen = "127.0.0.1:8080" # Address the http proxy listen to.
socks5addr = "127.0.0.1:1086" # Address of the socks5 proxy.
proxydomains = [
  "the.domain.suffix.you.want.to.connect.through.proxy.com",
]
proxyips = [
  "3.3.3.3/24", # CIDR ipv4 range
  "geo:us", # IP location, requires `geoip.dat`.
]

# Only one of proxydomains and directdomains can be set.
directdomains = [
  "the.domain.suffix.you.want.to.connect.directly.com",
]
# Only one of proxyips and directips can be set.
directips = [
  "3.3.3.3/24", # CIDR ipv4 range
  "geo:private", # IP location, requires `geoip.dat`.
]
blockdomains = [
  "the.domain.suffix.you.want.to.be.blocked.com",
]
blockips = [
  "3.4.3.3/24", # CIDR ip range
]

```

## Update in 0.2.0

- More flexible rule.
- geoip.dat supports.
