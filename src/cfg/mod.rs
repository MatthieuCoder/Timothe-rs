use std::collections::HashMap;

use poise::serenity_prelude::{ChannelId, RoleId};
use serde::Deserialize;

#[derive(Deserialize, Debug, Clone)]
pub struct DiscordConfig {
    pub token: String,
    pub prefix: String,
}

#[derive(Deserialize, Debug, Clone)]
pub struct ICalWatchItem {
    pub source: String,
    pub channel: ChannelId,
    pub role: RoleId,
}

#[derive(Deserialize, Debug, Clone)]
pub struct ICalWatchConfig {
    #[serde(flatten)]
    pub watchers: HashMap<String, ICalWatchItem>,
    pub cron_task: String,
}

#[derive(Deserialize, Debug, Clone)]
pub struct Config {
    pub discord: DiscordConfig,
    pub calendar: ICalWatchConfig,
}
