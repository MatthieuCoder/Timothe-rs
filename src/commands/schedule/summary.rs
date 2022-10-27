use anyhow::Context;
use chrono::{Datelike, Duration, Timelike, Utc};
use futures::{Stream, StreamExt};
use log::debug;
use poise::serenity_prelude::Color;

use crate::bot::CommandContext;

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

#[poise::command(
    slash_command,
    rename = "schedule",
    name_localized("en-US", "schedule"),
    description_localized("en-US", "Command used to manage the schedules"),
    subcommands("summary", "groups")
)]
pub async fn root(_: CommandContext<'_>) -> Result<(), anyhow::Error> {
    unreachable!();
}

#[poise::command(slash_command, guild_only)]
/// Liste les groupes de l'utilisateur
pub async fn groups(ctx: CommandContext<'_>) -> Result<(), anyhow::Error> {
    let sch = ctx.data();
    let user_roles = &ctx
        .author_member()
        .await
        .context("This command should be run in a guild.")?
        .roles;

    let user_calendars = sch
        .config
        .calendar
        .calendars
        .iter()
        .filter(|watcher| user_roles.iter().any(|f| watcher.1.role.contains(f)));

    let mut response = format!("**Vous faites partie des groupes: **\n\n");

    for (name, _) in user_calendars {
        response += &format!("\t**\\* {}**", name);
    }

    ctx.send(|f| f.ephemeral(true).content(response)).await?;
    Ok(())
}

async fn autocomplete_schedule<'a>(
    ctx: CommandContext<'_>,
    partial: &'a str,
) -> impl Stream<Item = String> + 'a {
    let guild = ctx.guild();

    let names: Vec<String> = ctx
        .data()
        .config
        .calendar
        .calendars
        .iter()
        .filter(|(_, calendar)| {
            if let Some(guild) = &guild {
                calendar
                    .role
                    .iter()
                    .any(|role| guild.roles.contains_key(role))
            } else {
                true
            }
        })
        .map(|name| name.0.to_string())
        .collect();

    futures::stream::iter(names)
        .filter(move |name| futures::future::ready(name.starts_with(partial)))
        .map(|name| name.to_string())
}

#[poise::command(slash_command)]
/// Affiche un résumé pour les prochains jours
pub async fn summary(
    ctx: CommandContext<'_>,

    #[description = "L'emploi du temps à inspecter"]
    #[autocomplete = "autocomplete_schedule"]
    schedule: Option<String>,
) -> Result<(), anyhow::Error> {
    let data = ctx.data();
    let member = &ctx.author_member().await;

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
            if let Some(member) = member {
                member.roles.iter().any(|f| watcher.1.role.contains(f))
            } else {
                false
            }
        }
    });

    let reader = data.calendar_manager.read().await;

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
        .context("Could't find any calendar matching.")?;

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

                let h = ((event.start.date().day() % 3) as f64 / 3f64) * 360f64;
                let l = (event.start.time().hour() as f64) / 14f64;

                debug!("h: {}, l: {}", h, l);

                let color = hsl_to_rgb(h as u32, 0.75f64, 1f64 - l);

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
