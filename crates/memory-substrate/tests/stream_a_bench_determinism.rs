#![cfg(feature = "bench-harness")]

#[allow(dead_code)]
#[path = "../src/bin/stream_a_bench.rs"]
mod stream_a_bench;

#[test]
fn generated_corpus_and_workload_are_deterministic_for_seed() {
    let first = stream_a_bench::generate_bench_input(0x5eed, 7, 3);
    let second = stream_a_bench::generate_bench_input(0x5eed, 7, 3);

    assert_eq!(
        serde_json::to_vec(&first).expect("serialize first input"),
        serde_json::to_vec(&second).expect("serialize second input")
    );
}
