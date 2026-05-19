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
    foreign_keys: Vec<String>,
    drops: Vec<String>,
    renames: Vec<(String, String)>,
    alters: Vec<String>,
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
            alters: Vec::new(),
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
        let check = format!(
            "ALTER TABLE {} ADD CONSTRAINT {}_{}_unsigned CHECK ({} >= 0)",
            self.name, self.name, name, name
        );
        self.alters.push(check);
        self.push_column(name, ColumnType::BigInteger)
    }

    pub fn unsigned_integer(&mut self, name: &str) -> &mut ColumnDef {
        let check = format!(
            "ALTER TABLE {} ADD CONSTRAINT {}_{}_unsigned CHECK ({} >= 0)",
            self.name, self.name, name, name
        );
        self.alters.push(check);
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
        let check = format!(
            "ALTER TABLE {} ADD CONSTRAINT {}_{}_enum CHECK ({} IN ({}))",
            self.name, self.name, name, name, list
        );
        self.alters.push(check);
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
        let fk_sql = format!(
            "ALTER TABLE {} ADD CONSTRAINT fk_{}_{} FOREIGN KEY ({}) REFERENCES {} (id) ON DELETE CASCADE",
            self.name, self.name, name, name, references
        );
        self.foreign_keys.push(fk_sql);
        self.push_column(name, ColumnType::BigInteger)
    }

    /// Begin a fluent foreign-key constraint builder for `column`.
    /// Mirrors `$table->foreign('user_id')->references('id')->on('users')`.
    pub fn foreign(&mut self, column: &str) -> ForeignKeyBuilder<'_> {
        ForeignKeyBuilder {
            table: &mut self.foreign_keys,
            table_name: self.name.clone(),
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
                out.push(build_per_driver(&t, self.driver));
            }
            TableMode::Alter => {
                // `ALTER TABLE ... ADD COLUMN ...` per column added.
                for col in &self.columns {
                    let mut t = SeaTable::alter();
                    t.table(sea_query::Alias::new(&self.name));
                    t.add_column(col.sea_def.clone());
                    out.push(build_alter_per_driver(&t, self.driver));
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
        out.extend(self.foreign_keys);
        out.extend(self.alters);
        out
    }
}

/// Fluent builder returned by `Table::foreign(col)`. Drop it (or call `.constrain()`)
/// to commit the foreign key SQL.
pub struct ForeignKeyBuilder<'a> {
    table: &'a mut Vec<String>,
    table_name: String,
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
        let constraint = format!("fk_{}_{}", self.table_name, self.column);
        let mut sql = format!(
            "ALTER TABLE {} ADD CONSTRAINT {} FOREIGN KEY ({}) REFERENCES {} ({})",
            self.table_name, constraint, self.column, self.ref_table, self.ref_col
        );
        if let Some(action) = &self.on_delete {
            sql.push_str(&format!(" ON DELETE {action}"));
        }
        if let Some(action) = &self.on_update {
            sql.push_str(&format!(" ON UPDATE {action}"));
        }
        self.table.push(sql);
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

    pub fn default(&mut self, value: impl Into<String>) -> &mut Self {
        self.sea_def.default(sea_query::Expr::cust(value.into()));
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
