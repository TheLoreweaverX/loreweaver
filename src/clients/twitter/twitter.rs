use anyhow::{Error, Result};
use log::{error, info};
use twitter_v2::{
    authorization::{BearerToken, Oauth1aToken},
    id::NumericId,
    Tweet, TwitterApi,
};

pub struct Client {
    auth: Oauth1aToken,
    user_id: NumericId,
    latest_mention_id: NumericId,
}

pub struct TwitterAuth {
    pub api_key: String,
    pub api_secret: String,
    pub access_token: String,
    pub access_token_secret: String,
}

impl Client {
    pub async fn new(credentials: TwitterAuth) -> Self {
        let auth = Oauth1aToken::new(
            credentials.api_key,
            credentials.api_secret,
            credentials.access_token,
            credentials.access_token_secret,
        );
        let user_id = TwitterApi::new(auth.clone())
            .get_users_me()
            .send()
            .await
            .unwrap()
            .into_data()
            .expect("[TWITTER_CLIENT] fatal error occured while fetching user_id")
            .id;

        // Fetch the latest mention ID
        // @todo: Make this the last replied to mention ID
        let latest_mention_id = TwitterApi::new(auth.clone())
            .get_user_mentions(user_id)
            .send()
            .await
            .ok()
            .and_then(|response| response.into_data())
            .and_then(|mentions| mentions.into_iter().map(|mention| mention.id).max())
            .unwrap_or_else(|| NumericId::new(0));

        Self {
            auth,
            user_id,
            latest_mention_id,
        }
    }

    pub async fn publish(&self, response: &str) -> Result<()> {
        let tweet = TwitterApi::new(self.auth.clone())
            .post_tweet()
            .text(response.to_string())
            .send()
            .await?
            .into_data()
            .ok_or_else(|| Error::msg("[TWITTER_CLIENT] failed to get tweet data"))?;

        info!("[TWITTER_CLIENT] Agent posted tweet (ID: {})", tweet.id);

        Ok(())
    }

    pub async fn reply(&mut self, id: NumericId, response: &str) -> Result<()> {
        let tweet = TwitterApi::new(self.auth.clone())
            .post_tweet()
            .in_reply_to_tweet_id(id)
            .text(response.to_string())
            .send()
            .await?
            .into_data()
            .ok_or_else(|| Error::msg("[TWITTER_CLIENT] failed to get tweet data"))?;

        info!("[TWITTER_CLIENT] Agent posted tweet (ID: {})", tweet.id);

        Ok(())
    }

    //@todo: find most efficient way to reply to all mentions without replying to the same one multiple times.
    pub async fn fetch_mentions(&mut self, count: usize) -> Result<Vec<Tweet>> {
        let mentions = TwitterApi::new(self.auth.clone())
            .get_user_mentions(self.user_id)
            .since_id(self.latest_mention_id)
            .max_results(count)
            .send()
            .await?
            .into_data()
            .ok_or_else(|| Error::msg("[TWITTER_CLIENT] failed to get mentions from tweet"))?;

        if let Some(max_id) = mentions.iter().map(|mention| mention.id).max() {
            self.latest_mention_id = max_id;
            info!(
                "[TWITTER_CLIENT] Updated latest_mention_id to {}",
                self.latest_mention_id
            );
        }
        info!("[TWITTER_CLIENT] Agent fetched all mentions");

        Ok(mentions)
    }

    pub async fn fetch_timeline(&mut self, count: usize) -> Result<Vec<Tweet>> {
        let timeline = TwitterApi::new(self.auth.clone())
            .get_user_tweets(self.user_id)
            .since_id(self.latest_mention_id)
            .max_results(count)
            .send()
            .await?
            .into_data()
            .ok_or_else(|| Error::msg("[TWITTER_CLIENT] failed to fetch timeline"))?;
        info!("[TWITTER_CLIENT] Agent fetched timeline");

        Ok(timeline)
    }

    //@note: for later concurrent purposes.
    pub fn kill(&self) -> Result<()> {
        Ok(())
    }
}
