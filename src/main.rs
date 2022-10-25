use std::{sync::Arc, time::Duration};

use anyhow::bail;
use calendar::CalendarWatcher;
use chrono::Utc;
use config::{Config, Environment, File};
use handler::Data;
use log::{info, error};
use poise::serenity_prelude::{self as serenity, ChannelId};
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
    match error {
        poise::FrameworkError::Setup { error, .. } => panic!("Failed to start bot: {:?}", error),
        poise::FrameworkError::Command { error, ctx } => {
            let _ = ctx.send(|f| f.content(format!("And error occured! ```{}```", error))).await;
            error!("Error in command `{}`: {:?}", ctx.command().name, error,);
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

    let schedule = saffron::Cron::new(match config.calendar.cron_task.parse() {
        Ok(r) => r,
        Err(e) => bail!("failed to parse the cron expression: {}", e),
    });

    let (shutdown_send, mut shutdown_recv) = mpsc::unbounded_channel::<()>();

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
    let w0 = watcher.clone();
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
            serenity::GatewayIntents::non_privileged() | serenity::GatewayIntents::MESSAGE_CONTENT,
        )
        .build()
        .await
        .expect("failed to create framework");

    let w1 = watcher.clone();
    let f0 = framework.clone().client().cache_and_http.http.clone();
    tokio::spawn(async move {
        {
            let mut wat = w1.write().await;
            wat.update_calendars().await;
        }
        loop {
            let current_time = Utc::now();
            let next = schedule
                .next_after(current_time)
                .expect("failed to get next date");
            info!("waiting {}, trigger in {}", next, next - current_time);
            let wait = sleep((next - current_time).to_std().expect("failed"));
            tokio::select! {
                _ = wait => {
                    let mut wat = w1.write().await;
                    let updates = wat.update_calendars().await;

                    for (name, updates) in updates {

                        for update in updates {
                            // this is a debug channel!
                            ChannelId(1034359605771386941).send_message(&f0, |f| {
                                match update {
                                    calendar::store::UpdateResult::Created(main) => {
                                        f.content(format!("Evènement in {} ajouté: {:?}", name, main))
                                    },
                                    calendar::store::UpdateResult::Updated { old, new } => {

                                        f.content(format!("Evènement in {} modifié: {:?} => {:?}", name, old, new))
                                    },
                                    calendar::store::UpdateResult::Removed(main) =>{
                                        f.content(format!("Evènement in {} supprimé: {:?}", name, main))
                                    },
                                }
                            }).await.expect("failed to send message");
                        }
                    }

                },
                _ = shutdown_recv.recv() => {
                    return;
                }
            }
        }
    });

    tokio::spawn(async move {
        framework
            .start_autosharded()
            .await
            .expect("failed to start");
    });

    match signal::ctrl_c().await {
        Ok(()) => {
            shutdown_send.send(())?;
        }
        Err(err) => {
            error!("Unable to listen for shutdown signal: {}", err);
            // we also shut down in case of error
        }
    }

    Ok(())
}
