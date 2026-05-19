//! Microbenchmarks for the Spark component template render path.
//!
//! `spark::template::render` is the per-interaction hot path on the server side
//! (forge-codegen lowers `.forge.html` once → MiniJinja renders with JSON state
//! on every `POST /_spark/update`). This measures cold-render-first-call and
//! warm-render-cached-source.

use std::time::Duration;

use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion};
use serde_json::json;
use tempfile::TempDir;

const COUNTER_TEMPLATE: &str = r#"<div>
    <h2>{{ count }}</h2>
    <button spark:click="increment">+1</button>
    @if(has_draft)
        <p>Draft: {{ draft }}</p>
    @endif
</div>"#;

const LIST_TEMPLATE: &str = r#"<div>
    <h2>{{ title }} ({{ items|length }})</h2>
    <ul>
    @foreach(items as item)
        <li>{{ item.title }} — {{ item.active }}</li>
    @endforeach
    </ul>
</div>"#;

fn setup_views_dir(templates: &[(&str, &str)]) -> TempDir {
    let dir = tempfile::tempdir().expect("tempdir");
    // SPARK_VIEWS_DIR points at the directory the template paths are relative to.
    // Templates are named like "spark/counter", so the on-disk layout is
    //   $SPARK_VIEWS_DIR/spark/counter.forge.html
    let views_root = dir.path().join("views");
    let spark_dir = views_root.join("spark");
    std::fs::create_dir_all(&spark_dir).expect("mkdir");
    for (name, body) in templates {
        std::fs::write(spark_dir.join(format!("{name}.forge.html")), body).expect("write");
    }
    std::env::set_var("SPARK_VIEWS_DIR", &views_root);
    dir
}

fn bench_render_small(c: &mut Criterion) {
    let _dir = setup_views_dir(&[("counter", COUNTER_TEMPLATE)]);
    spark::template::clear_cache();

    let state = json!({"count": 42, "draft": "hello", "has_draft": true});
    let mut group = c.benchmark_group("template_render_small");
    group.measurement_time(Duration::from_secs(5));

    // Cold (clear cache before each iteration).
    group.bench_function(BenchmarkId::from_parameter("cold"), |b| {
        b.iter(|| {
            spark::template::clear_cache();
            spark::template::render("spark/counter", &state).unwrap()
        });
    });

    // Warm — template is parsed once, then reused.
    spark::template::render("spark/counter", &state).unwrap();
    group.bench_function(BenchmarkId::from_parameter("warm"), |b| {
        b.iter(|| spark::template::render("spark/counter", &state).unwrap());
    });

    group.finish();
}

fn bench_render_list(c: &mut Criterion) {
    let _dir = setup_views_dir(&[("list", LIST_TEMPLATE)]);
    spark::template::clear_cache();

    let mut group = c.benchmark_group("template_render_list");
    group.measurement_time(Duration::from_secs(5));

    for n in [10, 50, 200] {
        let state = json!({
            "title": "Inbox",
            "items": (0..n).map(|i| json!({
                "title": format!("Message {i}"),
                "active": i % 2 == 0,
            })).collect::<Vec<_>>(),
        });
        // Warm up the cache.
        spark::template::render("spark/list", &state).unwrap();
        group.bench_with_input(BenchmarkId::from_parameter(n), &state, |b, st| {
            b.iter(|| spark::template::render("spark/list", st).unwrap());
        });
    }
    group.finish();
}

criterion_group!(benches, bench_render_small, bench_render_list);
criterion_main!(benches);
