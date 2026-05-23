//! Schema builder. Used in migrations: `Schema::create("users", |t| ...)`.
//!
//! Mirrors Laravel's `Schema::create` ergonomics + the full `Blueprint` column
//! type surface: `t.string("name").not_null().unique()`, `t.decimal("price", 10, 2)`,
//! `t.morphs("commentable")`, `t.remember_token()`, foreign-key constraint builders, etc.
//!
//! ## Dialects
//!
//! `Schema::new()` defaults to Postgres. For MySQL / SQLite, use `Schema::for_driver(Driver::*)`.
//! The MigrationRunner calls `Schema::for_driver(pool.driver())` automatically so user code
//! rarely needs to think about it.

use sea_query::{
    ColumnDef as SeaColumnDef, ColumnType, MysqlQueryBuilder, PostgresQueryBuilder,
    SqliteQueryBuilder, Table as SeaTable,
};

use crate::pool::Driver;

pub struct Schema {
    pub statements: Vec<String>,
    driver: Driver,
}

impl Default for Schema {
    fn default() -> Self {
        Self::new()
    }
}

impl Schema {
    pub fn new() -> Self {
        Self::for_driver(Driver::Postgres)
    }

    pub fn for_driver(driver: Driver) -> Self {
        Self {
            statements: Vec::new(),
            driver,
        }
    }

    pub fn driver(&self) -> Driver {
        self.driver
    }

    /// Create a new table.
    pub fn create<F>(&mut self, table: &str, build: F)
    where
        F: FnOnce(&mut Table),
    {
        let mut t = Table::new(table, TableMode::Create, self.driver);
        build(&mut t);
        self.statements.extend(t.into_statements());
    }

    /// Alter an existing table. The closure can call `add_*` / `drop_column` /
    /// `rename_column` / index / foreign-key methods on `Table`.
    ///
    /// Mirrors `Schema::table('users', function (Blueprint $table) { ... })`.
    pub fn table<F>(&mut self, table: &str, build: F)
    where
        F: FnOnce(&mut Table),
    {
        let mut t = Table::new(table, TableMode::Alter, self.driver);
        build(&mut t);
        self.statements.extend(t.into_statements());
    }

    pub fn drop(&mut self, table: &str) {
        let sql = match self.driver {
            Driver::Postgres => format!("DROP TABLE IF EXISTS {} CASCADE", table),
            Driver::MySql | Driver::Sqlite => format!("DROP TABLE IF EXISTS {}", table),
        };
        self.statements.push(sql);
    }

    pub fn drop_if_exists(&mut self, table: &str) {
        self.drop(table);
    }

    /// Rename a table. Mirrors `Schema::rename('old', 'new')`.
    pub fn rename(&mut self, from: &str, to: &str) {
        self.statements
            .push(format!("ALTER TABLE {from} RENAME TO {to}"));
    }

    /// Check if a table exists (executed at apply-time as a SELECT). v0.2 will return bool.
    pub fn has_table(&mut self, _table: &str) {
        // Sentinel — meant for runtime use, not migration generation.
    }

