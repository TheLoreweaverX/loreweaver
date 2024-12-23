pub mod clients;
pub mod core;
pub mod db;

use anyhow::Result;
use chrono::{TimeZone, Utc};
use clients::twitter::twitter::TwitterAuth;
use core::{
    character::Character, cli::Instance as CliInstance, twitter::Instance as TwitterInstance,
};
use db::mongo::Credentials as MongoCredentials;
use dotenv::from_filename;
use fern::colors::ColoredLevelConfig;
use std::env;

#[tokio::main]
async fn main() -> Result<()> {
    let colors = ColoredLevelConfig::new()
        .info(fern::colors::Color::BrightGreen)
        .error(fern::colors::Color::BrightRed)
        .warn(fern::colors::Color::BrightYellow);

    fern::Dispatch::new()
        .format(move |out, message, record| {
            out.finish(format_args!(
                "[{} | {} | arc_fork] {}",
                Utc.timestamp_millis(Utc::now().timestamp_millis())
                    .format("%H:%M:%S.%3f")
                    .to_string(),
                colors.color(record.level()),
                message
            ))
        })
        .level(log::LevelFilter::Info)
        .chain(std::io::stdout())
        .apply()
        .unwrap();

    let args = env::args().collect::<Vec<String>>();
    let stage = args
        .get(1)
        .map(|arg| arg.trim_start_matches("--"))
        .unwrap_or_else(|| panic!("expected stage argument: --dev or --prod"));

    let character_name = args
        .get(2)
        .unwrap_or_else(|| panic!("expected character name as second argument"));

    if let Err(e) = from_filename(format!(".env.{stage}")) {
        panic!("fatal error occurred loading env file: {e}");
    }

    let anthropic_api_key = env::var("ANTHROPIC_API_KEY")
        .expect("`ANTHROPIC_API_KEY` is a required environment variable");
    let openai_api_key =
        env::var("OPENAI_API_KEY").expect("OPENAI_API_KEY` is a required environment variable");

    let use_stats =
        env::var("USE_STATS").expect("USE_STATS is a required environment variable") == "true";
    let stats_collection = if use_stats {
        env::var("MONGO_CONN_STATS_COLLECTION")
            .expect("MONGO_CONN_STATS_COLLECTION is a required environment variable")
    } else {
        String::new()
    };

    let mongo_credentials = MongoCredentials {
        conn_url: env::var("MONGO_CONN_URL")
            .expect("MONGO_CONN_URL` is a required environment variable"),
        db: env::var("MONGO_CONN_DB").expect("MONGO_CONN_DB` is a required environment variable"),
        vec_collection: env::var("MONGO_CONN_VEC_COLLECTION")
            .expect("MONGO_CONN_VEC_COLLECTION` is a required environment variable"),
        stats_collection: stats_collection,
    };

    let twitter_credentials = TwitterAuth {
        api_key: env::var("TWITTER_API_KEY")
            .expect("`TWITTER_API_KEY` is a required environment variable"),
        api_secret: env::var("TWITTER_API_SECRET")
            .expect("`TWITTER_API_SECRET` is a required environment variable"),
        access_token: env::var("TWITTER_ACCESS_TOKEN")
            .expect("`TWITTER_ACCESS_TOKEN` is a required environment variable"),
        access_token_secret: env::var("TWITTER_ACCESS_TOKEN_SECRET")
            .expect("`TWITTER_ACCESS_TOKEN_SECRET` is a required environment variable"),
    };

    let character = Character::load(&character_name)?;

    if env::var("USE_CLI").map_or(false, |val| val == "true") {
        let mut cli_instance = CliInstance::new(&anthropic_api_key, character)
            .await
            .expect("Failed to create CLI instance");
        cli_instance
            .run()
            .await
            .expect("Failed to run CLI instance");
    } else {
        let mut twitter_instance = TwitterInstance::new(
            &anthropic_api_key,
            &openai_api_key,
            mongo_credentials,
            twitter_credentials,
            character,
            use_stats,
        )
        .await
        .expect("Failed to create CLI instance");
        twitter_instance.run().await
    }
    Ok(())
}
