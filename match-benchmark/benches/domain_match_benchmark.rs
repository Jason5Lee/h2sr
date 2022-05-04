use criterion::{criterion_group, criterion_main, Criterion};
use domain_match_benchmark::{AcMatcher, RegexMatcher, Domains, NewDomains};
use domain_match_benchmark::proxy::PROXY;
use rand::seq::SliceRandom;

pub fn criterion_benchmark(c: &mut Criterion) {
    let mut proxy_for_shuffle = PROXY.to_vec();
    let (test, _) = proxy_for_shuffle.partial_shuffle(&mut rand::thread_rng(), 1000);
    let test = &test as &[&str];
    
    let ac_matcher = AcMatcher::new(PROXY);
    // let regex_matcher = RegexMatcher::new(PROXY);
    let old_matcher = Domains::new(PROXY.iter());
    let new_matcher = NewDomains::new(PROXY.iter());

    c.bench_function("Ac Matcher", |b| b.iter(|| {
        for test_domain in test.iter().copied() {
            assert!(ac_matcher.mat(test_domain.as_bytes()))
        }
    }));
    // c.bench_function("Regex Matcher", |b| {
    //     b.iter(|| {
    //         for test_domain in test.iter().copied() {
    //             assert!(regex_matcher.mat(test_domain.as_bytes()))
    //         }
    //     })
    // });
    c.bench_function("New Matcher", |b| b.iter(|| {
        for test_domain in test.iter().copied() {
            assert!(new_matcher.mat(test_domain.as_bytes()), "{}", test_domain)
        }
    }));
    c.bench_function("Old Matcher", |b| b.iter(|| {
        for test_domain in test.iter().copied() {
            assert!(old_matcher.mat(test_domain.as_bytes()))
        }
    }));
}

criterion_group!(benches, criterion_benchmark);
criterion_main!(benches);