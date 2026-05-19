//! Counter — the Spark "hello world". Click `+1` and watch it tick without a page reload.

use anvilforge::prelude::*;

#[spark_component(template = "spark/counter")]
pub struct Counter {
    pub label: String,
    pub count: i32,
    #[spark(model)]
    pub draft: String,
}

#[spark_actions]
impl Counter {
    #[spark_mount]
    fn mount(props: MountProps) -> Self {
        Self {
            label: props.string("label").unwrap_or_else(|| "Clicks".to_string()),
            count: props.i32("initial").unwrap_or(0),
            draft: String::new(),
        }
    }

    async fn increment(&mut self) -> ::spark::Result<()> {
        self.count += 1;
        Ok(())
    }

    async fn add(&mut self, by: i32) -> ::spark::Result<()> {
        self.count += by;
        Ok(())
    }

    async fn reset(&mut self) -> ::spark::Result<()> {
        self.count = 0;
        Ok(())
    }
}
