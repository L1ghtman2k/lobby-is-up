pub mod error;
pub mod model;

use crate::lobby_cache::model::{
    Lobby, WebsocketMessageReceiveAllCurrentLobbies, WebsocketMessageReceiveFollowUp,
};
use dashmap::DashMap;
use once_cell::sync::Lazy;
use std::collections::HashMap;

use std::str::FromStr;

use serenity::futures::StreamExt;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex as StdMutex, Once};
use std::time;

use tokio::sync::broadcast;

use tokio::sync::{mpsc, Mutex as TokioMutex};
use tokio::time::{sleep, Duration};

use tokio_tungstenite::connect_async;

use tokio_tungstenite::tungstenite::error::UrlError;
use tokio_tungstenite::tungstenite::handshake::client::{generate_key, Request};
use tokio_tungstenite::tungstenite::http::header::USER_AGENT;
use tokio_tungstenite::tungstenite::http::Uri;
use tokio_tungstenite::tungstenite::{http, Error, Message};
use tracing::{debug, error, info, warn};

// This wrapper is what you'll use to interact with the singleton
pub struct LobbyCacheOnce {
    // Since we are using lazy_static, we need to wrap it in a Mutex and an Option
    // Mutex ensures that the Singleton can be safely used across threads
    // Option lets us take it in init() and then replace it with None
    inner: StdMutex<Option<Arc<LobbyCache>>>,
    once: Once,
}

static AOE2LOBBY_URL: Lazy<Uri> =
    Lazy::new(|| Uri::from_str("wss://aoe2lobby.com/ws/lobby/").unwrap());
static AOE2LOBBY_ORIGIN: Lazy<http::HeaderValue> =
    Lazy::new(|| http::HeaderValue::from_str("https://aoe2lobby.com").unwrap());

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

pub struct LobbyCache {
    pub lobby_cache: Arc<DashMap<String, Lobby>>,
    pub last_update: Arc<TokioMutex<Option<time::SystemTime>>>,
    pub shutdown: Arc<TokioMutex<mpsc::Sender<()>>>,
    pub running: Arc<AtomicBool>,
    pub update_broadcast_sender: broadcast::Sender<()>,
}

impl LobbyCache {
    pub fn new() -> Self {
        let (shutdown, _) = mpsc::channel(3);
        let update_broadcast_sender = broadcast::channel(32).0;

        LobbyCache {
            last_update: Arc::new(TokioMutex::new(None)),
            lobby_cache: Arc::new(DashMap::new()),
            shutdown: Arc::new(TokioMutex::new(shutdown)),
            running: Arc::new(AtomicBool::new(false)),
            update_broadcast_sender,
        }
    }

    pub fn subscribe(&self) -> broadcast::Receiver<()> {
        self.update_broadcast_sender.subscribe()
    }

    async fn handle_lobby_update(
        &self,
        overwrite_lobbies: HashMap<String, Lobby>,
        delete_lobbies: Vec<i64>,
        reset: bool,
    ) {
        let mut queue_broadcaster = false;
        if !overwrite_lobbies.is_empty() || !delete_lobbies.is_empty() || reset {
            debug!("Sending update broadcast");
            queue_broadcaster = true;
        }

        if reset {
            self.lobby_cache.clear();
        }
        for (lobby_id, lobby) in overwrite_lobbies {
            self.lobby_cache.insert(lobby_id, lobby);
        }
        for lobby_id in delete_lobbies {
            self.lobby_cache.remove(&lobby_id.to_string());
        }

        if queue_broadcaster {
            let _ = self.update_broadcast_sender.send(());
        }

        self.last_update
            .lock()
            .await
            .replace(time::SystemTime::now());
    }

    async fn handle_message(&self, msg: Message) -> error::Result<()> {
        if msg.is_text() || msg.is_binary() {
            match serde_json::from_str::<WebsocketMessageReceiveAllCurrentLobbies>(
                msg.to_string().as_str(),
            ) {
                Ok(all_current_lobbies) => {
                    self.handle_lobby_update(all_current_lobbies.allcurrentlobbies, vec![], true)
                        .await;
                }
                Err(e1) => {
                    match serde_json::from_str::<WebsocketMessageReceiveFollowUp>(
                        msg.to_string().as_str(),
                    ) {
                        Ok(followup) => {
                            self.handle_lobby_update(
                                followup.updatedlobbies,
                                followup.deletedlobbies,
                                false,
                            )
                            .await;
                        }
                        Err(e2) => {
                            warn!("Received unknown message");

                            return Err(error::LobbyCacheError::Parsing(
                                error::MessageParsingError {
                                    message: msg.to_string(),
                                    error_parsing_all_messages: e1,
                                    error_parsing_followup_message: e2,
                                },
                            ));
                        }
                    }
                }
            };
        }

        Ok(())
    }

