use super::character::Character;
use anyhow::{Error, Result};
use rand::rngs::ThreadRng;
use rand::{seq::SliceRandom, thread_rng};
use rig::{
    agent::Agent,
    completion::{Chat, Message as CompletionMessage},
    providers::anthropic::{
        completion::CompletionModel as AnthropicCompletionModel, ClientBuilder,
    },
};
use std::io::{self, Write};

pub struct Instance {
    agent: Agent<AnthropicCompletionModel>,
    character: Character,
}

impl Instance {
    pub async fn new(anthropic_api_key: &str, character: Character) -> Result<Self> {
        let anthropic = ClientBuilder::new(anthropic_api_key).build();

        Ok(Self {
            agent: anthropic
                .agent("claude-3-5-sonnet-20241022")
                .max_tokens(4096)
                .preamble(&character.bio)
                .temperature(1.0)
                .build(),
            character,
        })
    }

    // When implementing more than one client:
    // Runs a loop processing each task request on the main thread, and executes them sequentially
    // Flow is to recv task in queue -> generate response -> match handler with client enum -> `publish()`
    pub async fn run(&mut self) -> Result<()> {
        let mut rng = thread_rng();

        loop {
            print!("(1) TWITTER post | (2) Gen new LORE branch | Or type a custom prompt for an example TWITTER reply: ");
            io::stdout().flush()?;

            let mut input = String::new();
            io::stdin().read_line(&mut input)?;
            let input = input.trim();

            match input {
                "1" => {
                    println!("[CLI] Generating a new Twitter post...");
                    let prompt = self.gen_twitter_post_prompt(&mut rng);
                    let generated_tweet = self.handle_generate(&prompt, vec![]).await?;
                    self.character.add_previous_post(&generated_tweet);

                    println!("[CLI] Generated post:\n{}", generated_tweet);
                    println!();
                }
                "2" => {
                    println!("[CLI] Generating a new lore branch...");
                    match self.gen_lore_branch().await {
                        Ok(_) => println!(
                            "[CLI] Generated new lore branch under: {}.v{}.json",
                            self.character.character_name, self.character.version
                        ),
                        Err(e) => eprintln!("[CLI] Failed to generate new lore branch: {}", e),
                    };
                }
                custom => {
                    println!("[CLI] Generating a new Twitter reply...");
                    let prompt = self.gen_twitter_reply_prompt(custom.to_string(), &mut rng);
                    let generated_tweet = self.handle_generate(&prompt, vec![]).await?;
                    self.character.add_previous_post(&generated_tweet);

                    println!("[CLI] Generated reply:\n{}", generated_tweet);
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
        Ok(())
    }

    async fn handle_generate(
        &self,
        prompt: &str,
        history: Vec<CompletionMessage>,
    ) -> Result<String> {
        self.agent.chat(prompt, history).await.map_err(Error::new)
    }
}
