//! Spark reactive components. Each module is `mod`-included here.
//! Components register themselves at startup via `inventory` from `#[spark_component]`.

#[path = "Counter.rs"]
pub mod counter;
pub use counter::Counter;
