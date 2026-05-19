# Seeders & factories

Anvilforge's seeders + factories mirror Laravel's `Seeder` + `Factory` — same shapes, same names, auto-discovered via `#[derive(Seeder)]` so you never maintain a manual registry.

## Seeders

```bash
smith make:seeder RolesSeeder
```

Writes `database/seeders/RolesSeeder.rs` and appends a `mod` line to `database/seeders/mod.rs`:

```rust
use anvilforge::prelude::*;
use anvilforge::seeder::Seeder;
use anvilforge::async_trait::async_trait;

#[derive(Seeder)]
pub struct RolesSeeder;

#[async_trait]
impl Seeder for RolesSeeder {
    fn name(&self) -> &'static str { "RolesSeeder" }

    async fn run(&self, c: &Container) -> Result<()> {
        for name in ["admin", "editor", "viewer"] {
            sqlx::query("INSERT INTO roles (name) VALUES ($1) ON CONFLICT DO NOTHING")
                .bind(name)
                .execute(c.pool())
                .await
                .map_err(Error::Database)?;
        }
        Ok(())
    }
}
```

`#[derive(Seeder)]` auto-registers the seeder with `inventory`. `SeederRegistry::from_inventory()` discovers every registered seeder; the scaffolded `DatabaseSeeder` calls this internally.

Dispatching to sub-seeders inside `DatabaseSeeder::run` — Laravel's `$this->call([UserSeeder::class, PostSeeder::class])`:

```rust
#[async_trait]
impl Seeder for DatabaseSeeder {
    async fn run(&self, c: &Container) -> Result<()> {
        let registry = Self::registry();   // auto-discovered
        registry.run(c, "RolesSeeder").await?;
        registry.run(c, "UserSeeder").await?;
        Ok(())
    }
}
```

## Running seeders

```bash
smith db:seed                       # runs DatabaseSeeder (the root)
smith db:seed --class=RolesSeeder   # runs a single seeder
smith migrate --seed                # migrate then seed
smith migrate:fresh --seed          # drop, migrate, seed
smith migrate:refresh --seed        # reset, migrate, seed
```

## Factories

```bash
smith make:factory UserFactory                 # infers model `User`
smith make:factory PostFactory --model=Post    # explicit
```

The factory implements three things:
1. `Factory<M>` — `definition()` generates a random in-memory `M`.
2. `PersistentFactory<M>` — `save()` inserts an instance.
3. `HasFactory` on the model — links `User::factory()` to `UserFactory`.

```rust
use anvilforge::prelude::*;
use anvilforge::seeder::{Factory, HasFactory, PersistentFactory};
use anvilforge::async_trait::async_trait;

use crate::app::Models::User;

pub struct UserFactory;

impl Factory<User> for UserFactory {
    fn definition() -> User {
        use fake::{Fake, faker::{name::en::Name, internet::en::SafeEmail}};
        User {
            id: 0,
            name: Name().fake(),
            email: SafeEmail().fake(),
            ..Default::default()
        }
    }
}

#[async_trait]
impl PersistentFactory<User> for UserFactory {
    async fn save(c: &Container, model: User) -> Result<User> {
        Ok(model.save(c.pool()).await?)   // uses the derive-generated `save()`
    }
}

impl HasFactory for User {
    type Factory = UserFactory;
}
```

## The Laravel invocation pattern

Once `HasFactory` is wired, the model's `factory()` static method is available:

```rust
use anvilforge::seeder::HasFactory;

// Laravel: User::factory()->count(5)->create()
let users: Vec<User> = User::factory()
    .count(5)
    .create(&container)
    .await?;

// Laravel: User::factory()->make()
let in_memory: Vec<User> = User::factory().count(3).make();

// Laravel: User::factory()->createOne()  (sugar for ->count(1)->create()->first())
let one: User = User::factory().create_one(&container).await?;
```

## Using factories in seeders

```rust
#[derive(Seeder)]
pub struct UserSeeder;

#[async_trait]
impl Seeder for UserSeeder {
    async fn run(&self, c: &Container) -> Result<()> {
        User::factory().count(25).create(c).await?;
        Ok(())
    }
}
```

## Using factories in tests

```rust
#[tokio::test]
async fn lists_recent_users() {
    let c = test_container().await;
    let _users = User::factory().count(10).create(&c).await.unwrap();

    let response = client.get("/users").await;
    response.assert_status(200);
}
```

[Next: sessions & users →](../auth/sessions.md)
