pub mod model;

use crate::lobby_cache::model::{
    Lobby, WebsocketMessageReceive, WebsocketMessageSend, AOE2DE_APP_ID, AOE2DE_LOBBY_LOCATION,
};
use dashmap::DashMap;
use serde_json::Value;
use serenity::futures::{SinkExt, StreamExt};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex as StdMutex, Once};
use std::time;

use tokio::sync::broadcast;
use tokio::sync::{mpsc, oneshot, Mutex as TokioMutex};
use tokio::time::Duration;
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::Message;
use url::Url;

// This wrapper is what you'll use to interact with the singleton
pub struct LobbyCacheOnce {
    // Since we are using lazy_static, we need to wrap it in a Mutex and an Option
    // Mutex ensures that the Singleton can be safely used across threads
    // Option lets us take it in init() and then replace it with None
    inner: StdMutex<Option<Arc<LobbyCache>>>,
    once: Once,
}

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
    pub shutdown: TokioMutex<oneshot::Sender<()>>,
    pub running: Arc<AtomicBool>,
    pub update_broadcast_sender: broadcast::Sender<()>,
}

impl LobbyCache {
    pub fn new() -> Self {
        let (shutdown, _) = oneshot::channel();

        let update_broadcast_sender = broadcast::channel(32).0;

        LobbyCache {
            lobby_cache: Arc::new(DashMap::new()),
            shutdown: TokioMutex::new(shutdown),
            running: Arc::new(AtomicBool::new(false)),
            update_broadcast_sender,
        }
    }

    pub fn subscribe(&self) -> broadcast::Receiver<()> {
        self.update_broadcast_sender.subscribe()
    }

    pub async fn run(&self) {
        self.running.store(true, Ordering::SeqCst);
        let (tx, rx) = oneshot::channel::<()>();
        *self.shutdown.lock().await = tx;

        let map_ref = Arc::clone(&self.lobby_cache);

        let url = Url::parse("wss://aoe2.net/ws").unwrap();

        let (ws_stream, _) = connect_async(url).await.expect("Failed to connect"); // Todo: handle error, retry connection
        println!("WebSocket handshake has been successfully completed");

        let (mut write, read) = ws_stream.split();

        let (writer_tx, mut writer_rx) = mpsc::channel(32);

        // Clone the sender for use elsewhere in your code
        let writer_tx_clone = writer_tx.clone();

        println!("Starting websocket writer");
        // Spawn a task to send messages from the channel to the WebSocket
        let _send_task = tokio::spawn(async move {
            while let Some(msg) = writer_rx.recv().await {
                println!("Sending message: {:?}", msg);
                write.send(msg).await.unwrap();
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

        println!("Starting websocket reader");
        let read = read.for_each(|message| async {
            match message {
                Ok(msg) => {
                    println!("Received message");
                    if msg.is_text() || msg.is_binary() {
                        let update: WebsocketMessageReceive =
                            serde_json::from_str(msg.to_string().as_str()).unwrap();

                        println!("is text or binary");
                        match update {
                            WebsocketMessageReceive::Ping { data } => {
                                println!("ping");
                                let message = serde_json::to_string(&WebsocketMessageSend {
                                    message: "ping".to_string(),
                                    subscribe: None,
                                    location: None,
                                    data: Some(Value::from(data)),
                                })
                                .unwrap();
                                writer_tx_clone.send(Message::Text(message)).await.unwrap();
                            }
                            WebsocketMessageReceive::Lobby { data } => {
                                println!("lobby");
                                for lobby in data {
                                    if lobby.is_lobby {
                                        println!("deadlock on insert");
                                        map_ref.insert(
                                            lobby.id.clone(),
                                            StaleLobby {
                                                lobby,
                                                last_checked_in: time::SystemTime::now(),
                                            },
                                        );
                                        println!("deadlock on insert done");
                                    } else if map_ref.contains_key(&lobby.id) {
                                        println!("deadlock on remove");
                                        map_ref.remove(&lobby.id);
                                        println!("deadlock on remove done");
                                    }
                                }

                                map_ref.retain(|_, lobby| {
                                    lobby.last_checked_in.elapsed().unwrap()
                                        < Duration::from_secs(120)
                                });
                                println!("Map contains {} lobbies", map_ref.len());
                                if self.update_broadcast_sender.receiver_count() > 0 {
                                    println!("Sending update broadcast");
                                    let _ = self.update_broadcast_sender.send(());
                                }
                            }
                        }
                    }
                }
                Err(e) => {
                    println!("Error reading message: {:?}", e);
                }
            }
        });

        tokio::select! {
            _ = read => {
                println!("Websocket reader closed");
            },
            _ = rx => {
                println!("Shutdown signal received");
            }
        }

        writer_tx.send(Message::Close(None)).await.unwrap();
        println!("Closing websocket");
    }
}
