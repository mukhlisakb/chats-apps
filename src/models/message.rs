use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::prelude::FromRow;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct Message {
    pub id: Uuid,
    pub channel_id: Uuid,
    pub user_id: Uuid,
    pub content: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Serialize, FromRow)]
pub struct MessageResponse {
    pub id: Uuid,
    pub channel_id: Uuid,
    pub user_id: Uuid,
    pub username: String,
    pub content: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum WsMessage {
    #[serde(rename = "chat")]
    ChatMessage {
        id: Uuid,
        user_id: Uuid,
        username: String,
        content: String,
        created_at: DateTime<Utc>,
    },
    #[serde(rename = "typing")]
    TypingIndicator {
        user_id: Uuid,
        username: String,
        is_typing: bool,
    },
    #[serde(rename = "user_joined")]
    UserJoined { user_id: Uuid, username: String },
    #[serde(rename = "user_left")]
    UserLeft { user_id: Uuid, username: String },
    #[serde(rename = "presence")]
    PresenceUpdate {
        user_id: Uuid,
        username: String,
        is_typing: bool,
    },
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ClientMessage {
    #[serde(rename = "send_message")]
    SendMessage { content: String },
    #[serde(rename = "typing")]
    Typing { is_typing: bool },
}
