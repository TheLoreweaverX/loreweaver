use crate::core::Message;
use crate::db::mongo::Credentials;
use anyhow::{anyhow, Error, Result};
use rig::{embeddings::Embedding, OneOrMany};

use mongodb::{
    bson::{doc, Document},
    options::ClientOptions,
    Client as MongoClient, Collection,
};

pub struct Client {
    pub client: MongoClient,
    vec_db: Collection<Document>,
    stats_db: Collection<Document>,
}

impl Client {
    pub async fn new(creds: Credentials) -> Result<Self, Error> {
        let opts = ClientOptions::parse(creds.conn_url.clone()).await?;

        let client = MongoClient::with_options(opts.clone())?;

        let vec_db = client.database(&creds.db).collection(&creds.vec_collection);

        println!("{}", &creds.stats_collection);
        println!("{}", &creds.vec_collection);

        let stats_db = client
            .database(&creds.db)
            .collection(&creds.stats_collection);

        Ok(Self {
            client,
            vec_db,
            stats_db,
        })
    }

    pub async fn stats_create_version_doc(
        &self,
        version: u8,
        creation_date_unix: u32,
        character_data: String,
    ) -> Result<()> {
        let version_doc = doc! { "version": version as u32,
        "tweets_sent": 0,
        "replies_sent": 0,
        "messages_read": 0,
        "creation_date_unix": creation_date_unix,
        "character_data": character_data };

        self.stats_db.insert_one(version_doc).await?;

        Ok(())
    }

    pub async fn stats_version_doc_exists(&self, version: u8) -> Result<()> {
        let filter = doc! { "version": version as u32};

        let res = self.stats_db.find_one(filter).await?;

        if res.is_none() {
            return Err(anyhow!("No document found for version"));
        }

        Ok(())
    }

    pub async fn stats_inc_tweet_count(&self, version: u8) -> Result<u64, Error> {
        let filter = doc! { "version": version as u32 };
        let update = doc! {
            "$inc": { "tweets_sent": 1 }
        };

        let update_res = self.stats_db.update_one(filter, update).await?;

        if update_res.modified_count == 0 {
            return Err(anyhow!("No document found for version"));
        }

        Ok(update_res.modified_count)
    }

    pub async fn stats_inc_reply_count(&self, version: u8) -> Result<u64, Error> {
        let filter = doc! { "version": version as u32 };
        let update = doc! {
            "$inc": { "replies_sent": 1 }
        };

        let update_res = self.stats_db.update_one(filter, update).await?;

        if update_res.modified_count == 0 {
            return Err(anyhow!("No document found for version"));
        }

        Ok(update_res.modified_count)
    }

    pub async fn stats_add_msgs_read(&self, version: u8, num_msgs: u32) -> Result<u64, Error> {
        let filter = doc! { "version": version as u32 };
        let update = doc! {
            "$inc": { "messages_read": num_msgs }
        };

        let update_res = self.stats_db.update_one(filter, update).await?;

        if update_res.modified_count == 0 {
            return Err(anyhow!("No document found for version"));
        }

        Ok(update_res.modified_count)
    }

    // Store embedding to vector store (serves as Agent's memory)
    pub async fn vec_store_message(&self, embedding: Embedding, message: Message) -> Result<()> {
        let document = doc! {
            "id": message.id.clone(),
            "content": message.content.clone(),
            "embedding": embedding.vec,
        };
        self.vec_db.insert_one(document).await?;
        Ok(())
    }

    // Store many embeddings to vector store (serves as Agent's memory)
    pub async fn vec_store_message_many(
        &self,
        embeddings: Vec<(Message, OneOrMany<Embedding>)>,
    ) -> Result<()> {
        let documents = embeddings
            .iter()
            .map(|(Message { id, content, .. }, embedding)| {
                doc! {
                    "id": id.clone(),
                    "definition": content.clone(),
                    "embedding": embedding.first().vec,
                }
            })
            .collect::<Vec<_>>();
        self.vec_db.insert_many(documents).await?;
        Ok(())
    }
}
