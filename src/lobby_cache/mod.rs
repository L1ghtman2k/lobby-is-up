pub mod error;
pub mod model;

use crate::lobby_cache::model::{
    Lobby, WebsocketMessageReceive, WebsocketMessageSend, AOE2DE_APP_ID, AOE2DE_LOBBY_LOCATION,
};
use dashmap::DashMap;
use once_cell::sync::Lazy;
use serde_json::Value;
use serenity::futures::{SinkExt, StreamExt};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex as StdMutex, Once};
use std::time;

use tokio::sync::broadcast;
use tokio::sync::mpsc::Sender;

use tokio::sync::{mpsc, oneshot, Mutex as TokioMutex};
use tokio::time::Duration;

use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::http::header::USER_AGENT;
use tokio_tungstenite::tungstenite::{http, Message};
use url::Url;

// This wrapper is what you'll use to interact with the singleton
pub struct LobbyCacheOnce {
    // Since we are using lazy_static, we need to wrap it in a Mutex and an Option
    // Mutex ensures that the Singleton can be safely used across threads
    // Option lets us take it in init() and then replace it with None
    inner: StdMutex<Option<Arc<LobbyCache>>>,
    once: Once,
}

static AOE2_URL: Lazy<Url> = Lazy::new(|| Url::parse("wss://aoe2.net/ws").unwrap());

impl LobbyCacheOnce {
    pub fn new() -> Self {
        LobbyCacheOnce {
            inner: StdMutex::new(None),
            once: Once::new(),
        }
    }

    pub fn get_instance(&self) -> Arc<LobbyCache> {
        // We only want to create the instance once
        self.once.call_once(|| {
            let singleton = LobbyCache::new(); // Replace with your data
            *self.inner.lock().unwrap() = Some(Arc::new(singleton));
        });

        // Now we know that the singleton has been created, so we can safely unwrap
        self.inner.lock().unwrap().as_ref().unwrap().clone()
    }
}

#[derive(Debug, Clone)]
pub struct StaleLobby {
    pub lobby: Lobby,
    pub last_checked_in: time::SystemTime,
}

pub struct LobbyCache {
    pub lobby_cache: Arc<DashMap<String, StaleLobby>>,
    pub shutdown: Arc<TokioMutex<oneshot::Sender<()>>>,
    pub running: Arc<AtomicBool>,
    pub update_broadcast_sender: broadcast::Sender<()>,
}

impl LobbyCache {
    pub fn new() -> Self {
        let (shutdown, _) = oneshot::channel();
        let update_broadcast_sender = broadcast::channel(32).0;

        LobbyCache {
            lobby_cache: Arc::new(DashMap::new()),
            shutdown: Arc::new(TokioMutex::new(shutdown)),
            running: Arc::new(AtomicBool::new(false)),
            update_broadcast_sender,
        }
    }

    pub fn subscribe(&self) -> broadcast::Receiver<()> {
        self.update_broadcast_sender.subscribe()
    }

    async fn handle_message(
        update_broadcast_sender: broadcast::Sender<()>,
        map_ref: Arc<DashMap<String, StaleLobby>>,
        msg: Message,
        writer_tx_clone: Sender<Message>,
    ) -> error::Result<()> {
        println!("Received message");
        if msg.is_text() || msg.is_binary() {
            let update: WebsocketMessageReceive = serde_json::from_str(msg.to_string().as_str())?;

            match update {
                WebsocketMessageReceive::Ping { data } => {
                    println!("ping");
                    let message = serde_json::to_string(&WebsocketMessageSend {
                        message: "ping".to_string(),
                        subscribe: None,
                        location: None,
                        data: Some(Value::from(data)),
                    })?;
                    writer_tx_clone.send(Message::Text(message)).await.unwrap();
                }
                WebsocketMessageReceive::Lobby { data } => {
                    println!("lobby");
                    for lobby in data {
                        if lobby.is_lobby {
                            map_ref.insert(
                                lobby.id.clone(),
                                StaleLobby {
                                    lobby,
                                    last_checked_in: time::SystemTime::now(),
                                },
                            );
                        } else if map_ref.contains_key(&lobby.id) {
                            map_ref.remove(&lobby.id);
                        }
                    }

                    map_ref.retain(|_, lobby| {
                        lobby.last_checked_in.elapsed().unwrap() < Duration::from_secs(120)
                    });
                    println!("Map contains {} lobbies", map_ref.len());
                    if update_broadcast_sender.receiver_count() > 0 {
                        println!("Sending update broadcast");
                        let _ = update_broadcast_sender.send(());
                    }
                }
            }
        }

        Ok(())
    }

