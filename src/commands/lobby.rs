use crate::lobby_cache::LobbyCache;
use std::collections::HashMap;

use crate::commands::util::create_interaction_response;
use crate::lobby_cache::model::Lobby;

use once_cell::sync::Lazy;
use regex::Regex;
use serenity::builder::{CreateApplicationCommand, CreateEmbed};
use serenity::client::Context;
use serenity::model::application::interaction::application_command::{
    ApplicationCommandInteraction, CommandDataOption,
};
use serenity::model::application::interaction::InteractionResponseType;
use serenity::model::prelude::command::CommandOptionType;

use scopeguard::defer;
use serenity::utils::{Color, Colour};
use std::sync::Arc;

use crate::commands::error;
use tokio::sync::mpsc::{Receiver, Sender};
use tokio::sync::Mutex;
use tokio::time::sleep;
use tokio::time::Duration;
use tracing::error;
use tracing::log::debug;
use uuid::Uuid;

static RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"^aoe2de://0/\d+$").unwrap());

type ChannelQueue = Arc<Mutex<HashMap<String, Vec<(Uuid, Arc<Sender<()>>)>>>>;

pub struct LobbyHandler {
    lobby_cache: Arc<LobbyCache>,
    channel_queue: ChannelQueue,
}

impl LobbyHandler {
    pub fn new(lobby_cache: Arc<LobbyCache>) -> Self {
        Self {
            lobby_cache,
            channel_queue: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    async fn register_lobby_id_channel(
        &self,
        game_id: &str,
    ) -> error::Result<(Uuid, Receiver<()>)> {
        let mut channel_queue = self.channel_queue.lock().await;
        if channel_queue.len() > 5 {
            return Err(error::CommandError::TooManyLobbies);
        }

        let queue = channel_queue
            .entry(game_id.to_string())
            .or_insert(Vec::new());
        if queue.len() > 3 {
            let (_, sender) = queue.pop().unwrap();
            sender.send(()).await.unwrap();
        }

        let (sender, receiver) = tokio::sync::mpsc::channel(3);

        let uuid = Uuid::new_v4();

        queue.insert(0, (uuid, Arc::new(sender)));

        Ok((uuid, receiver))
    }

    async fn unregister(channel_queue: ChannelQueue, game_id: &str, uuid: Uuid) {
        let mut channel_queue = channel_queue.lock().await;
        let queue = channel_queue.get_mut(game_id).unwrap();
        queue.retain(|(id, _)| id != &uuid);
        if queue.is_empty() {
            channel_queue.remove(game_id);
        }
    }

    fn get_lobby(&self, game_id: &str) -> Option<Lobby> {
        return self
            .lobby_cache
            .lobby_cache
            .get(game_id)
            .map(|lobby_ref| lobby_ref.clone());
    }

    pub async fn run(&self, ctx: &Context, command: ApplicationCommandInteraction) {
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
                        "aoe2lobby.com hasn't replied in over a minute. Try again later..."
                            .to_string(),
                    )
                    .await;
                    return;
                }
            }

            let game_id = lobby_id.clone();
            let game_id = game_id.split('/').last().unwrap();

            let (uuid, mut cancel_receiver) = match self.register_lobby_id_channel(game_id).await {
                Ok(receiver) => receiver,
                Err(error) => {
                    create_interaction_response(ctx, command, format!("{}", error)).await;
                    return;
                }
            };

            let channel_queue_clone = self.channel_queue.clone();
            let game_id_clone = game_id.to_string();
            let uuid_clone = uuid;

            defer! {
                tokio::spawn(async move {
                    Self::unregister(channel_queue_clone, game_id_clone.as_str(), uuid_clone).await
                });
            }

            // Discord allows for up to 15 minutes for a response
            let deadline = tokio::time::Instant::now() + Duration::from_secs(14 * 60);

            if let Err(why) = command
                .create_interaction_response(&ctx.http, |response| {
                    response
                        .kind(InteractionResponseType::ChannelMessageWithSource)
                        .interaction_response_data(|message| {
                            message.embed(|embed| {
                                embed
                                    .title(format!("{}", lobby_id))
                                    .url(format!("https://aoe2lobby.com/j/{}", game_id));
                                embed
                            })
                        })
                })
                .await
            {
                error!("Cannot respond to slash command: {:?}", why);

                return;
            }

            let mut result: Option<Lobby> = None;

            for _ in 0..10 {
                if let Some(value) = self.get_lobby(game_id) {
                    debug!("Aoe2 Registered lobby with id: {}", value.lobbyid);
                    result = Some(value);
                    break;
                } else {
                    debug!("Retrying...");
                    sleep(Duration::from_secs(3)).await; // optional delay
                }
            }

