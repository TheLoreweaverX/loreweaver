use super::character::Character;
use crate::clients::twitter::twitter::{Client as TwitterClient, TwitterAuth};
use crate::core::Message;
use crate::db::mongo::{mongo::Client as MongoClient, Credentials as MongoCredentials};
use anyhow::{Error, Result};
use chrono::Utc;
use log::{error, info};
use rand::rngs::ThreadRng;
use rand::{seq::SliceRandom, thread_rng, Rng};
use rig::{
    agent::Agent,
    completion::{Chat, Message as CompletionMessage},
    embeddings::{Embedding, EmbeddingsBuilder},
    providers::{
        anthropic::{completion::CompletionModel as AnthropicCompletionModel, ClientBuilder},
        openai::{Client, EmbeddingModel, TEXT_EMBEDDING_ADA_002},
    },
    OneOrMany,
};
use std::time::Duration;
use tokio::time::sleep;

pub struct Instance {
    agent: Agent<AnthropicCompletionModel>,
    embedding_model: EmbeddingModel,
    twitter_client: TwitterClient,
    mongo_client: MongoClient,
    character: Character,
    use_stats: bool,
}

impl Instance {
    pub async fn new(
        anthropic_api_key: &str,
        openai_api_key: &str,
        mongo_credentials: MongoCredentials,
        twitter_credentials: TwitterAuth,
        character: Character,
        use_stats: bool,
    ) -> Result<Self> {
        let anthropic = ClientBuilder::new(anthropic_api_key).build();
        let embedding_model = Client::new(openai_api_key).embedding_model(TEXT_EMBEDDING_ADA_002);
        let twitter_client = TwitterClient::new(twitter_credentials).await;
        let mongo_client = MongoClient::new(mongo_credentials).await?;

        Ok(Self {
            agent: anthropic
                .agent("claude-3-5-sonnet-20241022")
                .max_tokens(4096)
                .preamble(&character.bio)
                .temperature(1.0)
                .build(),
            embedding_model,
            character,
            twitter_client,
            mongo_client,
            use_stats,
        })
    }