    pub async fn run(&self) {
        self.running.store(true, Ordering::SeqCst);
        let (tx, rx) = oneshot::channel::<()>();
        *self.shutdown.lock().await = tx;

        let background_shutdown_tx = broadcast::channel(32).0;

        let map_ref_moved = self.lobby_cache.clone();
        let update_broadcast_sender_moved = self.update_broadcast_sender.clone();

        let background_shutdown_tx_moved = background_shutdown_tx.clone();
        let mut background_shutdown_tx_cloned = background_shutdown_tx.subscribe();
        let background_task = tokio::spawn(async move {
            // Connect to the server, and retry if the connection fails
            let mut interval = tokio::time::interval(Duration::from_secs(5));
            println!("Starting websocket connection loop");
            loop {
                let (connection_restart_tx, mut connection_restart_rx) = broadcast::channel(32);

                let ws_stream = loop {
                    println!("Connecting to websocket...");
                    http::Request::builder()
                        .uri(AOE2_URL.as_str())
                        .header(
                            USER_AGENT,
                            "Lobby-is-up-bot;Repo:https://github.com/L1ghtman2k/lobby-is-up",
                        )
                        .body(())
                        .unwrap();

                    match connect_async(AOE2_URL.clone()).await {
                        Ok((ws_stream, _)) => {
                            break ws_stream;
                        }
                        Err(_) => {
                            println!("Error connecting to websocket, retrying in 5 seconds");
                            interval.tick().await;
                        }
                    }
                };
                println!("WebSocket handshake has been successfully completed");

                let (mut write, mut read) = ws_stream.split();

                let (writer_tx, mut writer_rx) = mpsc::channel(32);

                println!("Starting websocket writer");
                // Spawn a task to write messages from the channel to the WebSocket
                let connection_restart_tx_clone = connection_restart_tx.clone();
                let mut shutdown_broadcast_sender_writer = background_shutdown_tx_moved.subscribe();
                let mut connection_restart_rx_writer = connection_restart_tx.subscribe();
                let _write_task = tokio::spawn(async move {
                    loop {
                        tokio::select! {
                            _ = shutdown_broadcast_sender_writer.recv() => {
                                println!("Shutdown received, shutting down websocket writer");
                                break;
                            }
                            _ = connection_restart_rx_writer.recv() => {
                                println!("Restart received, shutting down websocket writer");
                                break;
                            }
                            msg = writer_rx.recv() => {
                                match msg {
                                    Some(msg) => {
                                        println!("Sending message: {:?}", msg);
                                        if let Err(e) = write.send(msg).await {
                                            let e = Arc::new(e);
                                            println!("Error sending message: {:?}", e);
                                            connection_restart_tx_clone.send(e).unwrap();
                                            break;
                                        }
                                    }
                                    None => {
                                        println!("Websocket writer channel closed");
                                        break;
                                    }
                                }

                            }
                        }
                    }
                });

                let initial_messages: Vec<WebsocketMessageSend> = vec![
                    WebsocketMessageSend {
                        message: "subscribe".to_string(),
                        subscribe: Some(vec![0]),
                        location: None,
                        data: None,
                    },
                    WebsocketMessageSend {
                        message: "location".to_string(),
                        subscribe: None,
                        location: Some(AOE2DE_LOBBY_LOCATION.to_string()),
                        data: None,
                    },
                    WebsocketMessageSend {
                        message: "subscribe".to_string(),
                        subscribe: Some(vec![AOE2DE_APP_ID]),
                        location: None,
                        data: None,
                    },
                ];
                for message in initial_messages {
                    let message = serde_json::to_string(&message).unwrap();
                    writer_tx.send(Message::Text(message)).await.unwrap();
                }

                // Spawn a task to read messages from the channel to the WebSocket
                let writer_tx_clone = writer_tx.clone();
                let connection_restart_tx_clone = connection_restart_tx.clone();

                let update_broadcast_sender_cloned = update_broadcast_sender_moved.clone();
                let map_ref = map_ref_moved.clone();
                let mut connection_restart_rx_reader = connection_restart_tx.subscribe();
                let mut shutdown_broadcast_sender_reader = background_shutdown_tx_moved.subscribe();
                let _read_task = tokio::spawn(async move {
                    println!("Starting websocket reader");
                    loop {
                        tokio::select! {
                            _ = shutdown_broadcast_sender_reader.recv() => {
                                println!("Shutdown received, shutting down websocket reader");
                                break;
                            }
                            _ = connection_restart_rx_reader.recv() => {
                                println!("Restart received, shutting down websocket reader");
                                break;
                            }
                            message = read.next() => {
                                match message {
                                    Some(Ok(msg)) => {
                                        if let Err(e) = Self::handle_message(
                                            update_broadcast_sender_cloned.clone(),
                                            map_ref.clone(),
                                            msg,
                                            writer_tx_clone.clone(),
                                        )
                                            .await
                                        {
                                            println!("Error handling message: {:?}", e);
                                        }
                                    }
                                    Some(Err(e)) => {
                                        let e = Arc::new(e);
                                        println!("Error reading message: {:?}", e);
                                        connection_restart_tx_clone.send(e.clone()).unwrap();
                                    }
                                    None => {
                                        println!("Websocket reader channel closed. Restarting connection");
                                        connection_restart_tx_clone.send(Arc::new(tokio_tungstenite::tungstenite::error::Error::ConnectionClosed)).unwrap();
                                        break;
                                    }
                                }
                            }
                        }
                    }
                });

                tokio::select! {
                    err = connection_restart_rx.recv() => {
                        println!("Connection error: {:?}", err);
                        futures::future::join_all(vec![
                            _write_task,
                            _read_task,
                        ]).await;
                        println!("Restarting connection");
                    },
                    _ = background_shutdown_tx_cloned.recv() => {
                        writer_tx.send(Message::Close(None)).await.unwrap();
                        println!("Closing websocket");
                        break
                    }
                }
                interval.tick().await;
            }
        });
        rx.await.unwrap();
        background_shutdown_tx.send(()).unwrap();
        background_task.await.unwrap();
    }
}
