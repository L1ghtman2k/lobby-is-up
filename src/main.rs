mod commands;
mod lobby_cache;

use std::env;
use std::process::exit;
use std::sync::atomic::Ordering;
use tokio::signal;
use serenity::async_trait;

use serenity::model::application::interaction::{Interaction, InteractionResponseType};
use serenity::model::gateway::Ready;
use serenity::model::id::GuildId;
use serenity::prelude::*;
use tokio::sync::mpsc;

struct Handler;

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
            commands.create_application_command(|command| commands::lobby::register(command))
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

            let content = match command.data.name.as_str() {
                "lobby" => commands::lobby::run(&command.data.options),
                _ => "not implemented :(".to_string(),
            };

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
    }
}

#[tokio::main]
async fn main() {

    let (shutdown_send, shutdown_recv) = mpsc::unbounded_channel();

    // Get lobby cache singleton
    let lobby_cache = lobby_cache::LobbyCacheOnce::new();
    let lobby_cache = lobby_cache.get_instance();

    // Start the lobby cache background task
    let _ = tokio::spawn(async move {
        lobby_cache.run().await;
        shutdown_send.send(()).unwrap();
    });


    // Configure the client with your Discord bot token in the environment.
    let token = env::var("DISCORD_TOKEN").expect("Expected a token in the environment");

    // Build our client.
    let mut client = Client::builder(token, GatewayIntents::empty())
        .event_handler(Handler)
        .await
        .expect("Error creating client");

    let shard_manager = client.shard_manager.clone();
    // Finally, start a single shard, and start listening to events.
    //
    // Shards will automatically attempt to reconnect, and will perform
    // exponential backoff until it reconnects.
    let _ = tokio::spawn(async move {
        if let Err(why) = client.start().await {
            eprintln!("Client error: {:?}", why);
        }
        shutdown_send.send(()).unwrap();
    });

    tokio::select! {
        _ = signal::ctrl_c() => {},
        _ = shutdown_recv.recv() => {},
    }

    println!("Received shutdown signal");
    if lobby_cache.running.load(Ordering::SeqCst) {
        lobby_cache.shutdown.lock().await.send(()).unwrap();
    }
    shard_manager.lock().await.shutdown_all();
}
