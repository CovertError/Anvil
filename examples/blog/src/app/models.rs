//! Models for the blog example.

use anvilforge::async_trait::async_trait;
use anvilforge::prelude::*;
use anvilforge::seeder::{Factory, HasFactory, PersistentFactory};

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

// ── Factories ────────────────────────────────────────────────────────────────
//
// Laravel pattern: `Author::factory()->count(50)->create()`. Anvilforge needs an
// explicit `HasFactory` binding because Rust doesn't have PHP's class-loading
// magic for `Database\Factories\UserFactory`.

pub struct AuthorFactory;

impl Factory<Author> for AuthorFactory {
    fn definition() -> Author {
        use fake::{
            faker::{internet::en::SafeEmail, name::en::Name},
            Fake,
        };
        Author {
            id: 0,
            name: Name().fake(),
            email: SafeEmail().fake(),
            created_at: None,
            updated_at: None,
        }
    }
}

#[async_trait]
impl PersistentFactory<Author> for AuthorFactory {
    async fn save(c: &Container, model: Author) -> Result<Author> {
        Ok(model.save(c.pool()).await?)
    }
}

impl HasFactory for Author {
    type Factory = AuthorFactory;
}
