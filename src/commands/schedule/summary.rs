use anyhow::Context;
use chrono::{Duration, Utc, Datelike, Timelike};
use futures::{Stream, StreamExt};
use log::debug;
use poise::serenity_prelude::Color;

use crate::handler::{Context as HandlerContext, Error};

/// Convert a hsl color to rgb; This is used to make the color gradients
fn hsl_to_rgb(h: u32, s: f64, l: f64) -> Color {
    let c = (1f64 - (2f64 * l - 1f64).abs()) * s;
    let x = c * (1f64 - (((h / 60) % 2) as f64 - 1f64));
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
    } else if h < 360 {
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

async fn autocomplete_schedule<'a>(
    ctx: HandlerContext<'_>,
    partial: &'a str,
) -> impl Stream<Item = String> + 'a {
    let names: Vec<String> = ctx
        .data()
        .config
        .calendar
        .calendars
        .keys()
        .map(|name| name.to_string())
        .collect();

    futures::stream::iter(names)
        .filter(move |name| futures::future::ready(name.starts_with(partial)))
        .map(|name| name.to_string())
}

#[poise::command(slash_command)]
/// Affiche un résumé pour les prochains jours
pub async fn summary(
    ctx: HandlerContext<'_>,

    #[description = "L'emploi du temps à inspecter"]
    #[autocomplete = "autocomplete_schedule"]
    schedule: Option<String>,
) -> Result<(), Error> {
    let data = ctx.data();
    let user_roles = &ctx.author_member().await.unwrap().roles;

    let duration = Duration::days(2);
    let from = Utc::now();
    let to = from + duration;

    // select all the calendars selected by the user
    // either base on the schedules argument or by the 
    // roles of the user.
    let calendars = data.config.calendar.calendars.iter().filter(|watcher| {
        if let Some(calendar) = &schedule {
            calendar == watcher.0
        } else {
            user_roles.iter().any(|f| watcher.1.role.contains(f))
        }
    });

    let reader = data.scheduler.read().await;

    let events = calendars
        .map(|(name, _)| {
            let calendar = reader.store.data.get(name)?;
            let events = calendar.get_range(from.naive_utc(), duration);

            Some(events)
        })
        .filter(|elem| elem.is_some())
        // this is just to have the right type in the reduce function
        // this is safe because we checked if all the members of the iterator are something
        .map(|elem| elem.expect("internal error"))
        .reduce(|mut f, mut x| {
            f.append(&mut x);
            f
        })
        .context("couldn't reduce the events")?;

    ctx.send(|f| {
        f.ephemeral(true).content(format!(
            "**Emploi du temps, de <t:{}> à <t:{}>:**\n\n",
            from.timestamp(),
            to.timestamp()
        ));

        for event in events {
            f.embed(|e| {
                debug!(
                    "day: {} hour: {}",
                    event.start.date().day(),
                    event.start.time().hour()
                );

                let h = (event.start.date().day() as f64 * 2345f64) % 360 as f64;
                let v = (event.start.time().hour() as f64) / 14f64;

                debug!("h: {}, v: {}", h, v);

                let color = hsl_to_rgb(h as u32, 1f64, 1f64 - v);

                e.title(&event.summary).color(color).description(format!(
                    "<t:{}> à <t:{}>\n`{}`",
                    event.start.timestamp(),
                    event.end.timestamp(),
                    event.description.replace("\\n", " ")
                ));

                if event.location.len() > 0 {
                    e.field("Emplacement", &event.location, true);
                }
                e
            });
        }

        f
    })
    .await?;

    Ok(())
}