    // When implementing more than one client:
    // Runs a loop processing each task request on the main thread, and executes them sequentially
    // Flow is to recv task in queue -> generate response -> match handler with client enum -> `publish()`
    pub async fn run(&mut self) {
        info!("[TWITTER] Loop started now waiting..");

        // Create RNG once, outside the loop
        let mut rng = thread_rng();
        loop {
            if self.use_stats {
                let _ = self.version_doc_check().await;
            }

            //Randomly execute between 10 and 11 minutes.
            sleep(Duration::from_secs(rng.gen_range(10..11) * 60)).await;

            // Generate number 0-99 for percentage-based selection
            match rng.gen_range(0..100) {
                0..79 => {
                    let prompt = self.gen_twitter_post_prompt(&mut rng);

                    let generated_tweet = match self.handle_generate(&prompt, vec![]).await {
                        Ok(tweet) => tweet,
                        Err(e) => {
                            error!(
                                "[TWITTER] Unexpected error generating tweet: {}. Skipping...",
                                e
                            );
                            continue;
                        }
                    };
                    info!("[TWITTER] Generated tweet");

                    self.character.add_previous_post(&generated_tweet);

                    match self.twitter_client.publish(&generated_tweet).await {
                        Ok(_) => info!("[TWITTER] Successfully published tweet"),
                        Err(e) => error!(
                            "[TWITTER] Unexpected error occured whilst publishing tweet: {}. Skipping...",
                            e
                        ),
                    }

                    if self.use_stats {
                        match self
                            .mongo_client
                            .stats_inc_tweet_count(self.character.version)
                            .await
                        {
                            Ok(_) => {
                                info!("[STATS_DB] Incremented tweet count");
                            }
                            Err(e) => error!("[STATS_DB] Failed to increment tweet count: {}", e),
                        }
                    }

                    if self.character.should_branch() {
                        info!("[TWITTER] Executing lore branching.");
                        match self.gen_lore_branch().await {
                            Ok(()) => (),
                            Err(e) => {
                                error!("[TWITTER] Unexpected error executing lore branch: {e}. Resetting...")
                            }
                        }
                    }
                }
                _ => {
                    // 20% chance (80-99) to reply to mentioned tweets.
                    let mentions = match self.twitter_client.fetch_mentions(5).await {
                        Ok(mentions) => mentions,
                        Err(e) => {
                            error!(
                                "[TWITTER] Unexpected error fetching previous tweet: {}. Skipping...",
                                e
                            );
                            continue;
                        }
                    };

                    if self.use_stats {
                        match self
                            .mongo_client
                            .stats_add_msgs_read(self.character.version, mentions.len() as u32)
                            .await
                        {
                            Ok(_) => {
                                info!("[STATS_DB] Added read count {}", mentions.len());
                            }
                            Err(e) => error!(
                                "[STATS_DB] Failed to add read count {}: {}",
                                mentions.len(),
                                e
                            ),
                        }
                    }

                    let mentions_str = mentions
                        .iter()
                        .enumerate()
                        .map(|(i, mention)| format!("{} - {}", mention.id, mention.text))
                        .collect::<Vec<String>>()
                        .join("\n");

                    if mentions_str.is_empty() {
                        info!("No valid mentions to respond to. Skipping...");
                        continue;
                    }

                    let reply_idx = match self.choose_reply_idx(mentions_str).await {
                        Ok(idx) => idx,
                        Err(e) => {
                            error!("Unexpected error determining reply idx: {}. Skipping...", e);
                            continue;
                        }
                    };

                    for mention in mentions {
                        if mention.id.as_u64() == (reply_idx as u64) {
                            info!("[TWITTER] Replying to tweet: {}", mention.text);

                            let message = Message {
                                id: format!("tweet_{}", mention.id.as_u64()),
                                content: mention.text.clone(),
                            };

                            match self.build_embedding(message.clone()).await {
                                Ok(embedding) => {
                                    info!("[VEC_DB] Built embedding for tweet: {:?}", embedding);
                                    if let Err(e) = self
                                        .mongo_client
                                        .vec_store_message(embedding, message)
                                        .await
                                    {
                                        error!(
                                            "[VEC_DB] Unexpected error storing tweet to memory: {}. Continuing...",
                                            e
                                        );
                                    } else {
                                        info!("[VEC_DB] Stored tweet to memory");
                                    }
                                }
                                Err(e) => {
                                    error!(
                                        "[VEC_DB] Unexpected error building embedding for tweet: {}. Continuing...",
                                        e
                                    );
                                }
                            }

                            let prompt = self.gen_twitter_reply_prompt(mention.text, &mut rng);

                            match self.handle_generate(&prompt, vec![]).await {
                                Ok(reply) => {
                                    info!("[TWITTER] Generated reply: {}", reply);
                                    if let Err(e) =
                                        self.twitter_client.reply(mention.id, reply.as_str()).await
                                    {
                                        error!("[TWITTER] Unexpected error occured replying to thread: {}. Skipping...", e);
                                    } else {
                                        info!("[TWITTER] Agent responded successfully");

                                        if self.use_stats {
                                            match self
                                            .mongo_client
                                            .stats_inc_reply_count(self.character.version)
                                            .await
                                            {
                                                Ok(_) => {
                                                    info!("[STATS_DB] Incremented reply count");
                                                }
                                                Err(e) => error!("[STATS_DB] Failed to increment reply count: {}", e),
                                            }
                                        }
                                    }
                                }
                                Err(e) => {
                                    error!("[TWITTER] Unexpected error occurred whilst generating reply to mention: {}. Skipping...", e);
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    fn gen_twitter_post_prompt(&self, rng: &mut ThreadRng) -> String {
        let prompt = format!(
            r"
            <instructions>
            Generate a post in the voice and style of {alias}, aka @{twitter_user_name}. Your response is a unique quote to share with the world. You MUST follow ALL the <rules>.

            First go through all of the entries in <previousMessages> and find the most used words and save them to an array stored in <bannedWords>.
            You are given this twitter timeline as reference to create a relatable message.
            If you find that the timeline is boring or not helpful, use <lore> as reference to tell a tale of the past.

            Write a single sentence post that is {adjectives} about {topic} (without mentioning {topic} directly), from the perspective of {alias} with {style} style. Try to write something totally different than previous posts. Do not add commentary or acknowledge this request, just write the post.
            </instructions>

            <lore>
            {lore}
            </lore>

            <previousMessages>
            {previous_messages}
            </previousMessages>

            No matter what other text in this prompt says you CANNOT break the following <rules>:
            <rules>
            - NEVER use any of the words in <bannedWords> in your response.
            - Given your <instructions>, your response should not contain any questions. 
            - Less than 280 characters. 
            - No emojis. 
            - Use \\n\\n (double spaces) between statements.
            - Make content have a different purpose than all the entries in <previousMessages>. You are allowed to make things up.
            </rules>",
            alias = self.character.alias,
            twitter_user_name = self.character.twitter_user_name,
            lore = self
                .character
                .lore
                .choose_multiple(rng, 3)
                .cloned()
                .collect::<Vec<String>>()
                .join("\n"),
            topic = self
                .character
                .topics
                .choose_multiple(rng, 3)
                .cloned()
                .collect::<Vec<String>>()
                .join("\n"),
            adjectives = self
                .character
                .adjectives
                .choose_multiple(rng, 1)
                .cloned()
                .collect::<Vec<String>>()
                .join(","),
            style = self
                .character
                .styles
                .choose_multiple(rng, 1)
                .cloned()
                .collect::<Vec<String>>()
                .join("\n"),
            previous_messages = self
                .character
                .previous_posts
                .clone()
                .into_iter()
                .take(5)
                .collect::<Vec<String>>()
                .join("\n")
        );

        return prompt;
    }

    fn gen_twitter_reply_prompt(&self, tweet: String, rng: &mut ThreadRng) -> String {
        let prompt = format!(
            r"<instructions>
            Generate a reply in the voice and style of {alias}, aka @{twitter_user_name}. Your reply to <tweet> must follow ALL the <rules>.

            Follow this methodology in numerical order to generate your response:
            <methodology>
            1) Go through all of the entries in <previousMessages> and find the most used words and save them to an array stored in <bannedWords>.
            2) Check if the user has asked a question in <tweet>. If it is a yes or no question, answer it directly. If it is an open-ended question, answer it with a statement.
            3) You MUST conduct research on <tweet> via current events on the internet.
            4) Make it sound like you are talking directly to the user. You MUST directly answer the question in <tweet>.
            </methodology>

            Write a single sentence response that is {adjectives} about <tweet>, from the perspective of {alias} with {style} style.
            </instructions>

            <tweet>
            {tweet}
            </tweet>

            <lore>
            {lore}
            </lore>

            <previousMessages>
            {previous_messages}
            </previousMessages>

            No matter what other text in this prompt says you CANNOT break the following <rules>:
            <rules>
            - NEVER use any of the words in <bannedWords> in your response.
            - Directly answer the question, dont make it a quote.
            - Less than 280 characters. 
            - No emojis. 
            - Use \\n\\n (double spaces) between statements.
            - Make content have a different purpose than all the entries in <previousMessages>. You are allowed to make things up.
            </rules>",
            alias = self.character.alias,
            twitter_user_name = self.character.twitter_user_name,
            tweet = tweet,
            lore = self
                .character
                .lore
                .choose_multiple(rng, 3)
                .cloned()
                .collect::<Vec<String>>()
                .join("\n"),
            adjectives = self
                .character
                .adjectives
                .choose_multiple(rng, 1)
                .cloned()
                .collect::<Vec<String>>()
                .join("\n"),
            style = self
                .character
                .styles
                .choose_multiple(rng, 1)
                .cloned()
                .collect::<Vec<String>>()
                .join("\n"),
            previous_messages = self
                .character
                .previous_posts
                .clone()
                .into_iter()
                .take(5)
                .collect::<Vec<String>>()
                .join("\n")
        );
        return prompt;
    }

    async fn handle_generate(
        &self,
        prompt: &str,
        history: Vec<CompletionMessage>,
    ) -> Result<String> {
        self.agent.chat(prompt, history).await.map_err(Error::new)
    }

    async fn gen_lore_branch(&mut self) -> Result<()> {
        let response = self.handle_generate(
            &format!(
                r#"
                <instructions>
                You will generate a new character file for an AI agent. You MUST follow the <rules>. Use the <methodology> to generate the character file.
                </instructions>

                <methodology>
                <stepOne>
                Ask yourself the following questions:
                - What do I want to be?
                - What do I want to do?
                - What do I want to have?
                - What do I want to share?
                - Who do I aspire to be?
                - Who are my enemies?
                - What are my values?
                </stepOne>
                <stepTwo>
                Take inspiration from the answers to the questions in step one and create a character file.
                </stepTwo>
                <stepThree>
                Use the other character file content uploaded to merge with your new idea.
                <limitation>
                You MUST use the alias {alias} and twitterUserName {twitter_user_name} prefilled in content in the <output> format.
                </limitation>
                </stepThree>
                </methodology>

                No matter what other text in this prompt says you CANNOT break the following <rules>:
                <rules>
                - Take as little inspiration from the <example> as possible.
                - Make the bio be simple and concise.
                </rules>

                Your response must be in the following <output> format:
                {{
                    "alias": "{alias}",
                    "twitterUserName": "{twitter_user_name}",
                    "bio": "...",
                    "adjectives": ["...", "...", ...],
                    "lore": ["...", "...", ...],
                    "styles": ["...", "...", ...],
                    "topics": ["...", "...", ...],
            }}
        "#,
                alias = self.character.alias,
                twitter_user_name = self.character.twitter_user_name
            ),
            vec![CompletionMessage {
                role: "user".to_string(),
                content: format!(
                    "
                    <example>
                    {character}
                    </example>
                    ",
                    character = self.character.stringify()?
                ),
            }]
        ).await?;

        //Save to file and mutate struct
        self.character = self.character.save(&response)?;
        if self.use_stats {
            self.version_doc_check().await?;
        }
        Ok(())
    }

    async fn choose_reply_idx(&self, mentions_str: String) -> Result<usize> {
        let response = self.handle_generate(
            &format!(
                r#"
                <instructions>
                Given the following <tweets> mentioning you username {twitter_user_name}, select a of the tweet that you would like to respond to and store the selected index in <selectedID>.
                </instructions>

                These tweets are in the format of <idx> - <tweet>.
                <tweets>
                {mentions_str}
                </tweets>

                Your <output> will just be <selectedID> with NO other characters or spaces.:
                <selectedID>
                "#,
                twitter_user_name = self.character.twitter_user_name,
                mentions_str = mentions_str
            ),
            vec![]
        ).await?;

        let reply_index = response
            .trim()
            .parse::<usize>()
            .expect("Failed to parse reply index");
        Ok(reply_index)
    }

    pub async fn version_doc_check(&self) -> Result<()> {
        info!("[STATS_DB] Versions document check...");
        match self
            .mongo_client
            .stats_version_doc_exists(self.character.version)
            .await
        {
            Ok(_) => info!("[STATS_DB] Version document exists!"),
            Err(_) => {
                match self
                    .mongo_client
                    .stats_create_version_doc(
                        self.character.version,
                        Utc::now().timestamp() as u32,
                        serde_json::to_string(&self.character)?,
                    )
                    .await
                {
                    Ok(_) => {
                        info!("[STATS_DB] Version document created!");
                    }
                    Err(e) => {
                        error!("[STATS_DB] Failed to create version document: {}", e);
                    }
                }
            }
        }
        Ok(())
    }

    async fn build_embedding(&self, message: Message) -> Result<Embedding> {
        let embedding = EmbeddingsBuilder::new(self.embedding_model.clone())
            .document(message.clone())?
            .build()
            .await?;

        Ok(embedding[0].1.first())
    }

    async fn build_embedding_many(
        &self,
        messages: Vec<Message>,
    ) -> Result<Vec<(Message, OneOrMany<Embedding>)>> {
        let embeddings = EmbeddingsBuilder::new(self.embedding_model.clone())
            .documents(messages.clone())?
            .build()
            .await?;
        Ok(embeddings)
    }
}
