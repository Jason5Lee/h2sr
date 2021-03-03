use ipnet::{IpNet, Ipv4Net, Ipv6Net};
use iprange::IpRange;
use std::{borrow::Borrow, net::IpAddr};

const NUM_ALPHABET: usize = 26;
const NUM_DIGIT: usize = 10;
const NUM_SPECIAL: usize = 2; // `.`, `-`
const NUM_CHILDREN: usize = NUM_ALPHABET + NUM_DIGIT + NUM_SPECIAL;
use std::fmt;
use std::usize;

const MATCHED: usize = usize::MAX;
const NOT_MATCHED: usize = usize::MAX - 1;

pub struct Pattern {
  // usize::MAX -> matched
  // usize::MAX-1 -> Not matched
  // other -> index of first child, should be <= self.0.len() - NUM_CHILDREN
  // should not be empty
  host_trie: Vec<usize>,
  ipv4: IpRange<Ipv4Net>,
  ipv6: IpRange<Ipv6Net>,
}

#[derive(Debug)]
pub enum Error {
  UnexpectedCharacter(u8),
}

impl fmt::Display for Error {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    match self {
      Error::UnexpectedCharacter(ch) => write!(f, "unknown character: '{:?}'", *ch as char),
    }
  }
}

impl std::error::Error for Error {}

pub type Result<T> = std::result::Result<T, Error>;

impl Default for Pattern {
  fn default() -> Self {
    Pattern {
      host_trie: vec![NOT_MATCHED],
      ipv4: Default::default(),
      ipv6: Default::default(),
    }
  }
}
impl Pattern {
  fn codec(ch: u8) -> Result<usize> {
    if b'A' <= ch && ch <= b'Z' {
      Ok((ch - b'A') as usize)
    } else if b'a' <= ch && ch <= b'z' {
      Ok((ch - b'a') as usize)
    } else if b'0' <= ch && ch <= b'9' {
      Ok((ch - b'0') as usize + NUM_ALPHABET)
    } else if ch == b'.' {
      Ok(NUM_ALPHABET + NUM_DIGIT)
    } else if ch == b'-' {
      Ok(NUM_ALPHABET + NUM_DIGIT + 1)
    } else {
      Err(Error::UnexpectedCharacter(ch))
    }
  }

  fn add_host(&mut self, suffix: &[u8]) -> Result<()> {
    let mut current = 0;
    for &b in suffix.iter().rev() {
      let child = match self.host_trie[current] {
        MATCHED => return Ok(()),
        NOT_MATCHED => {
          let child = self.host_trie.len();
          self
            .host_trie
            .extend(std::iter::repeat(NOT_MATCHED).take(NUM_CHILDREN));
          self.host_trie[current] = child;
          child
        }
        child => child,
      };
      current = child + Self::codec(b)?;
    }
    self.host_trie[current] = MATCHED;
    Ok(())
  }

  pub fn add(&mut self, host_or_ip: &str) -> Result<()> {
    if let Ok(ipnet) = host_or_ip.parse::<IpNet>() {
      match ipnet {
        IpNet::V4(ipnet) => {
          self.ipv4.add(ipnet);
        }
        IpNet::V6(ipnet) => {
          self.ipv6.add(ipnet);
        }
      }
      Ok(())
    } else {
      self.add_host(host_or_ip.as_bytes())
    }
  }

  pub fn build(&mut self) {
    self.host_trie.shrink_to_fit();
    self.ipv4.simplify();
    self.ipv6.simplify();
  }

  pub fn contain_host(&self, uri: &[u8]) -> bool {
    let mut current = 0usize;
    for &b in uri.iter().rev() {
      match self.host_trie[current] {
        MATCHED => return true,
        NOT_MATCHED => return false,
        child => match Self::codec(b) {
          Ok(n) => current = child + n,
          Err(_) => return false,
        },
      }
    }
    self.host_trie[current] == MATCHED
  }

  pub fn contain_ip(&self, ip: &IpAddr) -> bool {
    match ip {
      IpAddr::V4(ip) => self.ipv4.contains(ip),
      IpAddr::V6(ip) => self.ipv6.contains(ip),
    }
  }

  pub fn from_strs<'a>(iter: impl Iterator<Item = &'a str>) -> Result<Pattern> {
    let mut pattern = Pattern::default();

    for s in iter {
      pattern.add(s.borrow())?
    }
    pattern.build();

    Ok(pattern)
  }
}