    pub fn raw(&mut self, sql: impl Into<String>) {
        self.statements.push(sql.into());
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum TableMode {
    Create,
    Alter,
}

/// A table definition assembled inside the build closure.
pub struct Table {
    name: String,
    mode: TableMode,
    driver: Driver,
    columns: Vec<ColumnDef>,
    indexes: Vec<String>,
    foreign_keys: Vec<PendingFk>,
    drops: Vec<String>,
    renames: Vec<(String, String)>,
    checks: Vec<PendingCheck>,
    /// Composite-primary-key column list. Empty means "no composite PK"
    /// — single-column PKs (via `t.id()`) are tracked on the column itself
    /// and don't appear here.
    primary_keys: Vec<String>,
}

#[derive(Clone)]
struct PendingFk {
    column: String,
    ref_table: String,
    ref_col: String,
    on_delete: Option<String>,
    on_update: Option<String>,
}

impl PendingFk {
    fn constraint_name(&self, table: &str) -> String {
        format!("fk_{}_{}", table, self.column)
    }

    fn actions(&self) -> String {
        let mut s = String::new();
        if let Some(action) = &self.on_delete {
            s.push_str(&format!(" ON DELETE {action}"));
        }
        if let Some(action) = &self.on_update {
            s.push_str(&format!(" ON UPDATE {action}"));
        }
        s
    }

    fn inline_clause(&self, table: &str) -> String {
        format!(
            "CONSTRAINT {} FOREIGN KEY ({}) REFERENCES {} ({}){}",
            self.constraint_name(table),
            self.column,
            self.ref_table,
            self.ref_col,
            self.actions(),
        )
    }

    fn alter_sql(&self, table: &str) -> String {
        format!(
            "ALTER TABLE {} ADD CONSTRAINT {} FOREIGN KEY ({}) REFERENCES {} ({}){}",
            table,
            self.constraint_name(table),
            self.column,
            self.ref_table,
            self.ref_col,
            self.actions(),
        )
    }
}

#[derive(Clone)]
struct PendingCheck {
    name: String,
    expr: String,
}

impl PendingCheck {
    fn inline_clause(&self) -> String {
        format!("CONSTRAINT {} CHECK ({})", self.name, self.expr)
    }

    fn alter_sql(&self, table: &str) -> String {
        format!(
            "ALTER TABLE {} ADD CONSTRAINT {} CHECK ({})",
            table, self.name, self.expr
        )
    }
}

impl Table {
    fn new(name: impl Into<String>, mode: TableMode, driver: Driver) -> Self {
        Self {
            name: name.into(),
            mode,
            driver,
            columns: Vec::new(),
            indexes: Vec::new(),
            foreign_keys: Vec::new(),
            drops: Vec::new(),
            renames: Vec::new(),
            checks: Vec::new(),
            primary_keys: Vec::new(),
        }
    }

    fn push_column(&mut self, name: &str, ty: ColumnType) -> &mut ColumnDef {
        let sea_def = SeaColumnDef::new_with_type(sea_query::Alias::new(name), ty);
        self.columns.push(ColumnDef {
            sea_def,
            name: name.to_string(),
            mode: self.mode,
        });
        self.columns.last_mut().unwrap()
    }

    // ── identifier ───────────────────────────────────────────────────────────

    /// Declare a composite PRIMARY KEY across the named columns. Mirrors
    /// Laravel's `$table->primary(['user_id', 'role_id'])`. Use this for
    /// pivot tables like `model_has_roles` where the row identity is the
    /// combination of foreign keys.
    ///
    /// Inlines `PRIMARY KEY (col1, col2)` into the CREATE TABLE body.
    /// On Postgres/MySQL it would otherwise need
    /// `ALTER TABLE … ADD CONSTRAINT … PRIMARY KEY (…)`; SQLite has no
    /// such ALTER, so inline is the only portable form.
    ///
    /// Don't combine with `t.id()` on the same table — that creates a
    /// single-column `id` primary key, and adding a composite one is a
    /// conflict the database will reject.
    pub fn primary(&mut self, columns: &[&str]) -> &mut Self {
        if columns.is_empty() {
            return self;
        }
        self.primary_keys = columns.iter().map(|c| (*c).to_string()).collect();
        self
    }

    pub fn id(&mut self) -> &mut ColumnDef {
        let cd = self.push_column("id", ColumnType::BigInteger);
        cd.sea_def.not_null().primary_key().auto_increment();
        cd
    }

    pub fn uuid_id(&mut self) -> &mut ColumnDef {
        let cd = self.push_column("id", ColumnType::Uuid);
        cd.sea_def.not_null().primary_key();
        cd
    }

    /// `ULID` placeholder — alias for `uuid_id` (we don't ship a ULID Postgres type).
    pub fn ulid_id(&mut self) -> &mut ColumnDef {
        self.uuid_id()
    }

    // ── numeric ──────────────────────────────────────────────────────────────

    pub fn tiny_integer(&mut self, name: &str) -> &mut ColumnDef {
        self.push_column(name, ColumnType::TinyInteger)
    }

    pub fn small_integer(&mut self, name: &str) -> &mut ColumnDef {
        self.push_column(name, ColumnType::SmallInteger)
    }

    pub fn medium_integer(&mut self, name: &str) -> &mut ColumnDef {
        self.push_column(name, ColumnType::Integer)
    }

    pub fn integer(&mut self, name: &str) -> &mut ColumnDef {
        self.push_column(name, ColumnType::Integer)
    }

    pub fn big_integer(&mut self, name: &str) -> &mut ColumnDef {
        self.push_column(name, ColumnType::BigInteger)
    }

    /// Postgres has no native unsigned types, so this is a synonym for `big_integer`
    /// with a `>= 0` check constraint. Provided for Laravel parity.
    pub fn unsigned_big_integer(&mut self, name: &str) -> &mut ColumnDef {
        self.checks.push(PendingCheck {
            name: format!("{}_{}_unsigned", self.name, name),
            expr: format!("{} >= 0", name),
        });
        self.push_column(name, ColumnType::BigInteger)
    }

    pub fn unsigned_integer(&mut self, name: &str) -> &mut ColumnDef {
        self.checks.push(PendingCheck {
            name: format!("{}_{}_unsigned", self.name, name),
            expr: format!("{} >= 0", name),
        });
        self.push_column(name, ColumnType::Integer)
    }

    pub fn decimal(&mut self, name: &str, precision: u32, scale: u32) -> &mut ColumnDef {
        self.push_column(name, ColumnType::Decimal(Some((precision, scale))))
    }

    pub fn float(&mut self, name: &str) -> &mut ColumnDef {
        self.push_column(name, ColumnType::Float)
    }

    pub fn double(&mut self, name: &str) -> &mut ColumnDef {
        self.push_column(name, ColumnType::Double)
    }

    // ── string-ish ───────────────────────────────────────────────────────────

    pub fn string(&mut self, name: &str) -> &mut ColumnDef {
        self.push_column(name, ColumnType::String(sea_query::StringLen::N(255)))
    }

    /// Variable-length string with a custom max.
    pub fn string_with_length(&mut self, name: &str, length: u32) -> &mut ColumnDef {
        self.push_column(name, ColumnType::String(sea_query::StringLen::N(length)))
    }

    pub fn text(&mut self, name: &str) -> &mut ColumnDef {
        self.push_column(name, ColumnType::Text)
    }

    pub fn long_text(&mut self, name: &str) -> &mut ColumnDef {
        self.push_column(name, ColumnType::Text)
    }

    pub fn medium_text(&mut self, name: &str) -> &mut ColumnDef {
        self.push_column(name, ColumnType::Text)
    }

    pub fn char(&mut self, name: &str, length: u32) -> &mut ColumnDef {
        self.push_column(name, ColumnType::Char(Some(length)))
    }

    /// Laravel's `remember_token`: nullable VARCHAR(100) used by stay-logged-in cookies.
    pub fn remember_token(&mut self) -> &mut ColumnDef {
        let cd = self.push_column(
            "remember_token",
            ColumnType::String(sea_query::StringLen::N(100)),
        );
        cd.sea_def.null();
        cd
    }

    // ── enum / binary ────────────────────────────────────────────────────────

    /// CHECK-constrained enum column. Postgres has native ENUM types but they're
    /// painful for migrations; this models them as `VARCHAR` + CHECK constraint.
    pub fn enum_col(&mut self, name: &str, variants: &[&str]) -> &mut ColumnDef {
        let list = variants
            .iter()
            .map(|v| format!("'{}'", v.replace('\'', "''")))
            .collect::<Vec<_>>()
            .join(", ");
        self.checks.push(PendingCheck {
            name: format!("{}_{}_enum", self.name, name),
            expr: format!("{} IN ({})", name, list),
        });
        self.push_column(name, ColumnType::String(sea_query::StringLen::N(64)))
    }

    pub fn binary(&mut self, name: &str) -> &mut ColumnDef {
        self.push_column(name, ColumnType::VarBinary(sea_query::StringLen::None))
    }

    // ── boolean ──────────────────────────────────────────────────────────────

    pub fn boolean(&mut self, name: &str) -> &mut ColumnDef {
        self.push_column(name, ColumnType::Boolean)
    }

    // ── time ─────────────────────────────────────────────────────────────────

    pub fn timestamp(&mut self, name: &str) -> &mut ColumnDef {
        self.push_column(name, ColumnType::Timestamp)
    }

    pub fn timestamp_tz(&mut self, name: &str) -> &mut ColumnDef {
        self.push_column(name, ColumnType::TimestampWithTimeZone)
    }

    pub fn date(&mut self, name: &str) -> &mut ColumnDef {
        self.push_column(name, ColumnType::Date)
    }

    pub fn time(&mut self, name: &str) -> &mut ColumnDef {
        self.push_column(name, ColumnType::Time)
    }

    pub fn date_time(&mut self, name: &str) -> &mut ColumnDef {
        self.push_column(name, ColumnType::DateTime)
    }

    pub fn year(&mut self, name: &str) -> &mut ColumnDef {
        self.push_column(name, ColumnType::Year)
    }

    /// Adds `created_at` + `updated_at`, both `TIMESTAMPTZ NULL DEFAULT CURRENT_TIMESTAMP`.
    pub fn timestamps(&mut self) {
        self.push_column("created_at", ColumnType::TimestampWithTimeZone)
            .nullable()
            .default("CURRENT_TIMESTAMP");
        self.push_column("updated_at", ColumnType::TimestampWithTimeZone)
            .nullable()
            .default("CURRENT_TIMESTAMP");
    }

    pub fn soft_deletes(&mut self) {
        self.push_column("deleted_at", ColumnType::TimestampWithTimeZone)
            .nullable();
    }

    // ── json / uuid / network ────────────────────────────────────────────────

    pub fn json(&mut self, name: &str) -> &mut ColumnDef {
        self.push_column(name, ColumnType::Json)
    }

    pub fn jsonb(&mut self, name: &str) -> &mut ColumnDef {
        self.push_column(name, ColumnType::JsonBinary)
    }

    pub fn uuid(&mut self, name: &str) -> &mut ColumnDef {
        self.push_column(name, ColumnType::Uuid)
    }

    pub fn ip_address(&mut self, name: &str) -> &mut ColumnDef {
        // sea-query has no native INET; emit as VARCHAR(45) (max IPv6 len).
        self.push_column(name, ColumnType::String(sea_query::StringLen::N(45)))
    }

    pub fn mac_address(&mut self, name: &str) -> &mut ColumnDef {
        self.push_column(name, ColumnType::String(sea_query::StringLen::N(17)))
    }

    // ── polymorphic / morphs ─────────────────────────────────────────────────

    /// Polymorphic FK columns: `<name>_id BIGINT` + `<name>_type VARCHAR(255)`.
    /// Mirrors Laravel's `$table->morphs('commentable')`.
    pub fn morphs(&mut self, name: &str) {
        self.push_column(&format!("{name}_id"), ColumnType::BigInteger)
            .not_null();
        self.push_column(
            &format!("{name}_type"),
            ColumnType::String(sea_query::StringLen::N(255)),
        )
        .not_null();
        let idx_name = format!("idx_{}_{}_type_id", self.name, name);
        let sql = format!(
            "CREATE INDEX {} ON {} ({}_type, {}_id)",
            idx_name, self.name, name, name
        );
        self.indexes.push(sql);
    }

    pub fn nullable_morphs(&mut self, name: &str) {
        self.push_column(&format!("{name}_id"), ColumnType::BigInteger)
            .nullable();
        self.push_column(
            &format!("{name}_type"),
            ColumnType::String(sea_query::StringLen::N(255)),
        )
        .nullable();
        let idx_name = format!("idx_{}_{}_type_id", self.name, name);
        let sql = format!(
            "CREATE INDEX {} ON {} ({}_type, {}_id)",
            idx_name, self.name, name, name
        );
        self.indexes.push(sql);
    }

    /// UUID polymorphic variant: `<name>_id UUID` + `<name>_type VARCHAR(255)`.
    pub fn uuid_morphs(&mut self, name: &str) {
        self.push_column(&format!("{name}_id"), ColumnType::Uuid)
            .not_null();
        self.push_column(
            &format!("{name}_type"),
            ColumnType::String(sea_query::StringLen::N(255)),
        )
        .not_null();
        let idx_name = format!("idx_{}_{}_type_id", self.name, name);
        let sql = format!(
            "CREATE INDEX {} ON {} ({}_type, {}_id)",
            idx_name, self.name, name, name
        );
        self.indexes.push(sql);
    }

    // ── foreign keys ─────────────────────────────────────────────────────────

    /// Shortcut: add a `bigint` column with a FK to `references.id`. Laravel's
    /// `$table->foreignId('user_id')->constrained()` is split here into:
    ///
    /// - `t.foreign_id_for("user_id", "users")` — most common pattern
    /// - `t.big_integer("user_id")` + `t.foreign("user_id").references("id").on("users")` — explicit
    pub fn foreign_id_for(&mut self, name: &str, references: &str) -> &mut ColumnDef {
        self.foreign_id_for_with_action(name, references, "CASCADE", false)
    }

    /// Same as [`foreign_id_for`] but emits `ON DELETE SET NULL` and makes
    /// the column nullable. Matches Laravel's
    /// `$table->foreignId('user_id')->nullable()->constrained()->nullOnDelete()`.
    pub fn foreign_id_for_nullable(&mut self, name: &str, references: &str) -> &mut ColumnDef {
        self.foreign_id_for_with_action(name, references, "SET NULL", true)
    }

    /// Same as [`foreign_id_for`] but emits `ON DELETE RESTRICT`. Matches
    /// `$table->foreignId('order_id')->constrained()->restrictOnDelete()`.
    pub fn foreign_id_for_restrict(&mut self, name: &str, references: &str) -> &mut ColumnDef {
        self.foreign_id_for_with_action(name, references, "RESTRICT", false)
    }

    /// Same as [`foreign_id_for`] but emits no `ON DELETE` clause at all —
    /// the database's default (usually NO ACTION) applies. Use when you need
    /// a foreign-key column without a tied cascade policy.
    pub fn foreign_id_for_no_action(&mut self, name: &str, references: &str) -> &mut ColumnDef {
        let cd = {
            self.foreign_keys.push(PendingFk {
                column: name.to_string(),
                ref_table: references.to_string(),
                ref_col: "id".to_string(),
                on_delete: None,
                on_update: None,
            });
            self.push_column(name, ColumnType::BigInteger)
        };
        cd
    }

    fn foreign_id_for_with_action(
        &mut self,
        name: &str,
        references: &str,
        on_delete: &str,
        nullable: bool,
    ) -> &mut ColumnDef {
        self.foreign_keys.push(PendingFk {
            column: name.to_string(),
            ref_table: references.to_string(),
            ref_col: "id".to_string(),
            on_delete: Some(on_delete.to_string()),
            on_update: None,
        });
        let cd = self.push_column(name, ColumnType::BigInteger);
        if nullable {
            cd.nullable();
        }
        cd
    }

    /// Begin a fluent foreign-key constraint builder for `column`.
    /// Mirrors `$table->foreign('user_id')->references('id')->on('users')`.
    pub fn foreign(&mut self, column: &str) -> ForeignKeyBuilder<'_> {
        ForeignKeyBuilder {
            table: &mut self.foreign_keys,
            column: column.to_string(),
            ref_col: "id".to_string(),
            ref_table: String::new(),
            on_delete: None,
            on_update: None,
        }
    }

    // ── indexes ──────────────────────────────────────────────────────────────

    pub fn index(&mut self, columns: &[&str]) -> &mut Self {
        let idx_name = format!("idx_{}_{}", self.name, columns.join("_"));
        let sql = format!(
            "CREATE INDEX {} ON {} ({})",
            idx_name,
            self.name,
            columns.join(", ")
        );
        self.indexes.push(sql);
        self
    }

    pub fn unique_index(&mut self, columns: &[&str]) -> &mut Self {
        let idx_name = format!("uq_{}_{}", self.name, columns.join("_"));
        let sql = format!(
            "CREATE UNIQUE INDEX {} ON {} ({})",
            idx_name,
            self.name,
            columns.join(", ")
        );
        self.indexes.push(sql);
        self
    }

    /// Postgres trigram / GIN indexes are common — let users emit raw `CREATE INDEX … USING …`.
    pub fn raw_index(&mut self, sql: impl Into<String>) -> &mut Self {
        self.indexes.push(sql.into());
        self
    }

    // ── alter-table operations ───────────────────────────────────────────────

    pub fn drop_column(&mut self, name: &str) -> &mut Self {
        self.drops.push(format!(
            "ALTER TABLE {} DROP COLUMN IF EXISTS {}",
            self.name, name
        ));
        self
    }

    pub fn rename_column(&mut self, from: &str, to: &str) -> &mut Self {
        self.renames.push((from.to_string(), to.to_string()));
        self
    }

    pub fn drop_index(&mut self, name: &str) -> &mut Self {
        self.drops.push(format!("DROP INDEX IF EXISTS {}", name));
        self
    }

    pub fn drop_foreign(&mut self, constraint: &str) -> &mut Self {
        self.drops.push(format!(
            "ALTER TABLE {} DROP CONSTRAINT IF EXISTS {}",
            self.name, constraint
        ));
        self
    }

    pub fn drop_unique(&mut self, name: &str) -> &mut Self {
        self.drops.push(format!("DROP INDEX IF EXISTS {}", name));
        self
    }

    pub fn drop_timestamps(&mut self) -> &mut Self {
        self.drop_column("created_at").drop_column("updated_at")
    }

    pub fn drop_soft_deletes(&mut self) -> &mut Self {
        self.drop_column("deleted_at")
    }

    // ── emit ────────────────────────────────────────────────────────────────

    fn into_statements(self) -> Vec<String> {
        let mut out = Vec::new();
        match self.mode {
            TableMode::Create => {
                let mut t = SeaTable::create();
                t.table(sea_query::Alias::new(&self.name)).if_not_exists();
                for col in &self.columns {
                    t.col(col.sea_def.clone());
                }
                let mut sql = build_per_driver(&t, self.driver);

                // Inline FK + CHECK constraints + composite PK inside the
                // CREATE TABLE body. SQLite has no `ALTER TABLE … ADD
                // CONSTRAINT`, so emitting them post-create only works on
                // Postgres/MySQL; inline is portable.
                let mut inline = Vec::new();
                if !self.primary_keys.is_empty() {
                    inline.push(format!("PRIMARY KEY ({})", self.primary_keys.join(", ")));
                }
                for fk in &self.foreign_keys {
                    inline.push(fk.inline_clause(&self.name));
                }
                for chk in &self.checks {
                    inline.push(chk.inline_clause());
                }
                if !inline.is_empty() {
                    let trimmed_len = sql.trim_end().len();
                    if trimmed_len > 0 && sql.as_bytes()[trimmed_len - 1] == b')' {
                        let injection = format!(", {}", inline.join(", "));
                        sql.insert_str(trimmed_len - 1, &injection);
                    } else {
                        // Fallback: shouldn't happen for sea_query CREATE TABLE
                        // output, but if the SQL doesn't end in `)`, fall back to
                        // post-table ALTER statements (Postgres/MySQL only).
                        for fk in &self.foreign_keys {
                            out.push(fk.alter_sql(&self.name));
                        }
                        for chk in &self.checks {
                            out.push(chk.alter_sql(&self.name));
                        }
                    }
                }

                out.push(sql);
            }
            TableMode::Alter => {
                // `ALTER TABLE ... ADD COLUMN ...` per column added.
                for col in &self.columns {
                    let mut t = SeaTable::alter();
                    t.table(sea_query::Alias::new(&self.name));
                    t.add_column(col.sea_def.clone());
                    out.push(build_alter_per_driver(&t, self.driver));
                }

                let has_constraints = !self.foreign_keys.is_empty() || !self.checks.is_empty();
                if self.driver == Driver::Sqlite && has_constraints {
                    tracing::warn!(
                        table = %self.name,
                        fks = self.foreign_keys.len(),
                        checks = self.checks.len(),
                        "SQLite does not support ALTER TABLE ADD CONSTRAINT; FK/CHECK additions on existing tables are skipped. Recreate the table with the constraint inline.",
                    );
                } else {
                    for fk in &self.foreign_keys {
                        out.push(fk.alter_sql(&self.name));
                    }
                    for chk in &self.checks {
                        out.push(chk.alter_sql(&self.name));
                    }
                }
            }
        }
        for (from, to) in &self.renames {
            out.push(format!(
                "ALTER TABLE {} RENAME COLUMN {} TO {}",
                self.name, from, to
            ));
        }
        out.extend(self.drops);
        out.extend(self.indexes);
        out
    }
}

/// Fluent builder returned by `Table::foreign(col)`. Drop it (or call `.constrain()`)
/// to commit the foreign key SQL.
pub struct ForeignKeyBuilder<'a> {
    table: &'a mut Vec<PendingFk>,
    column: String,
    ref_col: String,
    ref_table: String,
    on_delete: Option<String>,
    on_update: Option<String>,
}

