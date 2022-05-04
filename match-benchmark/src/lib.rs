pub mod proxy;

pub struct AcMatcher {
    ac: aho_corasick::AhoCorasick<u32>,
}

impl AcMatcher {
    pub fn new<B: AsRef<[u8]>>(proxy: &[B]) -> Self {
        let patterns = proxy.iter()
            .map(|s| s.as_ref().iter().copied().rev().collect::<Vec<u8>>())
            .collect::<Vec<_>>();

        Self {
            ac: aho_corasick::AhoCorasickBuilder::new()
                .auto_configure(&patterns)
                .anchored(true)
                .build_with_size(&patterns)
                .expect("WTF")
        }
    }

    pub fn mat(&self, domain: &[u8]) -> bool {
        let match_domain = domain.iter().copied().rev().collect::<Vec<u8>>();
        for m in self.ac.find_overlapping_iter(&match_domain) {
            if m.end() >= match_domain.len() || match_domain[m.end()] == b'.' {
                return true
            }
        }
        false
    }
}

pub struct RegexMatcher {
    reg: regex::bytes::Regex,
}
impl RegexMatcher {
    pub fn new<B: AsRef<str>>(proxy: &[B]) -> Self {
        let mut pattern = String::new();
        pattern.push_str("^(.*\\.)?(");
        let mut not_first = false;
        for domain in proxy {
            if not_first {
                pattern.push('|');
            }
            not_first = true;
            pattern.push('(');

            for byte in domain.as_ref().chars() {
                if byte == '.' {
                    pattern.push('\\');
                }
                pattern.push(byte);
            }
            pattern.push(')')
        }
        pattern.push_str(")$");

        Self {
            reg: regex::bytes::Regex::new(&pattern).unwrap()
        }
    }

    pub fn mat(&self, domain: &[u8]) -> bool {
        self.reg.is_match(domain)
    }
}

pub mod old_domains {
    const NUM_ALPHABET: usize = 26;
    const NUM_DIGIT: usize = 10;
    const NUM_SPECIAL: usize = 2; // `.`, `-`
    const NUM_CHILDREN: usize = NUM_ALPHABET + NUM_DIGIT + NUM_SPECIAL;
    use std::usize;

    const MATCHED: usize = usize::MAX;
    const NOT_MATCHED: usize = usize::MAX - 1;

    pub struct Domains {
        // usize::MAX -> matched
        // usize::MAX-1 -> Not matched
        // other -> index of first child, should be <= self.0.len() - NUM_CHILDREN
        // should not be empty
        host_trie: Vec<usize>,
    }

    impl Default for Domains {
        fn default() -> Self {
            Domains {
                host_trie: vec![NOT_MATCHED],
            }
        }
    }
    impl Domains {
        fn codec(ch: u8) -> Option<usize> {
            if b'A' <= ch && ch <= b'Z' {
                Some((ch - b'A') as usize)
            } else if b'a' <= ch && ch <= b'z' {
                Some((ch - b'a') as usize)
            } else if b'0' <= ch && ch <= b'9' {
                Some((ch - b'0') as usize + NUM_ALPHABET)
            } else if ch == b'.' {
                Some(NUM_ALPHABET + NUM_DIGIT)
            } else if ch == b'-' {
                Some(NUM_ALPHABET + NUM_DIGIT + 1)
            } else {
                None
            }
        }

        fn add_host(&mut self, suffix: &[u8]) {
            let mut current = 0;
            for &b in suffix.iter().rev() {
                let child = match self.host_trie[current] {
                    MATCHED => return,
                    NOT_MATCHED => {
                        let child = self.host_trie.len();
                        self.host_trie
                            .extend(std::iter::repeat(NOT_MATCHED).take(NUM_CHILDREN));
                        self.host_trie[current] = child;
                        child
                    }
                    child => child,
                };
                current = child + Self::codec(b).unwrap();
            }
            self.host_trie[current] = MATCHED;
        }

        fn build(&mut self) {
            self.host_trie.shrink_to_fit();
        }

        pub fn mat(&self, uri: &[u8]) -> bool {
            let mut current = 0usize;
            for &b in uri.iter().rev() {
                match self.host_trie[current] {
                    MATCHED => return true,
                    NOT_MATCHED => return false,
                    child => match Self::codec(b) {
                        Some(n) => current = child + n,
                        None => return false,
                    },
                }
            }
            self.host_trie[current] == MATCHED
        }

        pub fn new<S: AsRef<str>>(iter: impl Iterator<Item = S>) -> Domains {
            let mut domains = Domains::default();

            for s in iter {
                domains.add_host(s.as_ref().as_bytes())
            }
            domains.build();

            domains
        }
    }
}
pub type Domains = old_domains::Domains;

pub mod new_domains {
    const NUM_ALPHABET: usize = 26;
    const NUM_DIGIT: usize = 10;
    const NUM_SPECIAL: usize = 2; // `.`, `-`
    const NUM_CHILDREN: usize = NUM_ALPHABET + NUM_DIGIT + NUM_SPECIAL;
    use std::usize;

