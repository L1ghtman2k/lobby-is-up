use serde_derive::Deserialize;
use serde_derive::Serialize;
use std::collections::HashMap;

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WebsocketMessageReceiveAllCurrentLobbies {
    pub allcurrentlobbies: HashMap<String, Lobby>,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WebsocketMessageReceiveFollowUp {
    pub deletedlobbies: Vec<i64>,
    pub updatedlobbies: HashMap<String, Lobby>,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Lobby {
    pub maxplayers: i64,
    pub relayserver_region: String,
    pub lobbyid: i64,
    pub slotstaken: i64,
    pub slotstotal: i64,
    pub slot: HashMap<String, Slot>,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Slot {
    pub color: String,
    pub team: Option<String>,
    pub civ: String,
    pub name: Option<String>,
}
