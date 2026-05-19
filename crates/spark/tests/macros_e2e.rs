//! End-to-end exercise of `#[spark_component]` + `#[spark_actions]` against
//! the live registry + snapshot pipeline. No HTTP server, no DB — just the core
//! mount → dispatch → re-encode loop.

use serde::{Deserialize, Serialize};

use spark::component::{Ctx, MountProps};
use spark::registry;
use spark::snapshot::{self, Memo};
use spark::Component;
use spark_derive::{actions, component};

#[component(template = "spark/test_counter")]
#[derive(Serialize, Deserialize)]
pub struct TestCounter {
    pub count: i32,
    #[spark(model)]
    pub draft: String,
}

#[actions]
impl TestCounter {
    #[spark_derive::mount]
    fn mount(props: MountProps) -> Self {
        Self {
            count: props.i32("initial").unwrap_or(0),
            draft: String::new(),
        }
    }

    async fn increment(&mut self) -> spark::Result<()> {
        self.count += 1;
        Ok(())
    }

    async fn add(&mut self, by: i32) -> spark::Result<()> {
        self.count += by;
        Ok(())
    }
}

const TEST_KEY: &str = "spark-test-app-key-thirty-two-bb";

#[test]
fn registry_finds_test_component_by_short_name() {
    let entry =
        registry::resolve("TestCounter").expect("TestCounter should be registered via inventory");
    assert!(entry.class.ends_with("::TestCounter"));
    assert_eq!(entry.view, "spark/test_counter");
}

#[test]
fn mount_then_dispatch_then_reencode() {
    let entry = registry::resolve("TestCounter").unwrap();
    let mut boxed = (entry.mount)(MountProps::new(serde_json::json!({ "initial": 5 })));

    // initial state.
    let initial_data = boxed.state.snapshot_data();
    assert_eq!(initial_data["count"], 5);

    // dispatch `add(3)`.
    let mut ctx = Ctx::new(None);
    let fut = boxed
        .state
        .dispatch_call("add", vec![serde_json::json!(3)], &mut ctx);
    futures::executor::block_on(fut).unwrap();

    let after = boxed.state.snapshot_data();
    assert_eq!(after["count"], 8);

    // round-trip the resulting snapshot.
    let memo = Memo {
        id: "test-id".into(),
        class: entry.class.to_string(),
        view: entry.view.to_string(),
        listeners: Vec::new(),
        errors: None,
    };
    let envelope = snapshot::Envelope::build(TEST_KEY, after.clone(), memo.clone());
    let wire = snapshot::encode(&envelope, TEST_KEY, false).unwrap();
    let decoded = snapshot::decode(&wire, TEST_KEY).unwrap();
    assert_eq!(decoded.data, after);
    assert_eq!(decoded.memo.class, entry.class);

    // hydrate via registry.load and confirm the state survives.
    let mut rehydrated = (entry.load)(&decoded.data).unwrap();
    let rehydrated_data = rehydrated.state.snapshot_data();
    assert_eq!(rehydrated_data["count"], 8);

    // dispatch one more action on the rehydrated instance.
    let mut ctx2 = Ctx::new(None);
    futures::executor::block_on(
        rehydrated
            .state
            .dispatch_call("increment", vec![], &mut ctx2),
    )
    .unwrap();
    assert_eq!(rehydrated.state.snapshot_data()["count"], 9);
}

#[test]
fn property_writes_only_affect_model_fields() {
    let entry = registry::resolve("TestCounter").unwrap();
    let mut boxed = (entry.mount)(MountProps::new(serde_json::json!({})));
    let mut ctx = Ctx::new(None);

    // Writing to the model field works.
    let writes = vec![spark::PropertyWrite {
        name: "draft".into(),
        value: serde_json::json!("hello"),
    }];
    futures::executor::block_on(boxed.state.apply_writes(&writes, &mut ctx)).unwrap();
    assert_eq!(boxed.state.snapshot_data()["draft"], "hello");

    // Writing to a non-model field is ignored (no error).
    let writes = vec![spark::PropertyWrite {
        name: "count".into(),
        value: serde_json::json!(999),
    }];
    futures::executor::block_on(boxed.state.apply_writes(&writes, &mut ctx)).unwrap();
    assert_eq!(boxed.state.snapshot_data()["count"], 0);
}

#[test]
fn unknown_method_is_a_clear_error() {
    let entry = registry::resolve("TestCounter").unwrap();
    let mut boxed = (entry.mount)(MountProps::new(serde_json::json!({})));
    let mut ctx = Ctx::new(None);

    let err = futures::executor::block_on(boxed.state.dispatch_call("nope", vec![], &mut ctx))
        .unwrap_err();
    match err {
        spark::Error::UnknownMethod { method, .. } => assert_eq!(method, "nope"),
        other => panic!("expected UnknownMethod, got {other:?}"),
    }
}