    const MATCHED: usize = usize::MAX;
    const NOT_MATCHED: usize = usize::MAX / 2 + 1;

    pub struct Domains {
        // usize::MAX -> matched
        // usize::MAX-1 -> Not matched
        // other -> index of first child, should be <= self.0.len() - NUM_CHILDREN
        // should not be empty
        host_trie: Vec<usize>,
    }

    impl Default for Domains {
        fn default() -> Self {
            Domains {
                host_trie: vec![NOT_MATCHED],
            }
        }
    }
    impl Domains {
        fn codec(ch: u8) -> Option<usize> {
            if b'A' <= ch && ch <= b'Z' {
                Some((ch - b'A') as usize)
            } else if b'a' <= ch && ch <= b'z' {
                Some((ch - b'a') as usize)
            } else if b'0' <= ch && ch <= b'9' {
                Some((ch - b'0') as usize + NUM_ALPHABET)
            } else if ch == b'.' {
                Some(NUM_ALPHABET + NUM_DIGIT)
            } else if ch == b'-' {
                Some(NUM_ALPHABET + NUM_DIGIT + 1)
            } else {
                None
            }
        }

        fn add_host(&mut self, suffix: &[u8]) {
            let mut current = 0;
            for &b in suffix.iter().rev() {
                let child = match self.host_trie[current] {
                    MATCHED => {
                        let child = self.host_trie.len();
                        self.host_trie
                            .extend(std::iter::repeat(NOT_MATCHED).take(NUM_CHILDREN));
                        if self.host_trie.len() > NOT_MATCHED {
                            panic!("too many patterns")
                        }
                        self.host_trie[current] = child + NOT_MATCHED;
                        child
                    },
                    NOT_MATCHED => {
                        let child = self.host_trie.len();
                        self.host_trie
                            .extend(std::iter::repeat(NOT_MATCHED).take(NUM_CHILDREN));
                        if self.host_trie.len() > NOT_MATCHED {
                            panic!("too many patterns")
                        }
                        self.host_trie[current] = child;
                        child
                    }
                    child => if child > NOT_MATCHED { child - NOT_MATCHED } else { child },
                };
                current = child + Self::codec(b).unwrap();
            }
            if self.host_trie[current] == NOT_MATCHED {
                self.host_trie[current] = MATCHED
            } else {
                if self.host_trie[current] < NOT_MATCHED {
                    self.host_trie[current] += NOT_MATCHED
                }
            }
        }

        fn build(&mut self) {
            self.host_trie.shrink_to_fit();
        }

        pub fn mat(&self, uri: &[u8]) -> bool {
            let mut current = 0usize;
            for &b in uri.iter().rev() {
                let child = self.host_trie[current];
                if child > NOT_MATCHED && b == b'.' {
                    return true
                }
                if child == NOT_MATCHED || child == MATCHED {
                    return false
                }
                match Self::codec(b) {
                    Some(n) => current = n + (if child > NOT_MATCHED { child - NOT_MATCHED } else { child }),
                    None => return false,
                }
            }
            self.host_trie[current] > NOT_MATCHED
        }

        pub fn new<S: AsRef<str>>(iter: impl Iterator<Item = S>) -> Domains {
            let mut domains = Domains::default();

            for s in iter {
                domains.add_host(s.as_ref().as_bytes())
            }
            domains.build();

            domains
        }
    }
}
pub type NewDomains = new_domains::Domains;


#[cfg(test)]
mod tests {
    use crate::{AcMatcher, RegexMatcher, NewDomains};

    const PROXY: &[&str] = &[
        "google.com",
        "google.com",
        "testgoogle.com",
    ];
    const TEST_CASES: &[(&[u8], bool)] = &[
        (b"google.com", true),
        (b"testgoogle.com", true),
        (b"baidu.com", false),
        (b"microsoftgoogle.com", false),
        (b"test.google.com", true),
    ];
    #[test]
    fn ac_matcher_works() {
        let matcher = AcMatcher::new(PROXY);

        for (domain, expect) in TEST_CASES.iter().copied() {
            assert_eq!(expect, matcher.mat(domain), "{}", String::from_utf8_lossy(domain))
        }
    }
    #[test]
    fn regex_matcher_works() {
        let matcher = RegexMatcher::new(PROXY);

        for (domain, expect) in TEST_CASES.iter().copied() {
            assert_eq!(expect, matcher.mat(domain), "{}", String::from_utf8_lossy(domain))
        }
    }
    #[test]
    fn new_matcher_works() {
        let matcher = NewDomains::new(PROXY.iter());

        for (domain, expect) in TEST_CASES.iter().copied() {
            assert_eq!(expect, matcher.mat(domain), "{}", String::from_utf8_lossy(domain))
        }
    }
}
