use crate::lobby_cache::LobbyCache;

use crate::commands::util::create_interaction_response;
use crate::lobby_cache::model::Lobby;

use once_cell::sync::Lazy;
use regex::Regex;
use serenity::builder::CreateApplicationCommand;
use serenity::client::Context;
use serenity::model::application::interaction::application_command::{
    ApplicationCommandInteraction, CommandDataOption,
};
use serenity::model::application::interaction::InteractionResponseType;
use serenity::model::prelude::command::CommandOptionType;
use std::sync::Arc;
use tokio::time::sleep;
use tokio::time::Duration;
use tracing::error;
use tracing::log::debug;

static RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"^aoe2de://0/\d+$").unwrap());

pub(crate) struct LobbyHandler {
    lobby_cache: Arc<LobbyCache>,
}

impl LobbyHandler {
    pub fn new(lobby_cache: Arc<LobbyCache>) -> Self {
        Self { lobby_cache }
    }

    fn get_lobby(&self, game_id: &str) -> Option<Lobby> {
        return self
            .lobby_cache
            .lobby_cache
            .get(game_id)
            .map(|lobby_ref| lobby_ref.clone().lobby);
    }

    pub async fn run(&self, ctx: Context, command: ApplicationCommandInteraction) {
        let options = &command.data.options;
        if let Some(lobby_id) = extract_lobby_id(options) {
            debug!("Lobby ID: {}", lobby_id);

            {
                let last_update = self.lobby_cache.last_update.lock().await;
                if last_update.is_none()
                    || last_update.unwrap().elapsed().unwrap() > Duration::from_secs(60)
                {
                    create_interaction_response(
                        ctx,
                        command,
                        "AOE2.net hasn't replied in over a minute".to_string(),
                    )
                    .await;
                    return;
                }
            }

            let game_id = lobby_id.split('/').last().unwrap();

            match self.get_lobby(game_id) {
                None => {
                    create_interaction_response(ctx, command, "Lobby not found".to_string()).await;
                }
                Some(lobby) => {
                    let mut players = format_players(&lobby);
                    if let Err(why) = command
                        .create_interaction_response(&ctx.http, |response| {
                            response
                                .kind(InteractionResponseType::ChannelMessageWithSource)
                                .interaction_response_data(|message| {
                                    message.embed(|embed| {
                                        embed
                                            .title(format!("{}", lobby_id))
                                            .url(format!("https://aoe2.net/j/{}", game_id))
                                            .description(players.clone());
                                        embed
                                    })
                                })
                        })
                        .await
                    {
                        error!("Cannot respond to slash command: {}", why);
                    }

                    let mut update_receiver = self.lobby_cache.subscribe();

                    loop {
                        debug!("Inside of updater loop");
                        tokio::select! {
                            _ = update_receiver.recv() => {
                                debug!("Received update");
                            }
                            _ = sleep(Duration::from_secs(10)) => {
                                debug!("Mandatory update");
                            }
                        }

                        debug!("Attempting to update interaction response");

                        match self.get_lobby(game_id) {
                            None => {
                                debug!("Lobby no longer running");

                                if let Err(why) = command
                                    .edit_original_interaction_response(&ctx.http, |response| {
                                        response.embed(|embed| {
                                            embed
                                                .title(format!("{}", lobby_id))
                                                .url(format!("https://aoe2.net/j/{}", game_id))
                                                .description("Lobby no longer active");
                                            embed
                                        })
                                    })
                                    .await
                                {
                                    error!("Cannot respond to slash command: {}", why);
                                    break;
                                }

                                break;
                            }

                            Some(lobby) => {
                                debug!("Lobby still running");
                                let new_players = format_players(&lobby);
                                if new_players == players {
                                    debug!("No change in players");
                                    continue;
                                } else {
                                    debug!("Change in players");
                                    players = new_players;
                                }
                                if let Err(why) = command
                                    .edit_original_interaction_response(&ctx.http, |response| {
                                        response.embed(|embed| {
                                            embed
                                                .title(format!("{}", lobby_id))
                                                .url(format!("https://aoe2.net/j/{}", game_id))
                                                .description(players.clone());
                                            embed
                                        })
                                    })
                                    .await
                                {
                                    error!("Cannot respond to slash command: {}", why);
                                    break;
                                }
                            }
                        }
                    }
                }
            }
        } else {
            create_interaction_response(ctx, command, "Invalid lobby id".to_string()).await;
        }
    }

    pub fn register(command: &mut CreateApplicationCommand) -> &mut CreateApplicationCommand {
        command
            .name("lobby")
            .description("Aoe2 Lobby ID")
            .create_option(|option| {
                option
                    .name("lobby_id")
                    .description("The lobby id(ex: aoe2de://0/123456789)")
                    .kind(CommandOptionType::String)
                    .required(true)
            })
    }
}

pub fn extract_lobby_id(options: &[CommandDataOption]) -> Option<String> {
    if let Some(command_application) = options.first() {
        if let Some(value) = &command_application.value {
            if let Some(lobby_id) = value.as_str() {
                let lobby_id = lobby_id.trim();
                if RE.is_match(lobby_id) {
                    return Some(lobby_id.to_string());
                }
            }
        }
    }
    None
}

fn format_players(lobby: &Lobby) -> String {
    let mut content = String::new();
    let mut players = vec![];
    for player in lobby.players.iter() {
        if let Some(name) = &player.name {
            players.push(name.clone());
        } else {
            players.push("Unknown".to_string());
        }
    }
    //Sort players by name
    players.sort();

    for player in players.iter() {
        content.push_str(&format!("{}\n", player));
    }

    content
}
