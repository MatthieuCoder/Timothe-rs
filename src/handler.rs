use std::sync::Arc;
use tokio::sync::RwLock;
use crate::{cfg::Config, calendar::CalendarWatcher};

pub type Error = anyhow::Error;
pub type Context<'a> = poise::Context<'a, Data, Error>;

// User data, which is stored and accessible in all command invocations
pub struct Data {
    pub config: Arc<Config>,
    pub scheduler: Arc<RwLock<CalendarWatcher>>
}
