use std::{collections::HashMap, sync::Arc};

use anyhow::Context;
use chrono::{Datelike, NaiveDateTime, Timelike, Utc};
use log::{debug, error, info};
use poise::serenity_prelude::{Color, CreateEmbed};
use serde::{Deserialize, Serialize};
use tokio::time::sleep;

use crate::bot::Bot;

pub mod schedule;
pub mod manager;

#[derive(PartialEq, Eq, Debug)]
pub enum UpdateResult {
    Created(Arc<Event>),
    Updated {
        old: Arc<Event>,
        new: Arc<Event>,
    },
    Removed(Arc<Event>),
}

#[derive(Debug, Default, Eq, PartialEq, Clone, Serialize, Deserialize)]
/// This struct is stored in disk and indexed by it's uid (from ADE)
/// We can simply diff the events using their uid.
pub struct Event {
    /// Summary of the event (Title)
    pub summary: String,
    /// Start of the event. (Utc aligned according to the iCalendar spec)
    pub start: NaiveDateTime,
    /// End of the event. (Utc aligned according to the iCalendar spec)
    pub end: NaiveDateTime,
    /// Where the event takes place.
    pub location: String,
    /// Description of the event.
    pub description: String,
    /// Unique id of the event.
    pub uid: String,
}

impl From<&UpdateResult> for CreateEmbed {
    fn from(event: &UpdateResult) -> Self {
        let mut f = Self::default();

        f.color(match event {
            UpdateResult::Created(_) => Color::DARK_GREEN,
            UpdateResult::Updated { .. } => Color::BLUE,
            UpdateResult::Removed(_) => Color::RED,
        })
        .title(match &event {
            UpdateResult::Created(event) | UpdateResult::Removed(event) => event.summary.clone(),
            UpdateResult::Updated { old, new } => {
                if old.summary == new.summary {
                    new.summary.clone()
                } else {
                    format!("{} => {}", old.summary, new.summary)
                }
            }
        })
        .description(match &event {
            UpdateResult::Created(event) | UpdateResult::Removed(event) => format!(
                "<t:{}> à <t:{}>\n`{}`",
                event.start.timestamp(),
                event.end.timestamp(),
                event.description.replace("\\n", " ")
            ),
            UpdateResult::Updated { old, new } => {
                format!(
                    "{}\n{}",
                    if old.start != new.start || old.end != new.end {
                        format!(
                            "Anciennement <t:{}> à <t:{}>, désormais <t:{}> à <t:{}>",
                            old.start.timestamp(),
                            old.end.timestamp(),
                            new.start.timestamp(),
                            new.end.timestamp()
                        )
                    } else {
                        format!(
                            "<t:{}> à <t:{}>",
                            new.start.timestamp(),
                            new.end.timestamp()
                        )
                    },
                    if old.description == new.description {
                        format!("`{}`", new.description.replace("\\n", ""))
                    } else {
                        format!(
                            "`{}` => `{}`",
                            old.description.replace("\\n", ""),
                            new.description.replace("\\n", "")
                        )
                    }
                )
            }
        });

        match event {
            UpdateResult::Created(event) | UpdateResult::Removed(event) => {
                if !event.location.is_empty() {
                    f.field("Emplacement", &event.location, true);
                }
            }
            UpdateResult::Updated { old, new } => {
                if !old.location.is_empty() || !new.location.is_empty() {
                    f.field(
                        "Emplacement",
                        format!("`{}` => `{}`", old.location, new.location),
                        true,
                    );
                }
            }
        }

        f
    }
}

/// Convert a hsl color to rgb; This is used to make the color gradients
#[allow(clippy::cast_sign_loss)]
#[allow(clippy::cast_possible_truncation)]
#[allow(clippy::many_single_char_names)]
fn hsl_to_rgb(h: u32, s: f64, l: f64) -> Color {
    let c = (1f64 - (2f64 * l - 1f64).abs()) * s;
    let x = c * (1f64 - (f64::from((h / 60) % 2) - 1f64));
    let m = l - c / 2f64;

    let (r0, g0, b0): (f64, f64, f64) = if h < 60 {
        (c, x, 0f64)
    } else if h < 120 {
        (x, c, 0f64)
    } else if h < 180 {
        (0f64, c, x)
    } else if h < 240 {
        (0f64, x, c)
    } else if h < 300 {
        (x, 0f64, c)
    } else if h <= 360 {
        (c, 0f64, x)
    } else {
        unreachable!()
    };

    Color::from_rgb(
        ((r0 + m) * 255f64) as u8,
        ((g0 + m) * 255f64) as u8,
        ((b0 + m) * 255f64) as u8,
    )
}

impl From<&Event> for CreateEmbed {
    fn from(event: &Event) -> Self {
        let mut f = Self::default();
        let h = (f64::from(event.start.date().day() % 10) / 3f64) * 360f64;
        let l = f64::from(event.start.time().hour()) / 14f64;

        debug!("h: {}, l: {}", h, l);
        
        #[allow(clippy::cast_sign_loss)]
        #[allow(clippy::cast_possible_truncation)]
        let color = hsl_to_rgb(h as u32, 0.75f64, 1f64 - l);

        f.title(&event.summary).color(color).description(format!(
            "<t:{}> à <t:{}>\n`{}`",
            event.start.timestamp(),
            event.end.timestamp(),
            event.description.replace("\\n", " ")
        ));

        if !event.location.is_empty() {
            f.field("Emplacement", &event.location, true);
        }
        f
    }
}
async fn process_events(bot: Arc<Bot>, updates_map: HashMap<String, Vec<UpdateResult>>) {
    let http = bot.framework.client().cache_and_http.http.clone();
    for (calendar_name, updates) in updates_map {
        let calendar = bot
            .data
            .config
            .calendar
            .calendars
            .get(&calendar_name)
            .unwrap();

        for channel in &calendar.channel {
            let mut embeds: Vec<CreateEmbed> = vec![];
            for update in &updates {
                embeds.push(update.into());
            }

            let chunks = embeds.chunks(10);

            for chunk in chunks {
                let chunk = chunk.to_vec();
                match channel
                    .send_message(http.clone(), |f| {
                        f.set_embeds(chunk);
                        f
                    })
                    .await
                {
                    Ok(_) => {
                        info!("sent message for updates!");
                    }
                    Err(err) => error!(
                        "failed to send to the channel {} for {}: {}",
                        channel, calendar_name, err
                    ),
                };
            }
        }
    }
}

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
                process_events(bot.clone(), updates).await;
            },
            _ = shutdown.recv() => {
                return Ok(());
            }
        }
    }
}
