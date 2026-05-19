//! Seeders and factories. Mirrors Laravel's `Illuminate\Database\Seeder` +
//! `Illuminate\Database\Eloquent\Factories\Factory`.
//!
//! A **seeder** is a unit struct implementing `Seeder` that knows how to
//! populate the DB with canonical data — e.g. `RolesSeeder` writes the same
//! three rows on every run.
//!
//! A **factory** is a unit struct implementing `Factory<M>` that generates
//! randomized fake data for tests + dev (via the `fake` crate).
//!
//! ```ignore
//! use anvilforge::prelude::*;
//! use anvilforge::seeder::{Seeder, Factory};
//! use anvilforge::async_trait::async_trait;
//!
//! pub struct RolesSeeder;
//! #[async_trait]
//! impl Seeder for RolesSeeder {
//!     async fn run(&self, c: &Container) -> Result<()> {
//!         for name in ["admin", "editor", "viewer"] {
//!             sqlx::query("INSERT INTO roles (name) VALUES ($1) ON CONFLICT DO NOTHING")
//!                 .bind(name).execute(c.pool()).await.map_err(Error::Database)?;
//!         }
//!         Ok(())
//!     }
//! }
//!
//! pub struct UserFactory;
//! impl Factory<User> for UserFactory {
//!     fn definition() -> User {
//!         use fake::{Fake, faker::{name::en::Name, internet::en::SafeEmail}};
//!         User {
//!             id: 0,
//!             name: Name().fake(),
//!             email: SafeEmail().fake(),
//!             ..Default::default()
//!         }
//!     }
//! }
//! ```

use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use parking_lot::RwLock;

use crate::container::Container;
use crate::Error;

/// A database seeder. Mirrors Laravel's `Seeder::run()`.
#[async_trait]
pub trait Seeder: Send + Sync {
    /// Human-readable name (defaults to type name via the derive).
    fn name(&self) -> &'static str {
        std::any::type_name::<Self>()
    }

    /// Run the seeder against the container.
    async fn run(&self, c: &Container) -> Result<(), Error>;
}

/// Boxed seeder. The scaffolded `DatabaseSeeder` holds a `Vec<BoxedSeeder>`.
pub type BoxedSeeder = Box<dyn Seeder>;

/// Inventory entry for a seeder. The `#[derive(Seeder)]` macro emits one of these
/// per type. `SeederRegistry::from_inventory()` builds a registry from every
/// registered seeder — apps never need to write a manual registration list.
pub struct SeederRegistration {
    pub name: &'static str,
    pub builder: fn() -> Arc<dyn Seeder>,
}

inventory::collect!(SeederRegistration);

/// Iterate every seeder registered via `#[derive(Seeder)]`.
pub fn collected() -> Vec<(&'static str, Arc<dyn Seeder>)> {
    inventory::iter::<SeederRegistration>
        .into_iter()
        .map(|r| (r.name, (r.builder)()))
        .collect()
}

/// Helper to call another seeder from inside one — mirrors `$this->call([...])`.
pub async fn call_seeders(c: &Container, seeders: &[BoxedSeeder]) -> Result<(), Error> {
    for s in seeders {
        tracing::info!(seeder = %s.name(), "running");
        s.run(c).await?;
    }
    Ok(())
}

/// Registry of named seeders — populated at app startup so `smith db:seed --class=Name`
/// can resolve a seeder by string name. The scaffolded `DatabaseSeeder` registers
/// every known seeder with the global `SEEDERS` registry.
#[derive(Default, Clone)]
pub struct SeederRegistry {
    inner: Arc<RwLock<HashMap<String, Arc<dyn Seeder>>>>,
}

