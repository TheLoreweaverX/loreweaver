pub mod character;
pub mod cli;
pub mod twitter;

use rig::Embed;
use serde::{Deserialize, Serialize};

pub struct MongoCredentials {
    pub conn_url: String,
    pub db: String,
    pub collection: String,
}
pub struct TwitterCredentials {
    pub api_key: String,
    pub api_secret: String,
    pub access_token: String,
    pub access_token_secret: String,
}

#[derive(Embed, Clone, Serialize, Deserialize, Debug)]
pub struct Message {
    pub id: String,
    #[embed]
    pub content: String,
}
