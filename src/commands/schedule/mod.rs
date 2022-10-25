use chrono::{Duration, Utc};
use futures::{Stream, StreamExt};
use poise::serenity_prelude::Color;

use crate::handler::{Context, Error};

pub mod summary;

#[poise::command(
    slash_command,
    rename = "schedule",
    name_localized("en-US", "schedule"),
    description_localized("en-US", "Command used to manage the schedules"),
    subcommands("summary", "groups")
)]
pub async fn root(_: Context<'_>) -> Result<(), Error> {
    unreachable!();
}

#[poise::command(slash_command)]
/// Liste les groupes de l'utilisateur
pub async fn groups(ctx: Context<'_>) -> Result<(), Error> {
    let sch = ctx.data();
    let user_roles = &ctx.author_member().await.unwrap().roles;

    let user_calendars = sch
        .config
        .calendar
        .watchers
        .iter()
        .filter(|watcher| user_roles.contains(&watcher.1.role));

    let mut response = format!("**Vous faites partie des groupes: **\n\n");

    for (name, _) in user_calendars {
        response += &format!("\t**\\* {}**", name);
    }

    ctx.send(|f| f.content(response)).await?;
    Ok(())
}

async fn autocomplete_schedule<'a>(
    ctx: Context<'_>,
    partial: &'a str,
) -> impl Stream<Item = String> + 'a {
    let names: Vec<String> = ctx
        .data()
        .config
        .calendar
        .watchers
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
    ctx: Context<'_>,

    #[description = "L'emploi du temps à inspecter"]
    #[autocomplete = "autocomplete_schedule"]
    schedule: Option<String>,
) -> Result<(), Error> {
    let sch = ctx.data();
    let user_roles = &ctx.author_member().await.unwrap().roles;

    let duration = Duration::days(2);
    let from = Utc::now();
    let to = from + duration;

    let user_calendars = sch.config.calendar.watchers.iter().filter(|watcher| {
        if let Some(calendar) = &schedule {
            calendar == watcher.0
        } else {
            user_roles.contains(&watcher.1.role)
        }
    });

    let a = sch.scheduler.read().await;

    let events = user_calendars
        .map(|(name, _)| {
            let calendar = a.store.data.get(name).unwrap();
            let events = calendar.get_range(from.naive_utc(), duration);

            events
        })
        .reduce(|mut f, mut x| {
            f.append(&mut x);
            f
        })
        .unwrap();

    ctx.send(|f| {
        f.ephemeral(true).content(format!(
            "**Emploi du temps, de <t:{}> à <t:{}>:**\n\n",
            from.timestamp(),
            to.timestamp()
        ));

        for event in events {
            f.embed(|e| {
                e.title(&event.summary)
                    .color(Color::from_rgb(
                        (event.end.timestamp() % 255).try_into().unwrap(),
                        (event.end.timestamp() % 255).try_into().unwrap(),
                        (event.start.timestamp() % 255).try_into().unwrap(),
                    ))
                    .description(format!(
                        "<t:{}> à <t:{}>\n`{}`",
                        event.start.timestamp(),
                        event.end.timestamp(),
                        event.description.replace("\\n", "\n")
                    ))
                    .field("Emplacement", &event.location, true)
            });
        }

        f
    })
    .await?;

    Ok(())
}
