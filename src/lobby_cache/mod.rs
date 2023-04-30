mod model;

use std::collections::HashMap;
use std::sync::{Arc, Once};
use std::sync::atomic::{AtomicBool, Ordering};
use dashmap::DashMap;
use serde_json::Value;
use serenity::futures::{SinkExt, StreamExt};
use tokio::sync::{oneshot, Mutex};
use tokio::time::{Duration, sleep};
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::Message;
use url::Url;
use crate::lobby_cache::model::{AOE2DE_APP_ID, AOE2DE_LOBBY_LOCATION, AOE2DE_PING_DATA, Lobby, WebsocketMessageReceive, WebsocketMessageSend};

// This wrapper is what you'll use to interact with the singleton
pub struct LobbyCacheOnce {
    // Since we are using lazy_static, we need to wrap it in a Mutex and an Option
    // Mutex ensures that the Singleton can be safely used across threads
    // Option lets us take it in init() and then replace it with None
    inner: Mutex<Option<Arc<LobbyCache>>>,
    once: Once,
}

impl LobbyCacheOnce {
    pub fn new() -> Self {
        LobbyCacheOnce {
            inner: Mutex::new(None),
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

struct LobbyCache {
    pub lobby_cache: Arc<DashMap<String, Lobby>>,
    pub shutdown: Mutex<oneshot::Sender<()>>,
    pub running: Arc<AtomicBool>,
}

impl LobbyCache {
    pub fn new() -> Self {
        let (tx, _) = oneshot::channel();

        LobbyCache {
            lobby_cache: Arc::new(DashMap::new()),
            shutdown: Mutex::new(tx),
            running: Arc::new(AtomicBool::new(false)),
        }
    }

    pub async fn run(&self) {
        self.running.store(true, Ordering::SeqCst);
        let (tx, mut rx) = oneshot::channel::<()>();
        *self.shutdown.lock().await = tx;

        let map_ref = Arc::clone(&self.lobby_cache);

        let background_task = tokio::spawn(async move {
            let url = Url::parse("wss://aoe2.net/ws").unwrap();

            let (ws_stream, _) = connect_async(url).await.expect("Failed to connect"); // Todo: handle error, retry connection
            println!("WebSocket handshake has been successfully completed");

            let (mut write, read) = ws_stream.split();
            const INITIAL_MESSAGES:Vec<WebsocketMessageSend> = vec![
                WebsocketMessageSend{
                    message: "subscribe".to_string(),
                    subscribe: Some(vec![0]),
                    location: None,
                    data: None,
                },
                WebsocketMessageSend{
                    message: "location".to_string(),
                    subscribe: None,
                    location: Some(AOE2DE_LOBBY_LOCATION.to_string()),
                    data: None,
                },
                WebsocketMessageSend{
                    message: "subscribe".to_string(),
                    subscribe: Some(vec![AOE2DE_APP_ID]),
                    location: None,
                    data: None,
                },
            ];
            for message in INITIAL_MESSAGES {
                let message = serde_json::to_string(&message).unwrap();
                write.send(Message::Text(message)).await.unwrap();
            }

            // Periodic pining background task
            let ping = tokio::spawn(async {
                loop {
                    sleep(Duration::from_secs(30)).await;
                    let message = serde_json::to_string(&WebsocketMessageSend{
                        message: "ping".to_string(),
                        subscribe: None,
                        location: None,
                        data: Some(Value::from(AOE2DE_PING_DATA)),
                    }).unwrap();
                    write.send(Message::Text(message)).await.unwrap();
                }
            });

            let read = read.for_each(|message| async {
                match message {
                    Ok(msg) => {
                        if msg.is_text() || msg.is_binary() {
                            let update: WebsocketMessageReceive = serde_json::from_str(msg.to_string().as_str()).unwrap();
                            if !update.data.is_empty(){
                                map_ref.clear();
                            }
                            for lobby in update.data {
                                map_ref.insert(lobby.id.clone(), lobby);
                            }
                        }
                    },
                    Err(e) => {
                        println!("Error reading message: {:?}", e);
                    }
                }
            });

                tokio::select! {
                    _ = read => {},
                    _ = write.send(Message::Close(None)) => {},
                }
        });

        // The background task will run until it receives the shutdown signal
        background_task.await.unwrap();
    }

}
