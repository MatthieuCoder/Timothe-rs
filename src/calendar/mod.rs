use std::sync::Arc;

use anyhow::Context;
use chrono::Utc;
use log::{info, debug};
use tokio::time::sleep;

use crate::bot::Bot;

pub mod manager;
pub mod store;

pub async fn manager_task(bot: Arc<Bot>) -> Result<(), anyhow::Error> {
    // parse the cron expression to a saffon cron expression
    let schedule = saffron::Cron::new(match bot.data.config.calendar.refetch.parse() {
        Ok(r) => Ok(r),
        Err(e) => Err(anyhow::anyhow!(
            "failed to parse the cron expression: {}",
            e
        )),
    }?);
    let mut shutdown = bot.shutdown.resubscribe();

    // update calendars at the start to ensure availability on startup
    bot.data
        .calendar_manager
        .write()
        .await
        .update_calendars()
        .await?;

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
                let updates = bot.data.calendar_manager.write().await.update_calendars().await?;
                debug!("got updates: {:#?}", updates);
            },
            _ = shutdown.recv() => {
                return Ok(());
            }
        }
    }
}