impl<'a> ForeignKeyBuilder<'a> {
    /// The referenced column on the foreign table. Default: `"id"`.
    pub fn references(mut self, column: &str) -> Self {
        self.ref_col = column.to_string();
        self
    }

    /// The foreign table.
    pub fn on(mut self, table: &str) -> Self {
        self.ref_table = table.to_string();
        self
    }

    /// `ON DELETE CASCADE` (or `RESTRICT` / `SET NULL` / `SET DEFAULT`).
    pub fn on_delete(mut self, action: &str) -> Self {
        self.on_delete = Some(action.to_string());
        self
    }

    pub fn on_update(mut self, action: &str) -> Self {
        self.on_update = Some(action.to_string());
        self
    }

    pub fn cascade(self) -> Self {
        self.on_delete("CASCADE")
    }

    pub fn set_null(self) -> Self {
        self.on_delete("SET NULL")
    }

    pub fn restrict(self) -> Self {
        self.on_delete("RESTRICT")
    }

    /// Commit the constraint to the table. Called explicitly OR implicitly via `Drop`.
    pub fn commit(self) {
        // moved out by Drop; nothing else to do here
        drop(self);
    }
}

impl<'a> Drop for ForeignKeyBuilder<'a> {
    fn drop(&mut self) {
        if self.ref_table.is_empty() {
            // No `.on(...)` was called — nothing to emit.
            return;
        }
        self.table.push(PendingFk {
            column: std::mem::take(&mut self.column),
            ref_table: std::mem::take(&mut self.ref_table),
            ref_col: std::mem::take(&mut self.ref_col),
            on_delete: self.on_delete.take(),
            on_update: self.on_update.take(),
        });
    }
}

