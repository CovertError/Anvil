# Migrations

Cast migrations are Rust structs implementing `cast::Migration` and marked with `#[derive(Migration)]`. The derive auto-registers them via `inventory`, so `MigrationRunner::new(pool)` discovers every migration in the workspace — no manual list to maintain. Same end result as Laravel auto-discovering `database/migrations/<timestamp>_*.php`.

## Generate

```bash
smith make:migration create_posts_table
```

Writes `database/migrations/<timestamp>_create_posts_table.rs` and **appends a `#[path = "..."] pub mod ...;` line to `database/migrations/mod.rs`** so the file is picked up by the compiler. Inventory then auto-registers it.

```rust
use anvilforge::prelude::*;
use anvilforge::cast::Schema;

#[derive(Migration)]
pub struct CreatePostsTable;

impl CastMigration for CreatePostsTable {
    fn name(&self) -> &'static str {
        "2026_01_01_120000_create_posts_table"
    }

    fn up(&self, s: &mut Schema) {
        s.create("posts", |t| {
            t.id();
            t.foreign_id_for("author_id", "authors");
            t.string("title").not_null();
            t.text("body").not_null();
            t.boolean("published").default("false");
            t.timestamps();
        });
    }

    fn down(&self, s: &mut Schema) {
        s.drop_if_exists("posts");
    }
}
```

## Schema builder — full reference

`Schema::create(name, |t| ...)` opens a `CREATE TABLE` block. `Schema::table(name, |t| ...)` opens an `ALTER TABLE` block.

### Identifier

| Method            | Result                                              |
| ----------------- | --------------------------------------------------- |
| `t.id()`          | `id BIGSERIAL PRIMARY KEY`                          |
| `t.uuid_id()`     | `id UUID PRIMARY KEY`                               |
| `t.ulid_id()`     | Alias for `uuid_id` (Postgres has no native ULID)   |

### Numeric

| Method                              | Postgres type                |
| ----------------------------------- | ---------------------------- |
| `t.tiny_integer(name)`              | `SMALLINT`                   |
| `t.small_integer(name)`             | `SMALLINT`                   |
| `t.integer(name)`                   | `INTEGER`                    |
| `t.big_integer(name)`               | `BIGINT`                     |
| `t.unsigned_integer(name)`          | `INTEGER` + `CHECK ≥ 0`      |
| `t.unsigned_big_integer(name)`      | `BIGINT` + `CHECK ≥ 0`       |
| `t.decimal(name, precision, scale)` | `DECIMAL(p, s)`              |
| `t.float(name)`                     | `REAL`                       |
| `t.double(name)`                    | `DOUBLE PRECISION`           |

### String

| Method                              | Postgres type           |
| ----------------------------------- | ----------------------- |
| `t.string(name)`                    | `VARCHAR(255)`          |
| `t.string_with_length(name, n)`     | `VARCHAR(n)`            |
| `t.char(name, n)`                   | `CHAR(n)`               |
| `t.text(name)`                      | `TEXT`                  |
| `t.long_text(name)` / `medium_text` | `TEXT`                  |
| `t.remember_token()`                | `VARCHAR(100) NULL` named `remember_token` |

### Enum / binary

| Method                                  | Result                                           |
| --------------------------------------- | ------------------------------------------------ |
| `t.enum_col(name, &["draft", "live"])` | `VARCHAR(64)` + `CHECK (col IN (...))`           |
| `t.binary(name)`                        | `BYTEA`                                          |

### Boolean / time

| Method                | Postgres type            |
| --------------------- | ------------------------ |
| `t.boolean(name)`     | `BOOLEAN`                |
| `t.timestamp(name)`   | `TIMESTAMP`              |
| `t.timestamp_tz(name)`| `TIMESTAMPTZ`            |
| `t.date(name)`        | `DATE`                   |
| `t.time(name)`        | `TIME`                   |
| `t.date_time(name)`   | `TIMESTAMP`              |
| `t.year(name)`        | `INTEGER` (year-only)    |
| `t.timestamps()`      | `created_at` + `updated_at` (both `TIMESTAMPTZ NULL DEFAULT NOW()`) |
| `t.soft_deletes()`    | `deleted_at TIMESTAMPTZ NULL` |

### JSON / UUID / network

| Method                  | Postgres type   |
| ----------------------- | --------------- |
| `t.json(name)`          | `JSON`          |
| `t.jsonb(name)`         | `JSONB`         |
| `t.uuid(name)`          | `UUID`          |
| `t.ip_address(name)`    | `VARCHAR(45)`   |
| `t.mac_address(name)`   | `VARCHAR(17)`   |

