use crate::calendar::manager_task;
use crate::{calendar::manager::Manager, cfg::Config, commands};
use anyhow::Context;
use futures::stream::FuturesUnordered;
use futures::StreamExt;
use log::error;
use poise::serenity_prelude::{ClientBuilder, GatewayIntents};
use poise::CreateReply;
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
        poise::FrameworkError::Command { error, ctx, .. } => {
            let f = CreateReply::default()
                .ephemeral(true)
                .content(format!("{:?}", error));
            std::mem::drop(ctx.send(f).await);
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

        let data = Arc::new(Data {
            config: config.clone(),
            calendar_manager,
        });

        Ok(Arc::new(Self {
            data,
            shutdown,
            shutdown_send,
        }))
    }
    pub async fn start(self: Arc<Self>) -> Result<(), anyhow::Error> {
        let mut shutdown = self.shutdown.resubscribe();
        let mut tasks = FuturesUnordered::new();

        let options = poise::FrameworkOptions {
            commands: vec![commands::help(), commands::schedule::summary::root()],
            prefix_options: poise::PrefixFrameworkOptions {
                prefix: None,
                edit_tracker: Some(Arc::new(poise::EditTracker::for_timespan(
                    Duration::from_secs(3600),
                ))),
                mention_as_prefix: true,
                ..Default::default()
            },
            on_error: |error| Box::pin(on_error(error)),
            ..Default::default()
        };
        let data = self.data.clone();
        let framework = poise::Framework::builder()
            .options(options)
            .setup(move |ctx, _ready, framework| {
                Box::pin(async move {
                    poise::builtins::register_globally(ctx, &framework.options().commands).await?;
                    Ok(data)
                })
            })
            .build();
        let client = ClientBuilder::new(
            self.data.config.discord.token.clone(),
            GatewayIntents::non_privileged(),
        )
        .framework(framework);

        let mut client = client.await.unwrap();
        let http = client.http.clone();

        tasks.push(tokio::spawn(async move {
            // wait until the bot terminates or a shutdown signal is received.
            tokio::select! {
                result = client.start_autosharded() => {
                    if let Err(err) = result {
                        error!("Client error: {}", err);
                    }
                },
                _ = shutdown.recv() => {
                    // shutdown the bot properly
                    client.shard_manager.shutdown_all().await;
                }
            };
        }));
        let self_clone = self.clone();
        tasks.push(tokio::spawn(async {
            let _ = manager_task(self_clone, http).await;
        }));
        let self_clone = self.clone();
        tasks.push(tokio::spawn(async {
            let _ = wait_for_stop_signal(self_clone).await;
        }));

        // wait for a task to finish.
        let task = tasks
            .next()
            .await
            .context("no tasks started, illegal state")?
            .context("failed to join task");

        // when a task is finished, we must terminate all the others,
        // hence we send a signal talling all tasks to stop processing
        // and return.
        self.shutdown_send.send(())?;

        while let Some(operation) = tasks.next().await {
            operation.context("failed to join task")?;
        }
        
        task?;
        Ok(())
    }
}
