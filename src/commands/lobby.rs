use crate::lobby_cache::LobbyCache;

use crate::commands::util::create_interaction_response;
use tokio::time::Duration;
use tokio::time::sleep;
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


static RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"^aoe2de://0/\d+$").unwrap());

pub(crate) struct LobbyHandler {
    lobby_cache: Arc<LobbyCache>,
}

impl LobbyHandler {
    pub fn new(lobby_cache: Arc<LobbyCache>) -> Self {
        Self { lobby_cache }
    }

    pub async fn run(&self, ctx: Context, command: ApplicationCommandInteraction) {
        let options = &command.data.options;
        if let Some(lobby_id) = extract_lobby_id(options) {
            println!("Lobby ID: {}", lobby_id);
            let game_id = lobby_id.split('/').last().unwrap();
            let guard = self.lobby_cache.lobby_cache.lock().await;

            match guard.get(game_id) {
                None => {
                    create_interaction_response(ctx, command, "Lobby not found".to_string()).await;
                }
                Some(lobby) => {
                    let lobby = lobby.clone();
                    drop(guard);
                    let mut content = String::new();
                    for player in lobby.lobby.players.iter() {
                        content.push_str(&format!("{:?}\n", player.name));
                    }

                    print!("Sent initial response");

                    if let Err(why) = command
                        .create_interaction_response(&ctx.http, |response| {
                            response
                                .kind(InteractionResponseType::ChannelMessageWithSource)
                                .interaction_response_data(|message| message.embed(|embed| {
                                    embed.title(format!("{}", lobby_id)).url(format!("https://aoe2.net/j/{}", game_id)).description(content);
                                    embed
                                }))
                        })
                        .await
                    {
                        println!("Cannot respond to slash command: {}", why);
                    }

                    println!("Subscribing to receiver");

                    let mut update_receiver = self.lobby_cache.subscribe();

                    loop {
                        println!("Inside of updater loop");
                        tokio::select! {
                            _ = update_receiver.recv() => {
                                println!("Received update");
                            }
                            _ = sleep(Duration::from_secs(10)) => {
                                println!("Mandatory update");
                            }
                        }

                        println!("Attempting to update interaction response");
                        let guard = self.lobby_cache.lobby_cache.lock().await;
                        match guard.get(game_id){
                            None => {
                                println!("Lobby no longer running");
                                create_interaction_response(ctx, command, "Lobby no longer running".to_string()).await;
                                break;
                            }
                            Some(lobby) => {
                                let lobby = lobby.clone();
                                drop(guard);
                                println!("Lobby still running");
                                let mut content = String::new();
                                for player in lobby.lobby.players.iter() {
                                    content.push_str(&format!("{:?}\n", player.name));
                                }

                                if let Err(why) = command
                                    .edit_original_interaction_response(&ctx.http, |response| {
                                        response.embed(|embed| {
                                            embed.title(format!("{}", lobby_id)).url(format!("https://aoe2.net/j/{}", game_id)).description(content);
                                            embed
                                        })
                                    })
                                    .await
                                {
                                    println!("Cannot respond to slash command: {}", why);
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
