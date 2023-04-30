use serde_derive::Deserialize;
use serde_derive::Serialize;
use serde_json::Value;

pub const AOE2DE_APP_ID: i64 = 813780;
pub const AOE2DE_LOBBY_LOCATION: &str = "#aoe2de-lobbies";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "message")]
pub enum WebsocketMessageReceive {
    #[serde(rename = "ping")]
    Ping {
        data: i64,
    },
    #[serde(rename = "lobbies")]
    Lobby {
        data: Vec<Lobby>,
    },
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WebsocketMessageSend {
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subscribe: Option<Vec<i64>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub location: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
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
    pub cheats: Option<bool>,
    pub full_tech_tree: Option<bool>,
    pub game_type: Option<String>,
    pub game_type_id: Option<i64>,
    pub leaderboard: Option<String>,
    pub full: bool,
    pub is_lobby: bool,
    pub location: String,
    pub resources: Option<String>,
    pub average_rating: Option<i64>,
    pub lock_speed: Option<bool>,
    pub lock_teams: Option<bool>,
    pub map_size: Option<String>,
    pub pop: Option<i64>,
    pub ranked: Option<bool>,
    pub rating_type_id: Option<i64>,
    pub server: Option<String>,
    pub shared_exploration: Option<bool>,
    pub speed: Option<String>,
    pub starting_age: Option<String>,
    pub turbo: Option<bool>,
    pub victory: Option<String>,
    pub visibility: Option<String>,
    pub num_spectators: Option<i64>,
    pub started: Option<i64>,
    #[serde(default)]
    pub players: Vec<Player>,
    pub opened: Option<i64>,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Player {
    pub slot: i64,
    pub slot_type: i64,
    pub steam_id: Option<String>,
    pub profile_id: Option<i64>,
    pub name: Option<String>,
    pub avatar: Option<String>,
    pub avatarfull: Option<String>,
    pub avatarmedium: Option<String>,
    pub color: Option<i64>,
    pub team: Option<i64>,
    pub civ: Option<i64>,
    pub civ_name: Option<String>,
    pub clan: Option<String>,
    pub country_code: Option<String>,
    pub rating: Option<i64>,
}


