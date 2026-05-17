use anvilforge::prelude::*;
use garde::Validate;

#[derive(Debug, Deserialize, Validate, FormRequest)]
pub struct StorePostRequest {
    #[garde(length(min = 1, max = 200))]
    pub title: String,

    #[garde(length(min = 1))]
    pub body: String,

    #[garde(skip)]
    pub author_id: i64,

    #[garde(skip)]
    pub published: Option<bool>,
}
