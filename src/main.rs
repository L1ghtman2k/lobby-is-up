mod commands;
mod lobby_cache;

use std::sync::Arc;
use std::{env, mem};

use serenity::async_trait;
use std::sync::atomic::Ordering;
use tokio::signal;

use crate::commands::lobby::LobbyHandler;
use crate::commands::util::create_interaction_response;
use crate::lobby_cache::LobbyCache;
use serenity::model::application::interaction::Interaction;
use serenity::model::gateway::Ready;
use serenity::model::id::GuildId;
use serenity::prelude::*;
use tokio::sync::{mpsc, oneshot};

struct Handler {
    lobby_cache: Arc<LobbyCache>,
}

impl Handler {
    pub fn new(lobby_cache: Arc<LobbyCache>) -> Self {
        Self { lobby_cache }
    }
}

#[async_trait]
impl EventHandler for Handler {
    async fn ready(&self, ctx: Context, ready: Ready) {
        println!("{} is connected!", ready.user.name);

        let guild_id = GuildId(
            env::var("GUILD_ID")
                .expect("Expected GUILD_ID in environment")
                .parse()
                .expect("GUILD_ID must be an integer"),
        );

        let commands = GuildId::set_application_commands(&guild_id, &ctx.http, |commands| {
            commands.create_application_command(|command| LobbyHandler::register(command))
        })
        .await;

        println!(
            "I now have the following guild slash commands: {:#?}",
            commands
        );
    }

    async fn interaction_create(&self, ctx: Context, interaction: Interaction) {
        if let Interaction::ApplicationCommand(command) = interaction {
            println!("Received command interaction: {:#?}", command);

            match command.data.name.as_str() {
                "lobby" => {
                    LobbyHandler::new(self.lobby_cache.clone())
                        .run(ctx, command)
                        .await
                }
                _ => {
                    create_interaction_response(ctx, command, "not implemented :(".to_string())
                        .await;
                }
            };
        }
    }
}

#[tokio::main]
async fn main() {
    let (shutdown_send, mut shutdown_recv) = mpsc::unbounded_channel();

    // Get lobby cache singleton
    let lobby_cache = lobby_cache::LobbyCacheOnce::new();
    let lobby_cache = lobby_cache.get_instance();

    let shutdown_lobby_cache_clone = shutdown_send.clone();
    let lobby_cache_shared = lobby_cache.clone();

    // Start the lobby cache background task
    let _lobby_cache_task = tokio::spawn(async move {
        lobby_cache_shared.run().await;
        println!("Lobby cache shutdown");
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
            eprintln!("Client error: {:?}", why);
        }
        println!("Discord client shutdown");
        shutdown_discord_client_clone.send(()).unwrap();
    });

    tokio::select! {
        _ = signal::ctrl_c() => {},
        _ = shutdown_recv.recv() => {},
    }

    println!("Received shutdown signal");
    if lobby_cache.running.load(Ordering::SeqCst) {
        let mut shutdown_signal = lobby_cache.shutdown.lock().await;

        // Take the Sender out of the mutex, replacing it with a new Sender
        let (new_tx, _new_rx) = oneshot::channel::<()>();
        let shutdown_signal = mem::replace(&mut *shutdown_signal, new_tx);

        // Now you can use the old sender
        let _ = shutdown_signal.send(());
    }
    shard_manager.lock().await.shutdown_all().await;
}
