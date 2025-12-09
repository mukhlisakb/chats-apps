use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::prelude::FromRow;
use uuid::Uuid;

#[derive(Debug, Deserialize)]
pub struct InviteByEmailRequest {
    pub email: String,
}

#[derive(Debug, Serialize, FromRow)]
pub struct InvitationResponse {
    pub id: Uuid,
    pub channel_id: Uuid,
    pub channel_name: String,
    pub inviter_id: Uuid,
    pub inviter_username: String,
    pub status: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize)]
pub struct RespondToInvitationRequest {
    pub accept: bool,
}
