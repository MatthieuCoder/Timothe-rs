use std::{str::FromStr, sync::Arc};

use config::{Config, ConfigError, Environment, File};
use cron::Schedule;
use calendar::CalendarWatcher;
use log::error;
use tokio_cron_scheduler::{Job, JobScheduler};

mod cfg;
mod commands;
mod handler;
mod calendar;

/// Loads the configuration using the `config` crate
fn load_config() -> Result<cfg::Config, ConfigError> {
    let settings = Config::builder()
        .add_source(File::with_name("config"))
        .add_source(Environment::with_prefix("TIMOTHE"))
        .build()?;

    Ok(settings.try_deserialize()?)
}

#[tokio::main]
/// Entrypoint for the Timothe discord bot.
/// Timothee is a simple discord that watches any ICS calendar and warns a set of users when it changes.
///
/// It's developped and maintained by Matthieu Pignolet <matthieu@matthieu-dev.xyz> on github (https://github.com/MatthieuCoder/Timothee-rs)
async fn main() {
    // Initialize the logger
    // Might be replaced later
    pretty_env_logger::init();

    let config = Arc::from(match load_config() {
        Ok(config) => config,
        Err(err) => {
            // todo: error handling enum
            error!("failed to load config: {}", err);
            // todo: find a better way to abord execution
            return;
        }
    });

    let scheduler = JobScheduler::new().await.unwrap();
    let watcher = Arc::new(CalendarWatcher::new(config.clone()));
    watcher.update_calendars().await;

    let schedule =
        Schedule::from_str(&config.calendar.cron_task).expect("invalid cron task syntax");

    let watcher_task = Job::new_async(schedule, move |_, _l| {
        let watcher = watcher.clone();
        Box::pin(async move {
            watcher.update_calendars().await;
        })
    })
    .unwrap();

    scheduler.add(watcher_task).await.unwrap();

    scheduler.start().await.unwrap();
}