pub struct ColumnDef {
    sea_def: SeaColumnDef,
    pub name: String,
    #[allow(dead_code)]
    mode: TableMode,
}

impl ColumnDef {
    pub fn not_null(&mut self) -> &mut Self {
        self.sea_def.not_null();
        self
    }

    pub fn nullable(&mut self) -> &mut Self {
        self.sea_def.null();
        self
    }

    pub fn unique(&mut self) -> &mut Self {
        self.sea_def.unique_key();
        self
    }

    pub fn primary_key(&mut self) -> &mut Self {
        self.sea_def.primary_key();
        self
    }

    /// Set the column default. String values that look like SQL string literals
    /// (no parens, not already quoted, not numeric, not a recognized keyword)
    /// are auto-quoted to avoid Postgres parsing them as column references.
    /// Use [`default_raw`](Self::default_raw) to bypass quoting entirely.
    pub fn default(&mut self, value: impl Into<String>) -> &mut Self {
        let v = value.into();
        let expr = if looks_like_sql_expr(&v) {
            v
        } else {
            format!("'{}'", v.replace('\'', "''"))
        };
        self.sea_def.default(sea_query::Expr::cust(expr));
        self
    }

    /// Set the default to raw SQL — no quoting, no parsing. Use this when you
    /// need to pass a specific expression (a cast like `'{}'::jsonb`, a function
    /// reference like `gen_random_uuid()`, etc.) and don't want the auto-quoting
    /// in [`default`](Self::default).
    pub fn default_raw(&mut self, sql: impl Into<String>) -> &mut Self {
        self.sea_def.default(sea_query::Expr::cust(sql.into()));
        self
    }