    pub async fn run(&self) {
        self.running.store(true, Ordering::SeqCst);
        let (tx, mut rx) = mpsc::channel::<()>(3);
        *self.shutdown.lock().await = tx;

        let map_ref_moved = self.lobby_cache.clone();
        let update_broadcast_sender_moved = self.update_broadcast_sender.clone();

        let last_update_moved = self.last_update.clone();

        info!("Starting websocket connection loop");
        loop {
            let ws_stream = loop {
                info!("Connecting to websocket...");

                // https://docs.rs/tungstenite/latest/src/tungstenite/client.rs.html#219
                let uri = AOE2LOBBY_URL.clone();
                let authority = uri
                    .authority()
                    .ok_or(Error::Url(UrlError::NoHostName))
                    .unwrap()
                    .as_str();
                let host = authority
                    .find('@')
                    .map(|idx| authority.split_at(idx + 1).1)
                    .unwrap_or_else(|| authority);

                let req = Request::builder()
                    .method("GET")
                    .header("Host", host)
                    .header("Connection", "Upgrade")
                    .header("Upgrade", "websocket")
                    .header("Sec-WebSocket-Version", "13")
                    .header("Sec-WebSocket-Key", generate_key())
                    .header(
                        "Sec-WebSocket-Extensions",
                        "permessage-deflate; client_max_window_bits",
                    )
                    .header("Origin", AOE2LOBBY_ORIGIN.clone())
                    .header(
                        USER_AGENT,
                        "Lobby-is-up-bot;Repo:https://github.com/L1ghtman2k/lobby-is-up",
                    )
                    .uri(uri)
                    .body(())
                    .unwrap();

                match connect_async(req).await {
                    Ok((ws_stream, _)) => {
                        break ws_stream;
                    }
                    Err(e) => {
                        error!(
                            "Error connecting to websocket, retrying in 5 seconds. Error: {:?}",
                            e
                        );

                        tokio::select! {
                            _ = rx.recv() => {
                                warn!("Shutdown received, shutting down websocket connection loop");
                                return;
                            }
                            _ = sleep(Duration::from_secs(7)) => {
                                warn!("Reconnecting to websocket after 7 seconds of waiting");
                                continue;
                            }
                        }
                    }
                }
            };
            info!("WebSocket handshake has been successfully completed");

            let (mut _write, mut read) = ws_stream.split();

            let _update_broadcast_sender_cloned = update_broadcast_sender_moved.clone();
            let _map_ref = map_ref_moved.clone();
            let _last_update_clone = last_update_moved.clone();

            debug!("Starting websocket reader");
            loop {
                tokio::select! {
                    _ = rx.recv() => {
                        warn!("Shutdown received, shutting down websocket reader");
                        return;
                    }
                    message = read.next() => {
                        match message {
                            Some(Ok(msg)) => {
                                if let Err(e) = self.handle_message(
                                    msg,
                                )
                                    .await
                                {
                                    error!("Error handling message: {:?}", e);
                                }
                            }
                            Some(Err(e)) => {
                                let e = Arc::new(e);
                                error!("Error reading message: {:?}. Reconnecting...", e);
                            }
                            None => {
                                warn!("Websocket reader channel closed. Restarting connection...");
                                break;
                            }
                        }
                    }

                    _ = sleep(Duration::from_secs(20)) => {
                        warn!("Didn't receive any messages for 20 seconds. Reconecting...");
                        break;
                    }

                }
            }

            tokio::select! {
                _ = rx.recv() => {
                    warn!("Shutdown received, shutting down websocket connection loop");
                    return;
                }
                _ = sleep(Duration::from_secs(5)) => {
                    warn!("Reconnecting to websocket after 5 seconds of waiting");
                    continue;
                }
            }
        }
    }
}
