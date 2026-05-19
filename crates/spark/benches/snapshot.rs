//! Microbenchmarks for the Spark snapshot pipeline.
//!
//! Measures the full encode + verify + decode round-trip for both signing
//! modes (HMAC-only and AES-GCM-encrypted). These numbers are what gate the
//! end-to-end latency of a Spark `click → server → re-render → client` cycle —
//! every interaction encodes one outbound snapshot and decodes one inbound.

use std::time::Duration;

use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use serde_json::json;

use spark::snapshot::{self, Envelope, Memo};

const KEY: &str = "spark-bench-app-key-thirty-two-bb";

fn sample_memo() -> Memo {
    Memo {
        id: "01HXY-bench".into(),
        class: "bench::Counter".into(),
        view: "spark/counter".into(),
        listeners: vec!["posts.created".into()],
        errors: None,
    }
}

fn small_state() -> serde_json::Value {
    json!({
        "count": 42,
        "label": "Visits",
        "draft": "hello"
    })
}

fn medium_state() -> serde_json::Value {
    json!({
        "count": 42,
        "label": "Visits",
        "draft": "hello",
        "items": (0..50).map(|i| json!({
            "id": i,
            "title": format!("Item {i}"),
            "active": i % 2 == 0,
        })).collect::<Vec<_>>(),
    })
}

fn bench_encode(c: &mut Criterion) {
    let mut group = c.benchmark_group("snapshot_encode");
    group.measurement_time(Duration::from_secs(5));

    for (name, state) in [("small", small_state()), ("medium", medium_state())] {
        let envelope = Envelope::build(KEY, state.clone(), sample_memo());
        let wire_len = snapshot::encode(&envelope, KEY, false).unwrap().len() as u64;
        group.throughput(Throughput::Bytes(wire_len));
        group.bench_with_input(BenchmarkId::new("hmac", name), &envelope, |b, env| {
            b.iter(|| snapshot::encode(env, KEY, false).unwrap());
        });
        group.bench_with_input(BenchmarkId::new("aes_gcm", name), &envelope, |b, env| {
            b.iter(|| snapshot::encode(env, KEY, true).unwrap());
        });
    }
    group.finish();
}

fn bench_decode(c: &mut Criterion) {
    let mut group = c.benchmark_group("snapshot_decode");
    group.measurement_time(Duration::from_secs(5));

    for (name, state) in [("small", small_state()), ("medium", medium_state())] {
        let envelope = Envelope::build(KEY, state, sample_memo());
        let hmac_wire = snapshot::encode(&envelope, KEY, false).unwrap();
        let enc_wire = snapshot::encode(&envelope, KEY, true).unwrap();
        group.throughput(Throughput::Bytes(hmac_wire.len() as u64));

        group.bench_with_input(BenchmarkId::new("hmac", name), &hmac_wire, |b, wire| {
            b.iter(|| snapshot::decode(wire, KEY).unwrap());
        });
        group.bench_with_input(BenchmarkId::new("aes_gcm", name), &enc_wire, |b, wire| {
            b.iter(|| snapshot::decode(wire, KEY).unwrap());
        });
    }
    group.finish();
}

fn bench_round_trip(c: &mut Criterion) {
    let mut group = c.benchmark_group("snapshot_round_trip");
    group.measurement_time(Duration::from_secs(5));
    let state = small_state();
    let memo = sample_memo();

    group.bench_function("hmac_small", |b| {
        b.iter(|| {
            let env = Envelope::build(KEY, state.clone(), memo.clone());
            let wire = snapshot::encode(&env, KEY, false).unwrap();
            snapshot::decode(&wire, KEY).unwrap();
        });
    });
    group.bench_function("aes_gcm_small", |b| {
        b.iter(|| {
            let env = Envelope::build(KEY, state.clone(), memo.clone());
            let wire = snapshot::encode(&env, KEY, true).unwrap();
            snapshot::decode(&wire, KEY).unwrap();
        });
    });
    group.finish();
}

criterion_group!(benches, bench_encode, bench_decode, bench_round_trip);
criterion_main!(benches);