    pub fn default_value<T>(&mut self, value: T) -> &mut Self
    where
        T: Into<sea_query::Value>,
    {
        self.sea_def.default(value);
        self
    }

    /// Add an inline column comment. Postgres stores this in `COMMENT ON COLUMN`.
    pub fn comment(&mut self, _text: impl Into<String>) -> &mut Self {
        // sea-query's API for column comments varies per dialect — emit as a no-op
        // until v0.2 wires up a post-emit COMMENT ON COLUMN statement.
        self
    }

    /// Alias for `default` — Laravel uses `useCurrent()` for timestamps.
    pub fn use_current(&mut self) -> &mut Self {
        self.default("CURRENT_TIMESTAMP")
    }
}

/// Heuristic: should this default value be passed to SQL verbatim, or quoted
/// as a string literal? Returns `true` for things that already look like a SQL
/// expression (quoted strings, numbers, function calls, known keywords).
fn looks_like_sql_expr(value: &str) -> bool {
    let v = value.trim();
    if v.is_empty() {
        return true;
    }
    // Already-quoted string / identifier.
    if v.starts_with('\'') || v.starts_with('"') || v.starts_with('`') {
        return true;
    }
    // Numeric literal (int or float, signed).
    if v.parse::<f64>().is_ok() {
        return true;
    }
    // Function call or any sub-expression in parens.
    if v.contains('(') {
        return true;
    }
    // Recognized keywords / time functions used as bare defaults.
    matches!(
        v.to_ascii_uppercase().as_str(),
        "TRUE"
            | "FALSE"
            | "NULL"
            | "CURRENT_TIMESTAMP"
            | "CURRENT_DATE"
            | "CURRENT_TIME"
            | "NOW"
            | "LOCALTIMESTAMP"
            | "LOCALTIME"
    )
}

