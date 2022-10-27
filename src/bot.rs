use crate::calendar::manager_task;
use crate::{calendar::manager::Manager, cfg::Config, commands};
use anyhow::anyhow;
use anyhow::Context;
use futures::stream::FuturesUnordered;
use futures::StreamExt;
use log::error;
use poise::serenity_prelude::{self as serenity};
use poise::Framework;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::broadcast::Receiver;
use tokio::{
    signal,
    sync::{broadcast::Sender, RwLock},
};

pub type CommandContext<'a> = poise::Context<'a, Arc<Data>, anyhow::Error>;

// User data, which is stored and accessible in all command invocations
pub struct Data {
    pub config: Arc<Config>,
    pub calendar_manager: Arc<RwLock<Manager>>,
}

pub struct Bot {
    pub data: Arc<Data>,
    pub shutdown: Receiver<()>,
    pub framework: Arc<Framework<Arc<Data>, anyhow::Error>>,
    shutdown_send: Sender<()>,
}

/// Sends a message through `shutdown_send` when a stop signal is detected.
/// Used to start the bot stop sequence.
async fn wait_for_stop_signal(bot: Arc<Bot>) -> Result<(), anyhow::Error> {
    let mut shutdown = bot.shutdown.resubscribe();
    tokio::select! {
        result = signal::ctrl_c() => {
            match result {
                Ok(()) => {
                    bot.shutdown_send
                        .send(())
                        .context("failed to send a shutdown signal")?;
                    Ok(())
                }
                Err(err) => Err(anyhow::anyhow!(err)),
            }
        },
        _ = shutdown.recv() => { Ok(()) }
    }
}

async fn on_error(error: poise::FrameworkError<'_, Arc<Data>, anyhow::Error>) {
    match error {
        poise::FrameworkError::Setup { error, .. } => panic!("Failed to start bot: {:?}", error),
        poise::FrameworkError::Command { error, ctx } => {
            std::mem::drop(
                ctx.send(|f| f.ephemeral(true).content(format!("{:?}", error)))
                    .await,
            );
            error!("Error in command `{}`: {:?}", ctx.command().name, error);
        }
        error => {
            if let Err(e) = poise::builtins::on_error(error).await {
                error!("Error while handling error: {}", e);
            }
        }
    }
}

impl Bot {
    pub async fn new(config: Arc<Config>) -> Result<Arc<Self>, anyhow::Error> {
        // Theses signals are used to stop the many tasks trigered.
        // this is called by the task listening for a stop signal.
        let (shutdown_send, shutdown) = tokio::sync::broadcast::channel(1);

        // initialize the calenar manager
        let calendar_manager = Arc::new(RwLock::new(Manager::new(config.clone())?));

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

        let data = Arc::new(Data {
            config: config.clone(),
            calendar_manager,
        });

        let new_data_ref = data.clone();
        let framework = poise::Framework::builder()
            .token(config.discord.token.clone())
            .user_data_setup(move |_ctx, _, _| Box::pin(async move { Ok(new_data_ref.clone()) }))
            .options(options)
            .intents(serenity::GatewayIntents::non_privileged())
            .build()
            .await
            .context("failed to create framework")?;

        Ok(Arc::new(Self {
            data,
            shutdown,
            framework,
            shutdown_send,
        }))
    }

    pub async fn start(self: Arc<Self>) -> Result<(), anyhow::Error> {
        let mut shutdown = self.shutdown.resubscribe();
        let mut tasks = FuturesUnordered::new();
        let http = self.framework.client().cache_and_http.http.clone();

        let this = self.clone();
        // runs the discord bot usign autosharded mode
        tasks.push(tokio::spawn(async move {
            let task = this.framework.clone().start_autosharded();

            // wait until the bot terminates or a shutdown signal is received.
            tokio::select! {
                result = task => { match result { Ok(result) => Ok(result), Err(error) => Err(anyhow!(error)) } },
                _ = shutdown.recv() => {
                    // shutdown the bot properly
                    this.framework.shard_manager().lock().await.shutdown_all().await;
                    Ok(())
                }
            }
        }));

        tasks.push(tokio::spawn(manager_task(self.clone(), http)));
        tasks.push(tokio::spawn(wait_for_stop_signal(self.clone())));

        // wait for a task to finish.
        let task = tasks
            .next()
            .await
            .context("no tasks started, illegal state")?
            .context("failed to join task")?;

        // when a task is finished, we must terminate all the others,
        // hence we send a signal talling all tasks to stop processing
        // and return.
        self.shutdown_send.send(())?;

        while let Some(operation) = tasks.next().await {
            let operation = operation.context("failed to join task")?;
            // return immediately if any task shut down unexpectedly
            operation?;
        }

        // return an error if the first task failed
        task?;
        Ok(())
    }
}
