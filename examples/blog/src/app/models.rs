//! Models for the blog example.

use anvil::prelude::*;

#[derive(Debug, Clone, Serialize, Deserialize, Model)]
#[table("authors")]
#[has_many(crate::app::models::Post, foreign_key = "author_id")]
pub struct Author {
    pub id: i64,
    pub name: String,
    pub email: String,
    pub created_at: Option<DateTime<Utc>>,
    pub updated_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Model)]
#[table("posts")]
#[belongs_to(crate::app::models::Author, foreign_key = "author_id")]
pub struct Post {
    pub id: i64,
    pub author_id: i64,
    pub title: String,
    pub body: String,
    pub published: bool,
    pub created_at: Option<DateTime<Utc>>,
    pub updated_at: Option<DateTime<Utc>>,
}
