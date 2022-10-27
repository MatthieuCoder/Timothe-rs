use std::sync::Arc;

use bot::Bot;
use cfg::load_config;
mod bot;
mod calendar;
mod cfg;
mod commands;

#[tokio::main]
async fn main() -> Result<(), anyhow::Error> {
    // Initialize the logger
    // Might be replaced later
    pretty_env_logger::init();

    // load the config and store it in an Arc (this is sub-optimal and MUST need to be replaced later)
    // a reference counter isn't optimal cause this data exists for the whole lifetime of the program.
    // todo: remove arc
    let config = Arc::from(load_config()?);

    // simply start the bot tasks
    let bot = Bot::new(config).await?;
    bot.start().await?;

    Ok(())
}