// ─── per-driver SQL emission ────────────────────────────────────────────────

fn build_per_driver(t: &sea_query::TableCreateStatement, driver: Driver) -> String {
    match driver {
        Driver::Postgres => t.build(PostgresQueryBuilder),
        Driver::MySql => t.build(MysqlQueryBuilder),
        Driver::Sqlite => t.build(SqliteQueryBuilder),
    }
}

fn build_alter_per_driver(t: &sea_query::TableAlterStatement, driver: Driver) -> String {
    match driver {
        Driver::Postgres => t.build(PostgresQueryBuilder),
        Driver::MySql => t.build(MysqlQueryBuilder),
        Driver::Sqlite => t.build(SqliteQueryBuilder),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_stmts(driver: Driver, f: impl FnOnce(&mut Table)) -> Vec<String> {
        let mut t = Table::new("posts", TableMode::Create, driver);
        f(&mut t);
        t.into_statements()
    }

    fn alter_stmts(driver: Driver, f: impl FnOnce(&mut Table)) -> Vec<String> {
        let mut t = Table::new("posts", TableMode::Alter, driver);
        f(&mut t);
        t.into_statements()
    }

    #[test]
    fn foreign_id_for_inlines_fk_in_create_on_sqlite() {
        // Regression: previously emitted `ALTER TABLE … ADD CONSTRAINT …` which
        // SQLite rejects with `near "CONSTRAINT": syntax error`.
        let stmts = create_stmts(Driver::Sqlite, |t| {
            t.id();
            t.foreign_id_for("user_id", "users");
        });
        let create = stmts
            .iter()
            .find(|s| s.starts_with("CREATE TABLE"))
            .unwrap();
        assert!(
            create.contains("FOREIGN KEY"),
            "FK should be inline in CREATE TABLE, got: {create}"
        );
        assert!(
            create.contains("REFERENCES users (id)"),
            "FK target should be inline, got: {create}"
        );
        assert!(
            create.contains("ON DELETE CASCADE"),
            "FK action should be inline, got: {create}"
        );
        assert!(
            !stmts.iter().any(|s| s.starts_with("ALTER TABLE")),
            "no ALTER TABLE should be emitted on SQLite, got: {stmts:?}"
        );
    }

    #[test]
    fn foreign_id_for_inlines_fk_in_create_on_postgres() {
        let stmts = create_stmts(Driver::Postgres, |t| {
            t.id();
            t.foreign_id_for("user_id", "users");
        });
        let create = stmts
            .iter()
            .find(|s| s.starts_with("CREATE TABLE"))
            .unwrap();
        assert!(create.contains("FOREIGN KEY"));
        assert!(create.contains("REFERENCES users (id)"));
        assert!(!stmts.iter().any(|s| s.starts_with("ALTER TABLE")));
    }

    #[test]
    fn explicit_foreign_builder_inlines_in_create() {
        let stmts = create_stmts(Driver::Sqlite, |t| {
            t.id();
            t.big_integer("user_id").not_null();
            t.foreign("user_id").references("id").on("users").cascade();
        });
        let create = stmts
            .iter()
            .find(|s| s.starts_with("CREATE TABLE"))
            .unwrap();
        assert!(create.contains("FOREIGN KEY (user_id)"));
        assert!(create.contains("ON DELETE CASCADE"));
    }

    #[test]
    fn unsigned_inlines_check_constraint() {
        let stmts = create_stmts(Driver::Sqlite, |t| {
            t.unsigned_big_integer("balance");
        });
        let create = stmts
            .iter()
            .find(|s| s.starts_with("CREATE TABLE"))
            .unwrap();
        assert!(
            create.contains("CHECK (balance >= 0)"),
            "CHECK should be inline, got: {create}"
        );
        assert!(!stmts.iter().any(|s| s.starts_with("ALTER TABLE")));
    }

    #[test]
    fn enum_col_inlines_check_constraint() {
        let stmts = create_stmts(Driver::Sqlite, |t| {
            t.enum_col("status", &["draft", "published"]);
        });
        let create = stmts
            .iter()
            .find(|s| s.starts_with("CREATE TABLE"))
            .unwrap();
        assert!(create.contains("CHECK (status IN ('draft', 'published'))"));
    }

    #[test]
    fn alter_mode_emits_alter_table_on_postgres() {
        let stmts = alter_stmts(Driver::Postgres, |t| {
            t.foreign("user_id").references("id").on("users").cascade();
        });
        assert!(stmts
            .iter()
            .any(|s| s.contains("ALTER TABLE posts ADD CONSTRAINT")
                && s.contains("FOREIGN KEY (user_id)")));
    }

    #[test]
    fn alter_mode_skips_fk_on_sqlite() {
        // SQLite truly doesn't support adding FK to an existing table — we warn
        // and skip rather than emitting invalid SQL.
        let stmts = alter_stmts(Driver::Sqlite, |t| {
            t.foreign("user_id").references("id").on("users").cascade();
        });
        assert!(
            !stmts.iter().any(|s| s.contains("ADD CONSTRAINT")),
            "no ADD CONSTRAINT on SQLite alter, got: {stmts:?}"
        );
    }

    #[test]
    fn default_quotes_string_literals() {
        let stmts = create_stmts(Driver::Postgres, |t| {
            t.string("status").not_null().default("pending");
        });
        let create = &stmts[0];
        assert!(
            create.contains("DEFAULT 'pending'"),
            "string default should be auto-quoted, got: {create}"
        );
    }

    #[test]
    fn default_preserves_already_quoted() {
        let stmts = create_stmts(Driver::Postgres, |t| {
            t.string("status").default("'pending'");
        });
        assert!(stmts[0].contains("DEFAULT 'pending'"));
        // Should not double-quote.
        assert!(!stmts[0].contains("'''"));
    }

    #[test]
    fn default_preserves_numeric_literal() {
        let stmts = create_stmts(Driver::Postgres, |t| {
            t.integer("attempts").default("0");
            t.integer("max").default("3");
            t.float("ratio").default("1.5");
        });
        assert!(stmts[0].contains("DEFAULT 0"));
        assert!(stmts[0].contains("DEFAULT 3"));
        assert!(stmts[0].contains("DEFAULT 1.5"));
    }

    #[test]
    fn default_preserves_boolean_keywords() {
        let stmts = create_stmts(Driver::Postgres, |t| {
            t.boolean("active").default("true");
            t.boolean("paid").default("false");
        });
        assert!(stmts[0].contains("DEFAULT TRUE") || stmts[0].contains("DEFAULT true"));
        assert!(stmts[0].contains("DEFAULT FALSE") || stmts[0].contains("DEFAULT false"));
    }

    #[test]
    fn default_preserves_current_timestamp() {
        let stmts = create_stmts(Driver::Postgres, |t| {
            t.timestamp("ts").default("CURRENT_TIMESTAMP");
        });
        assert!(stmts[0].contains("DEFAULT CURRENT_TIMESTAMP"));
    }

    #[test]
    fn default_preserves_function_call() {
        let stmts = create_stmts(Driver::Postgres, |t| {
            t.uuid("id").default("gen_random_uuid()");
        });
        assert!(stmts[0].contains("DEFAULT gen_random_uuid()"));
    }

    #[test]
    fn default_escapes_embedded_quotes() {
        let stmts = create_stmts(Driver::Postgres, |t| {
            t.string("note").default("O'Reilly");
        });
        assert!(
            stmts[0].contains("DEFAULT 'O''Reilly'"),
            "embedded quote should be escaped, got: {}",
            stmts[0]
        );
    }

    #[test]
    fn default_raw_bypasses_quoting() {
        let stmts = create_stmts(Driver::Postgres, |t| {
            t.jsonb("meta").default_raw("'{}'::jsonb");
        });
        assert!(stmts[0].contains("DEFAULT '{}'::jsonb"));
    }
}
