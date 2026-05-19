//! Spark prelude — items components and apps typically need.
//!
//! ```ignore
//! use spark::prelude::*;
//! ```

pub use crate::broadcast::{broadcast, SparkBroadcast};
pub use crate::component::{
    BrowserDispatch, Component, ComponentEmit, Ctx, MountProps, PropertyWrite,
};
pub use crate::install::{install, install_routes};
pub use crate::registry::{BoxedComponent, ComponentEntry, DynComponent};
pub use crate::Result;

pub use spark_derive::{
    actions as spark_actions, component as spark_component, mount as spark_mount, on as spark_on,
    updated as spark_updated,
};
