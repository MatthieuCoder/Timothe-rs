use crate::handler::{Context, Error};

pub mod schedule;

#[poise::command(
    prefix_command,
    owners_only
)]
pub async fn register(
    ctx: Context<'_>,
) -> Result<(), Error> {
    poise::builtins::register_application_commands_buttons(ctx).await?;
    Ok(())
}
