use std::{sync::Arc, time::Duration};

use calendar::CalendarWatcher;
use chrono::Utc;
use config::{Config, Environment, File};
use handler::Data;
use log::info;
use poise::serenity_prelude as serenity;
use tokio::{
    signal,
    sync::{mpsc, RwLock},
    time::sleep,
};

mod calendar;
mod cfg;
mod commands;
mod handler;

/// Loads the configuration using the `config` crate
fn load_config() -> Result<cfg::Config, anyhow::Error> {
    let settings = Config::builder()
        .add_source(File::with_name("config"))
        .add_source(Environment::with_prefix("TIMOTHE"))
        .build()?;

    Ok(settings.try_deserialize()?)
}

async fn on_error(error: poise::FrameworkError<'_, Data, handler::Error>) {
    // This is our custom error handler
    // They are many errors that can occur, so we only handle the ones we want to customize
    // and forward the rest to the default handler
    match error {
        poise::FrameworkError::Setup { error, .. } => panic!("Failed to start bot: {:?}", error),
        poise::FrameworkError::Command { error, ctx } => {
            println!("Error in command `{}`: {:?}", ctx.command().name, error,);
        }
        error => {
            if let Err(e) = poise::builtins::on_error(error).await {
                println!("Error while handling error: {}", e)
            }
        }
    }
}

#[tokio::main]
/// Entrypoint for the Timothe discord bot.
/// Timothee is a simple discord that watches any ICS calendar and warns a set of users when it changes.
///
/// It's developped and maintained by Matthieu Pignolet <matthieu@matthieu-dev.xyz> on github (https://github.com/MatthieuCoder/Timothe-rs)
async fn main() -> Result<(), anyhow::Error> {
    // Initialize the logger
    // Might be replaced later
    pretty_env_logger::init();

    // load the config
    let config = Arc::from(load_config()?);
    let watcher = Arc::new(RwLock::new(CalendarWatcher::new(config.clone())?));

    let schedule = saffron::Cron::new(config.calendar.cron_task.parse().unwrap());

    let (shutdown_send, mut shutdown_recv) = mpsc::unbounded_channel::<()>();

    let w1 = watcher.clone();
    tokio::spawn(async move {
        {
            let mut wat = w1.write().await;
            wat.update_calendars().await;
        }
        loop {
            let current_time = Utc::now();
            let next = schedule.next_after(current_time).unwrap();
            info!("waiting {}, trigger in {}", next, next - current_time);
            let wait = sleep((next - current_time).to_std().unwrap());

            tokio::select! {
                _ = wait => {
                    let mut wat = w1.write().await;
                    wat.update_calendars().await;
                },
                _ = shutdown_recv.recv() => {
                    return;
                }
            }
        }
    });

    let w2 = watcher.clone();
    tokio::spawn(async move {
        let options = poise::FrameworkOptions {
            commands: vec![
                commands::register(),
                commands::help(),
                commands::schedule::root(),
            ],
            prefix_options: poise::PrefixFrameworkOptions {
                prefix: None,
                edit_tracker: Some(poise::EditTracker::for_timespan(Duration::from_secs(3600))),
                mention_as_prefix: true,
                ..Default::default()
            },
            on_error: |error| Box::pin(on_error(error)),
            
            
            ..Default::default()
        };
        poise::Framework::builder()
            .token(config.discord.token.clone())
            
            .user_data_setup(move |_ctx, _ready, _framework| {
                Box::pin(async move {
                    Ok(Data {
                        config,
                        scheduler: w2,
                    })
                })
            })
            .options(options)
            .intents(
                serenity::GatewayIntents::non_privileged()
                    | serenity::GatewayIntents::MESSAGE_CONTENT,
            )
            .run_autosharded()
            .await
            .unwrap();
    });

    match signal::ctrl_c().await {
        Ok(()) => {
            shutdown_send.send(())?;
        }
        Err(err) => {
            eprintln!("Unable to listen for shutdown signal: {}", err);
            // we also shut down in case of error
        }
    }

    Ok(())
}