            match result {
                Some(lobby) => {
                    let mut state = extract_state(&lobby);
                    if let Err(why) = command
                        .edit_original_interaction_response(&ctx.http, |response| {
                            response.set_embed(create_embed(&state))
                        })
                        .await
                    {
                        error!("Cannot respond to slash command: {:?}", why);
                        return;
                    }

                    let mut update_receiver = self.lobby_cache.subscribe();
                    loop {
                        debug!("Inside of updater loop");
                        tokio::select! {
                            _ = cancel_receiver.recv() => {
                                debug!("Received cancel");
                                let mut last_embed = create_embed(&state);
                                {
                                    last_embed.footer(|footer| footer.text("Message no longer updated live"));
                                }

                                if let Err(why) = command
                                    .edit_original_interaction_response(&ctx.http, |response| {
                                        response.set_embed(last_embed)
                                    })
                                    .await
                                {
                                    error!("Cannot respond to slash command: {:?}", why);
                                    return;
                                }
                                break;
                            }
                            _ = tokio::time::sleep_until(deadline) => {
                                let mut last_embed = create_embed(&state);
                                {
                                    last_embed.footer(|footer| footer.text("Message no longer updated live, as it was up for over 15 minutes"));

                                }
                                if let Err(why) = command
                                    .edit_original_interaction_response(&ctx.http, |response| {
                                        response.set_embed(last_embed)
                                    })
                                    .await
                                {
                                    error!("Cannot respond to slash command: {:?}", why);
                                    return;
                                }
                                break;
                            }
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
                                                .url(format!("https://aoe2lobby.com/j/{}", game_id))
                                                .description("Lobby no longer active");
                                            embed
                                        })
                                    })
                                    .await
                                {
                                    error!("Cannot respond to slash command: {:?}", why);
                                    break;
                                }

                                break;
                            }

                            Some(lobby) => {
                                let _new_state = extract_state(&lobby);
                                debug!("Lobby still running");
                                let new_state = extract_state(&lobby);
                                if new_state == state {
                                    debug!("No change in state");
                                    continue;
                                } else {
                                    debug!("Change in players");
                                    state = new_state;
                                }
                                if let Err(why) = command
                                    .edit_original_interaction_response(&ctx.http, |response| {
                                        response.set_embed(create_embed(&state))
                                    })
                                    .await
                                {
                                    error!("Cannot respond to slash command: {:?}", why);
                                    break;
                                }
                            }
                        }
                    }
                }
                None => {
                    if let Err(why) = command
                        .edit_original_interaction_response(&ctx.http, |response| {
                            response.embed(|embed| {
                                embed
                                    .title(format!("{}", lobby_id))
                                    .url(format!("https://aoe2lobby.com/j/{}", game_id))
                                    .description("aoe2lobby.com hasn't picked up this lobby after 30 seconds.\nPlayer data will be unavailable.");
                                embed
                            })
                        })
                        .await
                    {
                        error!("Cannot respond to slash command: {:?}", why);
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

#[derive(Debug, Clone, PartialEq, Eq)]
struct State {
    players: String,
    color: Color,
    slots_taken: i64,
    slots_total: i64,
    id: String,
}

fn extract_state(lobby: &Lobby) -> State {
    State {
        players: format_players(lobby),
        color: extract_colors(lobby),
        slots_taken: lobby.slotstaken,
        slots_total: lobby.slotstotal,
        id: lobby.lobbyid.clone().to_string(),
    }
}

fn create_embed(state: &State) -> CreateEmbed {
    let mut embed = CreateEmbed::default();
    let remaining_slots = state.slots_total - state.slots_taken;
    let remaining_slots = if remaining_slots > 0 {
        format!("+{}", remaining_slots)
    } else {
        "Lobby is full".to_string()
    };
    embed
        .title(format!("Lobby is up! aoe2de://0/{}", state.id))
        .url(format!("https://aoe2lobby.com/j/{}", state.id))
        .color(state.color)
        .footer(|footer| footer.text(remaining_slots))
        .description(state.players.clone());
    embed
}

// fn create_components(state: &State) -> CreateComponents{
//     let mut components = CreateComponents::default();
//     components.set_action_row(create_action_row(state));
//     components
// }
//
// fn create_action_row(state: &State) -> CreateActionRow{
//     let mut row = CreateActionRow::default();
//     row.create_button(|button| {
//         button
//             .style(ButtonStyle::Success)
//             .label("Join")
//             .url(format!("https://aoe2lobby.com/j/{}", state.id))
//     });
//     row
// }

fn extract_colors(lobby: &Lobby) -> Color {
    if lobby.slotstotal - lobby.slotstaken > 0 {
        Colour::DARK_GREEN
    } else {
        Color::RED
    }
}

fn format_players(lobby: &Lobby) -> String {
    let mut content = String::new();
    let mut players = vec![];
    for player in lobby.slot.values() {
        if let Some(name) = &player.name {
            if name != "Open" && name != "Closed" {
                players.push(name.clone());
            } else if name.is_empty() {
                players.push("Unknown".to_string());
            }
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
