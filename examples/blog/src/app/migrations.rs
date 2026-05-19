//! Migrations. Each struct derives `Migration`, which registers it with the
//! framework's inventory — `MigrationRunner::new(pool)` auto-discovers them.

use anvilforge::prelude::*;
use cast::Schema;

#[derive(Migration)]
pub struct CreateAuthorsTable;
impl CastMigration for CreateAuthorsTable {
    fn name(&self) -> &'static str { "2026_01_01_000001_create_authors_table" }
    fn up(&self, s: &mut Schema) {
        s.create("authors", |t| {
            t.id();
            t.string("name").not_null();
            t.string("email").not_null().unique();
            t.timestamps();
        });
    }
    fn down(&self, s: &mut Schema) {
        s.drop_if_exists("authors");
    }
}

#[derive(Migration)]
pub struct CreatePostsTable;
impl CastMigration for CreatePostsTable {
    fn name(&self) -> &'static str { "2026_01_01_000002_create_posts_table" }
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

#[derive(Migration)]
pub struct CreateJobsTable;
impl CastMigration for CreateJobsTable {
    fn name(&self) -> &'static str { "2026_01_01_000003_create_jobs_table" }
    fn up(&self, s: &mut Schema) {
        s.raw("CREATE TABLE IF NOT EXISTS jobs (id UUID PRIMARY KEY, job_type TEXT NOT NULL, payload JSONB NOT NULL, attempts INTEGER NOT NULL DEFAULT 0, max_attempts INTEGER NOT NULL DEFAULT 3, queue TEXT NOT NULL, available_at TIMESTAMPTZ NOT NULL DEFAULT NOW())");
        s.raw("CREATE TABLE IF NOT EXISTS failed_jobs (id UUID PRIMARY KEY, job_type TEXT NOT NULL, payload JSONB NOT NULL, error TEXT NOT NULL, failed_at TIMESTAMPTZ NOT NULL DEFAULT NOW())");
    }
    fn down(&self, s: &mut Schema) {
        s.raw("DROP TABLE IF EXISTS jobs");
        s.raw("DROP TABLE IF EXISTS failed_jobs");
    }
}

/// Back-compat: kept for tests + the example blog's old call sites. New apps
/// should use `MigrationRunner::new(pool)` which auto-discovers via inventory.
pub fn all() -> Vec<Box<dyn CastMigration>> {
    vec![
        Box::new(CreateAuthorsTable),
        Box::new(CreatePostsTable),
        Box::new(CreateJobsTable),
    ]
}
