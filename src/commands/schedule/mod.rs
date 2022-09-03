use crate::handler::{Context, Error};

pub mod summary;

#[poise::command(
    slash_command,
    rename = "schedule",

    name_localized("en-US","schedule"),
    description_localized("en-US","Command used to manage the schedules"),


)]
pub async fn parent(_: Context<'_>) -> Result<(), Error> {
    unreachable!();
}
