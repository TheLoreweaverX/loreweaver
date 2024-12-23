use anyhow::{Error, Result};
use lazy_static::lazy_static;
use serde::{Deserialize, Serialize};
use std::{
    collections::VecDeque,
    env,
    fs::{self, File, OpenOptions},
    io,
    path::Path,
};

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Character {
    pub alias: String,
    pub twitter_user_name: String,
    pub bio: String,
    pub adjectives: Vec<String>,
    pub lore: Vec<String>,
    pub styles: Vec<String>,
    pub topics: Vec<String>,

    // Character metadata
    #[serde(skip)]
    pub character_name: String,
    #[serde(skip)]
    pub version: u8,
    #[serde(skip)]
    pub posts_since_branch: u8,
    #[serde(skip, default)]
    pub previous_posts: VecDeque<String>,
}

lazy_static! {
    pub static ref POSTS_BEFORE_BRANCH: u8 = {
        env::var("POSTS_BEFORE_BRANCH")
            .and_then(|val| {
                val.parse::<u8>()
                    .map_err(|_| std::env::VarError::NotPresent)
            })
            .unwrap_or(5)
    };
}
impl Character {
    pub fn load(character_name: &str) -> Result<Self> {
        // Generate the file path
        let path = Path::new("characters").join(format!("{}.json", character_name));

        // Read the file contents
        let contents = fs::read_to_string(&path)?;

        // Deserialize into a Character instance
        let mut character = serde_json::from_str::<Character>(&contents)?;

        // Extract the version from the filename (e.g., "loreweaver.v3.json")
        if let Some(version_str) = character_name.split('.').find(|part| part.starts_with('v')) {
            character.version = version_str[1..].parse::<u8>().unwrap_or(1);
        } else {
            character.version = 1; // Default to 1 if no version is found
        }

        // Set character file name for future use in lore branching
        character.character_name = character_name
            .split('.')
            .next()
            .filter(|&s| !s.is_empty())
            .unwrap_or(character_name)
            .to_string();

        Ok(character)
    }

    pub fn stringify(&self) -> Result<String> {
        serde_json::to_string_pretty(self).map_err(Error::new)
    }

    pub fn add_previous_post(&mut self, post: &str) {
        if self.previous_posts.len() >= 5 {
            self.previous_posts.pop_front();
        }
        self.previous_posts.push_back(post.to_string());
    }

    pub fn should_branch(&mut self) -> bool {
        self.posts_since_branch += 1;

        if self.posts_since_branch >= *POSTS_BEFORE_BRANCH {
            self.posts_since_branch = 0;
            true
        } else {
            false
        }
    }

    pub fn save(&mut self, json: &str) -> Result<Self> {
        // Increment version
        self.version += 1;

        // Generate new file name using character name and version
        let path =
            Path::new("characters").join(format!("{}.v{}.json", self.character_name, self.version));

        // Create new character struct from input JSON
        let mut updated_character = serde_json::from_str::<Character>(json.trim())?;

        // Save file
        let temp_path = path.with_extension("tmp");
        {
            let mut character_file = OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(true)
                .open(&temp_path)?;
            serde_json::to_writer_pretty(&mut character_file, &updated_character)?;
        }
        fs::rename(temp_path, path)?;

        // Set the previous character metadata to new one
        updated_character.version = self.version;
        updated_character.character_name = self.character_name.clone();
        Ok(updated_character)
    }
}
