use serde::{Deserialize, Serialize};

#[derive(Clone, serde_repr::Serialize_repr, serde_repr::Deserialize_repr, PartialEq, Debug)]
#[repr(u8)]
pub enum EventType {
    Init = 0,
    Data = 1,
    Time = 2,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Event {
    pub commit_id: String,
    pub event_type: EventType,
    pub content: serde_json::Value,
    pub metadata: serde_json::Value,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CeramicMetadata {
    pub controllers: Vec<String>,
    pub model: String,
}