### Polymorphic (`morphs`)

```rust
t.morphs("commentable");
// → commentable_id BIGINT NOT NULL,
//   commentable_type VARCHAR(255) NOT NULL,
//   plus an index on (commentable_type, commentable_id)
```

Variants: `nullable_morphs(name)`, `uuid_morphs(name)`.

### Column modifiers (chain on any column)

```rust
t.string("email")
    .not_null()
    .unique()
    .default("''")
    .comment("primary contact");

t.timestamp("created_at").use_current();
```

| Modifier                 | Effect                          |
| ------------------------ | ------------------------------- |
| `.not_null()`            | `NOT NULL`                      |
| `.nullable()`            | `NULL`                          |
| `.unique()`              | `UNIQUE`                        |
| `.primary_key()`         | `PRIMARY KEY`                   |
| `.default("...")`        | `DEFAULT <expr>`                |
| `.default_value(v)`      | Typed default via sea-query     |
| `.use_current()`         | `DEFAULT CURRENT_TIMESTAMP`     |

### Indexes

```rust
t.index(&["author_id", "published"]);   // idx_<table>_author_id_published
t.unique_index(&["email"]);             // uq_<table>_email
t.raw_index("CREATE INDEX trgm_posts_title ON posts USING gin (title gin_trgm_ops)");
```

### Foreign keys

Two forms — shortcut and fluent:

```rust
// Shortcut: bigint column + FK to <table>.id with ON DELETE CASCADE
t.foreign_id_for("author_id", "authors");

// Fluent (matches Laravel's $table->foreign()->references()->on()):
t.big_integer("editor_id").nullable();
t.foreign("editor_id").references("id").on("users").set_null();
```

Chainable actions on the fluent builder: `.cascade()`, `.set_null()`, `.restrict()`, `.on_delete("...")`, `.on_update("...")`.

### Schema::table — ALTER operations

```rust
s.table("posts", |t| {
    t.string("slug").nullable();          // ADD COLUMN
    t.integer("read_count").default("0");
    t.rename_column("body", "content");   // RENAME COLUMN
    t.drop_column("legacy_html");         // DROP COLUMN IF EXISTS
    t.drop_index("idx_posts_title");
    t.drop_foreign("fk_posts_author_id");
});
```

### Other Schema methods

```rust
s.drop("posts");
s.drop_if_exists("posts");
s.rename("posts", "articles");
s.raw("CREATE EXTENSION IF NOT EXISTS pg_trgm");
```

## Auto-registration

The `#[derive(Migration)]` macro emits an `inventory::submit!` entry. `MigrationRunner::new(pool)` calls `inventory::iter` to discover every registered migration in the binary — no manual list to maintain.

`smith make:migration` does two things:
1. Writes the migration file with `#[derive(Migration)]` already applied.
2. Appends a `#[path = "..."] pub mod xxx;` line to `database/migrations/mod.rs` so the file gets compiled.

You don't touch any other file.

## CLI — the full Laravel migrate:* surface

```bash
smith migrate                       # apply pending migrations
smith migrate --step                # one batch per migration (rollback granularity)
smith migrate --pretend             # print SQL without executing
smith migrate --seed                # apply + run seeders

smith migrate:rollback              # undo the last batch
smith migrate:rollback --steps=3    # undo the last N batches

smith migrate:reset                 # undo every applied migration
smith migrate:refresh               # reset + re-run
smith migrate:refresh --seed        # reset + re-run + seed
smith migrate:fresh                 # DROP SCHEMA + migrate
smith migrate:fresh --seed          # DROP + migrate + seed

smith migrate:install               # just create the migrations table
smith migrate:status                # show applied vs pending

smith db:seed                       # run seeders only
smith db:wipe                       # DROP SCHEMA without re-migrating
```

`smith migrate:status` output:

```
Migration                                                     Status    Batch
------------------------------------------------------------  --------  -----
2026_01_01_000001_create_users_table                          applied   1
2026_01_01_000002_create_posts_table                          applied   1
2026_05_18_120000_add_slug_to_posts                           pending   -
```

## Batches and rollbacks

Every `smith migrate` records a *batch number* in the `migrations` table. `rollback` undoes everything in the highest batch. `rollback --steps=N` undoes the last N batches. `--step` puts each migration in its own batch so each one can be rolled back individually.

[Next: sessions & users →](../auth/sessions.md)
