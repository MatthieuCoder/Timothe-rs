use std::{collections::HashMap, time::Duration};
use poise::serenity_prelude::{ChannelId, RoleId};
use serde::Deserialize;

#[derive(Deserialize, Debug, Clone, Default)]
pub struct DiscordConfig {
    pub token: String,
    pub prefix: String,
}

#[derive(Deserialize, Debug, Clone, Default)]
pub struct CalendarItem {
    pub source: String,
    pub channel: ChannelId,
    pub role: RoleId,
    pub fetch_time: String,
}

#[derive(Deserialize, Debug, Clone, Default)]
pub struct CalendarConfig {
    #[serde(flatten)]
    pub watchers: HashMap<String, CalendarItem>,
    pub cron_task: String,
}

#[derive(Deserialize, Debug, Clone, Default)]
pub struct StorageConfig {
    pub path: String,
}

#[derive(Deserialize, Debug, Clone, Default)]
pub struct Config {
    pub discord: DiscordConfig,
    pub calendar: CalendarConfig,
    pub storage: StorageConfig,
}
