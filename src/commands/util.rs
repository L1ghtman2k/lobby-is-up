use serenity::client::Context;
use serenity::model::application::interaction::application_command::ApplicationCommandInteraction;
use serenity::model::application::interaction::InteractionResponseType;

pub async fn create_interaction_response<D: ToString>(
    ctx: Context,
    command: ApplicationCommandInteraction,
    content: D,
) {
    if let Err(why) = command
        .create_interaction_response(&ctx.http, |response| {
            response
                .kind(InteractionResponseType::ChannelMessageWithSource)
                .interaction_response_data(|message| message.content(content))
        })
        .await
    {
        println!("Cannot respond to slash command: {}", why);
    }
}
