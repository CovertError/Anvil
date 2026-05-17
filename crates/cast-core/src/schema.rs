//! Schema builder. Used in migrations: `Schema::create("users", |t| ...)`.
//!
//! Mirrors Laravel's `Schema::create` ergonomics: `t.string("name").not_null().unique()`.

use sea_query::{ColumnDef as SeaColumnDef, ColumnType, PostgresQueryBuilder, Table as SeaTable};

#[derive(Default)]
pub struct Schema {
    pub statements: Vec<String>,
}

impl Schema {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn create<F>(&mut self, table: &str, build: F)
    where
        F: FnOnce(&mut Table),
    {
        let mut t = Table::new(table);
        build(&mut t);
        let (create_sql, post_sqls) = t.into_sql();
        self.statements.push(create_sql);
        self.statements.extend(post_sqls);
    }

    pub fn drop(&mut self, table: &str) {
        self.statements
            .push(format!("DROP TABLE IF EXISTS {} CASCADE", table));
    }

    pub fn drop_if_exists(&mut self, table: &str) {
        self.drop(table);
    }

    pub fn raw(&mut self, sql: impl Into<String>) {
        self.statements.push(sql.into());
    }
}

/// A table definition assembled inside `Schema::create`'s closure.
pub struct Table {
    name: String,
    columns: Vec<Box<ColumnDef>>,
    indexes: Vec<String>,
    foreign_keys: Vec<String>,
}

impl Table {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            columns: Vec::new(),
            indexes: Vec::new(),
            foreign_keys: Vec::new(),
        }
    }

    fn push_column(&mut self, name: &str, ty: ColumnType) -> &mut ColumnDef {
        let sea_def = SeaColumnDef::new_with_type(sea_query::Alias::new(name), ty);
        // Box gives the SeaColumnDef a stable address even if `columns` reallocates,
        // so the `&mut ColumnDef` returned to the caller stays valid across other
        // method calls in the building closure.
        let boxed = Box::new(ColumnDef {
            sea_def,
            name: name.to_string(),
        });
        self.columns.push(boxed);
        self.columns.last_mut().unwrap().as_mut()
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

    pub fn string(&mut self, name: &str) -> &mut ColumnDef {
        self.push_column(name, ColumnType::String(sea_query::StringLen::N(255)))
    }

    pub fn text(&mut self, name: &str) -> &mut ColumnDef {
        self.push_column(name, ColumnType::Text)
    }

    pub fn integer(&mut self, name: &str) -> &mut ColumnDef {
        self.push_column(name, ColumnType::Integer)
    }

    pub fn big_integer(&mut self, name: &str) -> &mut ColumnDef {
        self.push_column(name, ColumnType::BigInteger)
    }

    pub fn boolean(&mut self, name: &str) -> &mut ColumnDef {
        self.push_column(name, ColumnType::Boolean)
    }

    pub fn timestamp(&mut self, name: &str) -> &mut ColumnDef {
        self.push_column(name, ColumnType::Timestamp)
    }

    pub fn timestamp_tz(&mut self, name: &str) -> &mut ColumnDef {
        self.push_column(name, ColumnType::TimestampWithTimeZone)
    }

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

    pub fn json(&mut self, name: &str) -> &mut ColumnDef {
        self.push_column(name, ColumnType::Json)
    }

    pub fn uuid(&mut self, name: &str) -> &mut ColumnDef {
        self.push_column(name, ColumnType::Uuid)
    }

    /// Add a `bigint` foreign key referencing `references.id`.
    pub fn foreign_id_for(&mut self, name: &str, references: &str) -> &mut ColumnDef {
        let fk_sql = format!(
            "ALTER TABLE {} ADD CONSTRAINT fk_{}_{} FOREIGN KEY ({}) REFERENCES {} (id) ON DELETE CASCADE",
            self.name, self.name, name, name, references
        );
        self.foreign_keys.push(fk_sql);
        self.push_column(name, ColumnType::BigInteger)
    }

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

    fn into_sql(self) -> (String, Vec<String>) {
        let mut t = SeaTable::create();
        t.table(sea_query::Alias::new(&self.name)).if_not_exists();
        for col in &self.columns {
            t.col(&mut col.sea_def.clone());
        }
        let create_sql = t.build(PostgresQueryBuilder);
        let mut post = self.indexes;
        post.extend(self.foreign_keys);
        (create_sql, post)
    }
}

pub struct ColumnDef {
    sea_def: SeaColumnDef,
    pub name: String,
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
}
