use criterion::{criterion_group, criterion_main, Criterion};
use domain_match_benchmark::proxy::PROXY;
use domain_match_benchmark::{AcMatcher, Domains, NewDomains};
use rand::seq::SliceRandom;
use rand::thread_rng;

fn random_prefix() -> String {
    const CHARS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";
    let mut s = String::with_capacity(16);
    let mut rng = thread_rng();
    for _ in 0..16 {
        s.push(*CHARS.choose(&mut rng).unwrap() as char)
    }
    s
}
pub fn criterion_benchmark(c: &mut Criterion) {
    let ac_matcher = AcMatcher::new(PROXY);
    // let regex_matcher = RegexMatcher::new(PROXY);
    let old_matcher = Domains::new(PROXY.iter());
    let new_matcher = NewDomains::new(PROXY.iter());

    let mut proxy_for_shuffle = PROXY.to_vec();
    let test_same: Vec<String> = proxy_for_shuffle
        .partial_shuffle(&mut thread_rng(), 1000)
        .0
        .iter()
        .map(|s| s.to_string())
        .collect();

    let test_prefix: Vec<String> = proxy_for_shuffle
        .partial_shuffle(&mut thread_rng(), 1000)
        .0
        .iter()
        .map(|s| format!("{}.{}", random_prefix(), s))
        .collect();

    let test_prefix_diff: Vec<String> = proxy_for_shuffle
        .partial_shuffle(&mut thread_rng(), 1000)
        .0
        .iter()
        .filter_map(|s| {
            for _ in 0..1000 {
                let attempt = format!("{}{}", random_prefix(), s);
                let matched = ac_matcher.mat(attempt.as_bytes());
                assert_eq!(
                    matched,
                    new_matcher.mat(attempt.as_bytes()),
                    "diff: {attempt}"
                );
                if !matched {
                    return Some(attempt);
                }
            }
            None
        })
        .collect();

    c.bench_function("Ac Matcher - Same as pattern", |b| {
        b.iter(|| {
            for test_domain in test_same.iter() {
                assert!(ac_matcher.mat(test_domain.as_bytes()))
            }
        })
    });
    // c.bench_function("Regex Matcher", |b| {
    //     b.iter(|| {
    //         for test_domain in test.iter().copied() {
    //             assert!(regex_matcher.mat(test_domain.as_bytes()))
    //         }
    //     })
    // });
    c.bench_function("New Matcher - Same as pattern", |b| {
        b.iter(|| {
            for test_domain in test_same.iter() {
                assert!(new_matcher.mat(test_domain.as_bytes()), "{}", test_domain)
            }
        })
    });
    c.bench_function("Old Matcher - Same as pattern", |b| {
        b.iter(|| {
            for test_domain in test_same.iter() {
                assert!(old_matcher.mat(test_domain.as_bytes()))
            }
        })
    });

    c.bench_function("Ac Matcher - With prefix", |b| {
        b.iter(|| {
            for test_domain in test_prefix.iter() {
                assert!(ac_matcher.mat(test_domain.as_bytes()))
            }
        })
    });
    c.bench_function("New Matcher - With prefix", |b| {
        b.iter(|| {
            for test_domain in test_prefix.iter() {
                assert!(new_matcher.mat(test_domain.as_bytes()), "{}", test_domain)
            }
        })
    });

    c.bench_function("Ac Matcher - With diff prefix", |b| {
        b.iter(|| {
            for test_domain in test_prefix_diff.iter() {
                assert!(!ac_matcher.mat(test_domain.as_bytes()))
            }
        })
    });
    c.bench_function("New Matcher - With diff prefix", |b| {
        b.iter(|| {
            for test_domain in test_prefix_diff.iter() {
                assert!(!new_matcher.mat(test_domain.as_bytes()), "{}", test_domain)
            }
        })
    });
}

criterion_group!(benches, criterion_benchmark);
criterion_main!(benches);
