use anyhow::Context;
use chrono::{Duration, Utc};
use futures::{Stream, StreamExt};
use log::info;
use poise::{serenity_prelude::CreateEmbed, CreateReply};
use std::fmt::Write;

use crate::bot::CommandContext;

#[allow(clippy::unused_async)]
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

    let mut response = "**Vous faites partie des groupes: **\n\n".to_string();

    for (name, _) in user_calendars {
        write!(response, "\t**\\* {}**", name)?;
    }
    let f = CreateReply::default().ephemeral(true).content(response);
    ctx.send(f).await?;
    Ok(())
}

#[allow(clippy::unused_async)]
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
            guild.as_ref().map_or(true, |guild| {
                calendar
                    .role
                    .iter()
                    .any(|role| guild.roles.contains_key(role))
            })
        })
        .map(|name| name.0.to_string())
        .collect();

    futures::stream::iter(names)
        .filter(move |name| futures::future::ready(name.starts_with(partial)))
        .map(|name| name)
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

    let duration = Duration::days(5);
    let from = Utc::now();
    let to = from + duration;

    // select all the calendars selected by the user
    // either base on the schedules argument or by the
    // roles of the user.
    let calendars = data.config.calendar.calendars.iter().filter(|watcher| {
        schedule.as_ref().map_or_else(
            || {
                member.as_ref().map_or(false, |member| {
                    member.roles.iter().any(|f| watcher.1.role.contains(f))
                })
            },
            |calendar| calendar == watcher.0,
        )
    });

    let reader = data.calendar_manager.read().await;

    let events = calendars
        .map(|(name, _)| {
            let calendar = reader.store.data.get(name)?;
            let events = calendar.get_range(from, duration);

            info!("found {} events for {}", events.len(), name);

            Some(events)
        })
        .filter(std::option::Option::is_some)
        // this is just to have the right type in the reduce function
        // this is safe because we checked if all the members of the iterator are something
        .map(|elem| elem.expect("internal error"))
        .reduce(|mut f, mut x| {
            f.append(&mut x);
            f
        })
        .context("Could't find any calendar matching.")?;

    let mut reply = CreateReply::default().ephemeral(true);
    let mut embed = CreateEmbed::default()
        .title("Résumé des événements à venir")
        .color(0x3498DB)
        .description(format!(
            "Voici les cours du <t:{}> au <t:{}>:",
            from.timestamp(),
            to.timestamp()
        ));

    for event in events {
        let mut string = format!(
            "<t:{}> à <t:{}> - **{}**\n```{}```\n\n",
            event.start.timestamp(),
            event.end.timestamp(),
            event.summary,
            event.description.replace("\\n", " ").trim()
        );
        if !event.location.is_empty() {
            string += format!("`{}`", &event.location).as_str();
        }
        embed = embed.field(&event.summary, string, false);
    }

    reply.embeds.push(embed);

    ctx.send(reply).await?;

    Ok(())
}
