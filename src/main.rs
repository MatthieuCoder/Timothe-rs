use std::{sync::Arc, time::Duration};

use anyhow::{bail, Context};
use calendar::CalendarWatcher;
use chrono::Utc;
use config::{Config, Environment, File};
use handler::Data;
use log::{error, info};
use poise::serenity_prelude::{self as serenity};
use tokio::{signal, sync::RwLock, time::sleep};

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
    match error {
        poise::FrameworkError::Setup { error, .. } => panic!("Failed to start bot: {:?}", error),
        poise::FrameworkError::Command { error, ctx } => {
            let _ = ctx.send(|f| f.ephemeral(true).content(format!("{:?}", error))).await;
            error!("Error in command `{}`: {:?}", ctx.command().name, error);
        }
        error => {
            if let Err(e) = poise::builtins::on_error(error).await {
                error!("Error while handling error: {}", e)
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

    let schedule = saffron::Cron::new(match config.calendar.refetch.parse() {
        Ok(r) => r,
        Err(e) => bail!("failed to parse the cron expression: {}", e),
    });

    let (shutdown_send, _shutdown_recv) = tokio::sync::broadcast::channel(1);

    let options = poise::FrameworkOptions {
        commands: vec![
            commands::register(),
            commands::help(),
            commands::schedule::summary::root(),
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

    let w0 = watcher.clone();
    let w1 = watcher.clone();

    let framework = poise::Framework::builder()
        .token(config.discord.token.clone())
        .user_data_setup(move |_ctx, _, _| {
            Box::pin(async move {
                Ok(Data {
                    config,
                    scheduler: w0,
                })
            })
        })
        .options(options)
        .intents(
            serenity::GatewayIntents::non_privileged(),
        )
        .build()
        .await
        .context("failed to create framework")?;
    
    let mut shutdown = shutdown_send.subscribe();
    let watcher: tokio::task::JoinHandle<Result<(), anyhow::Error>> = tokio::spawn(async move {
        // update calendars at the start to ensure availability.
        let mut wat = w1.write().await;
        wat.update_calendars().await?;
        // force unlock of the lock guard
        drop(wat);

        loop {
            // calculate the next cron execution and wait
            let current_time = Utc::now();

            // this souldn't fail.
            // if it does, we should terminate
            let next = schedule
                .next_after(current_time)
                .context("failed to get next date")?;

            let sleep_time = next - current_time;
            info!("waiting {}s, trigger at {}", sleep_time.num_seconds(), next);

            let wait = sleep(
                sleep_time
                    .to_std()
                    .context("failed to convert a chrono duration to a std duration")?,
            );

            tokio::select! {
                _ = wait => {
                    let mut wat = w1.write().await;
                    let _updates = wat.update_calendars().await?;
                },
                _ = shutdown.recv() => {
                    return Ok(());
                }
            }
        }
    });

    let mut shutdown = shutdown_send.subscribe();
    let discord = tokio::spawn(async move {
        let task = framework
            .start_autosharded();

        tokio::select! {
            result = task => { result },
            _ = shutdown.recv() => {
                Ok(())
            }
        }
    });

    let stop = tokio::spawn(async move {
        match signal::ctrl_c().await {
            Ok(()) => {
                shutdown_send
                    .send(())
                    .context("failed to send a shutdown signal")?;
            }
            Err(err) => {
                bail!(err)
            }
        }

        Ok(())
    });

    stop.await??;
    discord.await??;
    watcher.await??;

    Ok(())
}
