use crate::handler::{Context, Error};

pub mod summary;

#[poise::command(
    slash_command,
    rename = "schedule",
    name_localized("en-US", "schedule"),
    description_localized("en-US", "Command used to manage the schedules"),
    subcommands("summary::summary", "groups")
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

    ctx.send(|f| f.ephemeral(true).content(response)).await?;
    Ok(())
}