impl SeederRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Build a registry from every seeder registered via `#[derive(Seeder)]`.
    /// Mirrors Laravel's auto-discovery of `database/seeders/*.php`.
    pub fn from_inventory() -> Self {
        let registry = Self::new();
        for (name, seeder) in collected() {
            registry.inner.write().insert(name.to_string(), seeder);
        }
        registry
    }

    pub fn register<S: Seeder + 'static>(&self, name: impl Into<String>, seeder: S) {
        self.inner.write().insert(name.into(), Arc::new(seeder));
    }

    pub fn get(&self, name: &str) -> Option<Arc<dyn Seeder>> {
        self.inner.read().get(name).cloned()
    }

    pub fn names(&self) -> Vec<String> {
        let mut names: Vec<String> = self.inner.read().keys().cloned().collect();
        names.sort();
        names
    }

    pub async fn run(&self, c: &Container, name: &str) -> Result<(), Error> {
        let seeder = self
            .get(name)
            .ok_or_else(|| Error::Internal(format!("unknown seeder: {name}")))?;
        seeder.run(c).await
    }
}

/// A model factory. Generates randomized fake instances of `M` for tests/dev.
///
/// Mirrors Laravel's `Post::factory()->count(50)->create()`. The `definition()`
/// method returns a single random instance; `make_many()` / `create_many()`
/// generate batches.
pub trait Factory<M>: Sized {
    /// Generate one random in-memory instance.
    fn definition() -> M;

    /// Generate `count` random in-memory instances (not persisted).
    fn make_many(count: usize) -> Vec<M> {
        (0..count).map(|_| Self::definition()).collect()
    }
}

/// Helper for factories that want to persist instances.
///
/// Implementing this manually per model lets the factory call into the model's
/// insert SQL.
#[async_trait]
pub trait PersistentFactory<M>: Factory<M>
where
    M: Send,
{
    async fn save(c: &Container, model: M) -> Result<M, Error>;

    /// Generate + persist a single instance.
    async fn create(c: &Container) -> Result<M, Error> {
        Self::save(c, Self::definition()).await
    }

    /// Generate + persist `count` instances. Returns them in insertion order.
    async fn create_many(c: &Container, count: usize) -> Result<Vec<M>, Error> {
        let mut out = Vec::with_capacity(count);
        for instance in Self::make_many(count) {
            out.push(Self::save(c, instance).await?);
        }
        Ok(out)
    }
}

/// Bind a model to its factory. Mirrors Laravel's convention of `Database\Factories\UserFactory`
/// being associated with `App\Models\User`.
///
/// Implement on the model:
/// ```ignore
/// impl HasFactory for User {
///     type Factory = UserFactory;
/// }
/// ```
///
/// Then `User::factory().count(50).create(&c).await?` works.
pub trait HasFactory: Sized {
    type Factory: Factory<Self>;

    fn factory() -> FactoryBuilder<Self, Self::Factory> {
        FactoryBuilder::new()
    }
}

/// Fluent builder returned by `Model::factory()`. Mirrors Laravel's
/// `Post::factory()->count(50)->create()`.
pub struct FactoryBuilder<M, F: Factory<M>> {
    count: usize,
    _phantom: std::marker::PhantomData<(M, F)>,
}

impl<M, F: Factory<M>> FactoryBuilder<M, F> {
    pub fn new() -> Self {
        Self {
            count: 1,
            _phantom: std::marker::PhantomData,
        }
    }

    /// Set how many instances the builder produces.
    pub fn count(mut self, n: usize) -> Self {
        self.count = n;
        self
    }

    /// Produce in-memory instances (no DB hit). Equivalent to Laravel's `->make()`.
    pub fn make(self) -> Vec<M> {
        F::make_many(self.count)
    }

    /// Produce + persist instances. Equivalent to Laravel's `->create()`.
    pub async fn create(self, c: &Container) -> Result<Vec<M>, Error>
    where
        M: Send,
        F: PersistentFactory<M> + Send,
    {
        F::create_many(c, self.count).await
    }

    /// Convenience: produce exactly one persisted instance.
    pub async fn create_one(self, c: &Container) -> Result<M, Error>
    where
        M: Send,
        F: PersistentFactory<M> + Send,
    {
        F::create(c).await
    }
}

impl<M, F: Factory<M>> Default for FactoryBuilder<M, F> {
    fn default() -> Self {
        Self::new()
    }
}
