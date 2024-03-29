mod commands;
mod lobby_cache;

use std::env;
use std::env::VarError;
use std::sync::Arc;

use futures::future::join_all;
use serenity::async_trait;
use serenity::model::application::command::Command;
use std::sync::atomic::Ordering;

use tokio::signal;

use crate::commands::lobby::LobbyHandler;
use crate::commands::util::create_interaction_response;
use crate::lobby_cache::LobbyCache;
use serenity::model::application::interaction::Interaction;
use serenity::model::gateway::Ready;
use serenity::model::id::GuildId;

use serenity::prelude::*;
use tokio::sync::mpsc;
use tracing::log::debug;
use tracing::subscriber::set_global_default;
use tracing::{error, info, warn};
use tracing_bunyan_formatter::{BunyanFormattingLayer, JsonStorageLayer};
use tracing_log::LogTracer;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::{EnvFilter, Registry};

struct Handler {
    lobby_handler: Arc<LobbyHandler>,
    guild_ids: Vec<GuildId>,
}

impl Handler {
    pub fn new(lobby_cache: Arc<LobbyCache>) -> Self {
        // extract guild ids, and if it is set(comma separated list of integers), verify it is an integer
        let guild_ids: Vec<GuildId> = match env::var("GUILD_IDS") {
            Ok(guild_ids) => guild_ids
                .split(',')
                .map(|guild_id| {
                    GuildId(
                        guild_id
                            .parse()
                            .expect("GUILD_IDS must be an vec of integers"),
                    )
                })
                .collect(),
            Err(VarError::NotPresent) => vec![],
            Err(VarError::NotUnicode(_)) => {
                panic!("GUILD_IDS must be a comma separated list of integers")
            }
        };

        Self {
            lobby_handler: Arc::new(LobbyHandler::new(lobby_cache)),
            guild_ids,
        }
    }
}

#[async_trait]
impl EventHandler for Handler {
    async fn ready(&self, ctx: Context, ready: Ready) {
        info!("{} is connected!", ready.user.name);

        if self.guild_ids.is_empty() {
            info!("Running in global mode");
            let _commands = Command::set_global_application_commands(&ctx.http, |commands| {
                commands.create_application_command(|command| LobbyHandler::register(command))
            }).await.expect("Failed to register application commands");
        } else {
            info!("Running in guild mode");
            for guild_id in &self.guild_ids {
                let _commands =
                    GuildId::set_application_commands(guild_id, &ctx.http, |commands| {
                        commands
                            .create_application_command(|command| LobbyHandler::register(command))
                    })
                    .await
                    .expect("Failed to register application commands");
            }
        }
    }

    async fn interaction_create(&self, ctx: Context, interaction: Interaction) {
        if let Interaction::ApplicationCommand(command) = interaction {
            debug!("Received command interaction: {:#?}", command);

            //Verify that the interaction is a command is coming for expected guild
            if !self.guild_ids.is_empty() {
                match command.guild_id {
                    Some(guild_id) => {
                        if !self.guild_ids.contains(&guild_id) {
                            warn!(
                                "Received command interaction from unexpected guild: {}",
                                guild_id
                            );
                            return;
                        }
                    }
                    None => {
                        warn!("Received command interaction without guild id");
                        return;
                    }
                }
            }
            match command.data.name.as_str() {
                "lobby" => {
                    self.lobby_handler.run(&ctx, command).await;
                }
                _ => {
                    create_interaction_response(&ctx, command, "not implemented :(".to_string())
                        .await;
                }
            };
        }
    }
}

#[tokio::main]
async fn main() {
    let mut filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    let formatting_layer = BunyanFormattingLayer::new("lobby_is_up".to_string(), std::io::stdout);
    for (log_directive, directive_level) in [
        ("lobby_is_up".to_string(), "info".to_string()),
        ("serenity".to_string(), "warn".to_string()),
        ("_".to_string(), "error".to_string()),
    ] {
        filter = filter.add_directive(
            format!("{}={}", log_directive, directive_level)
                .parse()
                .unwrap(),
        );
    }
    let subscriber = Registry::default()
        .with(filter)
        .with(JsonStorageLayer)
        .with(formatting_layer);

    match LogTracer::init() {
        Ok(_) => {
            if let Err(e) = set_global_default(subscriber) {
                panic!("Failed to set global subscriber: {}", e);
            }
        }
        Err(e) => {
            panic!("Failed to set subscriber: {}", e);
        }
    }

    let (shutdown_send, mut shutdown_recv) = mpsc::unbounded_channel();

    // Get lobby cache singleton
    let lobby_cache = lobby_cache::LobbyCacheOnce::new();
    let lobby_cache = lobby_cache.get_instance();

    let shutdown_lobby_cache_clone = shutdown_send.clone();
    let lobby_cache_shared = lobby_cache.clone();

    // Start the lobby cache background task
    let _lobby_cache_task = tokio::spawn(async move {
        lobby_cache_shared.run().await;
        warn!("Lobby cache shutdown");
        shutdown_lobby_cache_clone.send(()).unwrap();
    });

    // Configure the client with your Discord bot token in the environment.
    let token = env::var("DISCORD_TOKEN").expect("Expected a token in the environment");

    // Build our client.
    let mut client = Client::builder(token, GatewayIntents::empty())
        .event_handler(Handler::new(lobby_cache.clone()))
        .await
        .expect("Error creating client");

    let shard_manager = client.shard_manager.clone();
    // Finally, start a single shard, and start listening to events.
    //
    // Shards will automatically attempt to reconnect, and will perform
    // exponential backoff until it reconnects.

    let shutdown_discord_client_clone = shutdown_send.clone();

    let _discord_task = tokio::spawn(async move {
        if let Err(why) = client.start().await {
            error!("Client error: {:?}", why);
        }
        warn!("Discord client shutdown");
        shutdown_discord_client_clone.send(()).unwrap();
    });

    tokio::select! {
        _ = signal::ctrl_c() => {

        },
        _ = shutdown_recv.recv() => {

        },
    }

    warn!("Received shutdown signal");
    if lobby_cache.running.load(Ordering::SeqCst) {
        let shutdown_signal = lobby_cache.shutdown.lock().await;
        shutdown_signal.send(()).await.unwrap();
    }
    shard_manager.lock().await.shutdown_all().await;

    join_all(vec![_lobby_cache_task, _discord_task]).await;
}
