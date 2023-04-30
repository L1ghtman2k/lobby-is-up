use serde_derive::Deserialize;
use serde_derive::Serialize;
use serde_json::Value;
use serde_with::skip_serializing_none;

pub const AOE2DE_APP_ID: i64 = 813780;
pub const AOE2DE_LOBBY_LOCATION: &str = "#aoe2de-lobbies";
pub const AOE2DE_PING_DATA: i64 = 1682837538;

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WebsocketMessageReceive {
    pub data: Vec<Lobby>,
    pub message: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[skip_serializing_none]
#[serde(rename_all = "camelCase")]
pub struct WebsocketMessageSend {
    pub message: String,
    pub subscribe: Option<Vec<i64>>,
    pub location: Option<String>,
    pub data: Option<Value>,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Lobby {
    pub id: String,
    pub match_id: i64,
    pub app_id: i64,
    pub name: String,
    pub num_players: i64,
    pub num_slots: i64,
    pub status: String,
    pub full: bool,
    pub is_lobby: bool,
    pub game_type: String,
    pub game_type_id: i64,
    pub leaderboard: String,
    pub location: String,
    pub map_size: String,
    pub ranked: bool,
    pub rating_type_id: i64,
    pub resources: Option<String>,
    pub server: String,
    pub starting_age: String,
    pub num_spectators: i64,
    pub opened: i64,
    pub players: Vec<Player>,
    pub average_rating: Option<i64>,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Player {
    pub steam_id: Option<String>,
    pub profile_id: Option<i64>,
    pub slot: i64,
    pub slot_type: i64,
    pub name: Option<String>,
    pub avatar: Option<String>,
    pub avatarfull: Option<String>,
    pub avatarmedium: Option<String>,
    pub clan: Option<String>,
    pub country_code: Option<String>,
    pub rating: Option<i64>,
}
