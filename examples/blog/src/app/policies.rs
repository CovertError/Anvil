use anvil_core::auth::Policy;

use crate::app::models::{Author, Post};

pub struct PostPolicy;

impl Policy<Author, Post> for PostPolicy {
    fn check(user: &Author, ability: &str, post: &Post) -> bool {
        match ability {
            "view" => true,
            "update" | "delete" => user.id == post.author_id,
            _ => false,
        }
    }
}
