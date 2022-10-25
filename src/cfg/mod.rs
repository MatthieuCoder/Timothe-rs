use poise::serenity_prelude::{ChannelId, RoleId};
use serde::Deserialize;
use std::collections::HashMap;

#[derive(Deserialize, Debug, Clone, Default)]
/// Configuration regarding the discord bot configuration
/// this includes the token and status of the discord bot.
pub struct DiscordConfig {
    pub token: String,
}

#[derive(Deserialize, Debug, Clone, Default)]
/// A calendar item is simply a calendar watched by the bot
/// this includes links such as the source (url) the discord channel,
/// roles and fetch_time.
/// Check each field for the documentation and usages.
pub struct CalendarItem {
    /// The source url of the calendar.
    /// this can use the http or https protocol.
    pub source: String,
    /// A list of discord channels where alerts are going to be sent
    pub channel: Vec<ChannelId>,
    /// A list of discord roles liked to the calendar.
    /// this is going to be used to know which calendars belong to which user.
    pub role: Vec<RoleId>,
    /// This specifies the amount of time covered by a request to the source.
    /// For example; For a request, the source can output two weeks of events.
    /// This is used to check if any events are deleted in this time range.
    /// You should always try to put it above what's outputed to avoid missing any deletion
    /// events.
    pub time_amount: String,
}

#[derive(Deserialize, Debug, Clone, Default)]
/// This is the central piece of configuration; It lists all the calendars
/// and specifies the time between updates.
/// Check each field for the documentation and usages.
pub struct CalendarConfig {
    // flattened to allow .calendar.calendar2
    #[serde(flatten)]
    /// List of calendars to watch
    pub calendars: HashMap<String, CalendarItem>,
    /// Specifies the time between updates for all the calendars.
    /// This uses the cron syntax.
    pub refetch: String,
}

#[derive(Deserialize, Debug, Clone, Default)]
/// Specifies the configuration for the database.
///! The database is very much experimental and should be used with caution.
pub struct StorageConfig {
    /// Relative or absolute path to the database file.
    /// this file is versionned and need to be saved on a real disk.
    pub path: String,
}

#[derive(Deserialize, Debug, Clone, Default)]
/// Main configuration structure
/// This does not have any particular meaning; It just contains 
/// all the configuration blocks.
pub struct Config {
    pub discord: DiscordConfig,
    pub calendar: CalendarConfig,
    pub storage: StorageConfig,
}
