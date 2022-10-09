use chrono::{Duration, Utc};

use crate::handler::{Context, Error};

pub mod summary;

#[poise::command(
    slash_command,
    rename = "schedule",
    name_localized("en-US", "schedule"),
    description_localized("en-US", "Command used to manage the schedules"),
    subcommands("summary")
)]
pub async fn root(_: Context<'_>) -> Result<(), Error> {
    unreachable!();
}

#[poise::command(slash_command)]
pub async fn summary(ctx: Context<'_>) -> Result<(), Error> {
    let sch = ctx.data();
    let user_roles = &ctx.author_member().await.unwrap().roles;

    let duration = Duration::days(2);
    let from = Utc::now() + Duration::days(7);
    let to = from + duration;

    let user_calendars = sch
        .config
        .calendar
        .watchers
        .iter()
        .filter(|watcher| user_roles.contains(&watcher.1.role));

    let a = sch.scheduler.read().await;

    let b = user_calendars
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

    let mut response = format!(
        "**Emploi du temps, de <t:{}> à <t:{}>:**\n\n",
        from.timestamp(),
        to.timestamp()
    );

    for c in b {
        response += &format!(
            "**{} {}**\n\tde <t:{}> à <t:{}>\n\t`{}`\n\tDernière modification le <t:{}>\n\n",
            c.location,
            c.summary,
            c.start.timestamp(),
            c.end.timestamp(),
            c.description.replace("\\n", ""),
            c.last_modified.timestamp()
        );
    }

    ctx.send(|f| f.content(response)).await?;

    Ok(())
}
