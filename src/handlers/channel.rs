use crate::{
    models::{
        channel::{
            Channel, ChannelMemberInfo, ChannelResponse, ChannelWithMembers, CreateChannelRequest,
        },
        MessageResponse,
    },
    utils::jwt::Claims,
};
use actix_web::{web, HttpMessage, HttpRequest, HttpResponse};
use sqlx::PgPool;
use uuid::Uuid;

pub async fn create_channel(
    pool: web::Data<PgPool>,
    req: HttpRequest,
    body: web::Json<CreateChannelRequest>,
) -> Result<HttpResponse, actix_web::Error> {
    let claims = req
        .extensions()
        .get::<Claims>()
        .cloned()
        .ok_or_else(|| actix_web::error::ErrorUnauthorized("No claims found"))?;

    let user_id = Uuid::parse_str(&claims.sub)
        .map_err(|_| actix_web::error::ErrorUnauthorized("Invalid user ID"))?;

    let channel = sqlx::query_as::<_, Channel>(
        r#"
        INSERT INTO channels (name, created_by)
        VALUES ($1, $2)
        RETURNING id, name, created_by, created_at
        "#,
    )
    .bind(&body.name)
    .bind(user_id)
    .fetch_one(pool.get_ref())
    .await
    .map_err(|_| actix_web::error::ErrorInternalServerError("Failed to create channel"))?;

    sqlx::query(
        r#"
        INSERT INTO channel_members (channel_id, user_id, role)
        VALUES ($1, $2, 'admin')
        "#,
    )
    .bind(channel.id)
    .bind(user_id)
    .execute(pool.get_ref())
    .await
    .map_err(|_| actix_web::error::ErrorInternalServerError("Failed to add member"))?;

    Ok(HttpResponse::Ok().json(ChannelResponse {
        id: channel.id,
        name: channel.name,
        created_by: channel.created_by,
        created_at: channel.created_at,
        role: "admin".to_string(),
    }))
}

pub async fn list_channels(
    pool: web::Data<PgPool>,
    req: HttpRequest,
) -> Result<HttpResponse, actix_web::Error> {
    let claims = req
        .extensions()
        .get::<Claims>()
        .cloned()
        .ok_or_else(|| actix_web::error::ErrorInternalServerError("No claims found"))?;

    let user_id = Uuid::parse_str(&claims.sub)
        .map_err(|_| actix_web::error::ErrorInternalServerError("Invalid user ID"))?;

    let channels: Vec<ChannelResponse> = sqlx::query_as::<_, ChannelResponse>(
        r#"
        SELECT c.id, c.name, c.created_by, c.created_at, cm.role
        FROM channels c
        INNER JOIN channel_members cm ON c.id = cm.channel_id
        WHERE cm.user_id = $1
        ORDER BY created_at DESC
        "#,
    )
    .bind(user_id)
    .fetch_all(pool.get_ref())
    .await
    .map_err(|_| actix_web::error::ErrorInternalServerError("Failed to fetch channels"))?;

    Ok(HttpResponse::Ok().json(channels))
}

pub async fn get_channel(
    pool: web::Data<PgPool>,
    req: HttpRequest,
    path: web::Path<Uuid>,
) -> Result<HttpResponse, actix_web::Error> {
    let claims = req
        .extensions()
        .get::<Claims>()
        .cloned()
        .ok_or_else(|| actix_web::error::ErrorInternalServerError("No claims found"))?;

    let user_id = Uuid::parse_str(&claims.sub)
        .map_err(|_| actix_web::error::ErrorInternalServerError("Invalid user id"))?;

    let channel_id = path.into_inner();

    let is_member = sqlx::query_scalar::<_, bool>(
        r#"
    SELECT EXISTS(
        SELECT 1 FROM channel_members
        WHERE channel_id = $1 AND user_id = $2
    )
        "#,
    )
    .bind(channel_id)
    .bind(user_id)
    .fetch_one(pool.get_ref())
    .await
    .map_err(|_| actix_web::error::ErrorInternalServerError("Database error"))?;

    if !is_member {
        return Err(actix_web::error::ErrorInternalServerError(
            "Not a member of this channel",
        ));
    }

    let channel = sqlx::query_as::<_, Channel>(
        r#"
        SELECT id, name, created_by, created_at
        FROM channels
        WHERE id = $1
    "#,
    )
    .bind(channel_id)
    .fetch_optional(pool.get_ref())
    .await
    .map_err(|_| actix_web::error::ErrorInternalServerError("Database error"))?
    .ok_or_else(|| actix_web::error::ErrorNotFound("CHannel not found"))?;

    let members = sqlx::query_as::<_, ChannelMemberInfo>(
        r#"
    SELECT cm.user_id, u.username, cm.role, false as is_online
    FROM channel_members cm 
    INNER JOIN users u ON cm.user_id = u.id
    WHERE cm.channel_id = $1
    ORDER BY cm.role DESC, u.username
        "#,
    )
    .bind(channel_id)
    .fetch_all(pool.get_ref())
    .await
    .map_err(|_| actix_web::error::ErrorInternalServerError("Failed to fetch"))?;

    Ok(HttpResponse::Ok().json(ChannelWithMembers {
        id: channel.id,
        name: channel.name,
        created_by: channel.created_by,
        created_at: channel.created_at,
        members,
    }))
}

pub async fn get_messages(
    pool: web::Data<PgPool>,
    req: HttpRequest,
    path: web::Path<Uuid>,
) -> Result<HttpResponse, actix_web::Error> {
    let claims = req
        .extensions()
        .get::<Claims>()
        .cloned()
        .ok_or_else(|| actix_web::error::ErrorUnauthorized("No claims found"))?;

    let user_id = Uuid::parse_str(&claims.sub)
        .map_err(|_| actix_web::error::ErrorInternalServerError("Invalid user id"))?;

    let channel_id = path.into_inner();

    let is_member = sqlx::query_scalar::<_, bool>(
        r#"
    SELECT EXISTS(
        SELECT 1 FROM channel_members
        WHERE channel_id = $1 AND user_id = $2
    )
        "#,
    )
    .bind(channel_id)
    .bind(user_id)
    .fetch_one(pool.get_ref())
    .await
    .map_err(|_| actix_web::error::ErrorInternalServerError("Database error"))?;

    if !is_member {
        return Err(actix_web::error::ErrorForbidden(
            "Not a memmber of this channel",
        ));
    }

    let messages = sqlx::query_as::<_, MessageResponse>(
        r#"
    SELECT m.id, m.channel_id, m.user_id, u.username, m.content, m.created_at
    FROM messages m 
    INNER JOIN users u ON m.user_id = u.id
    WHERE m.channel_id = $1
    ORDER BY m.created_at DESC
    LIMIT 100
        "#,
    )
    .bind(channel_id)
    .fetch_all(pool.get_ref())
    .await
    .map_err(|_| actix_web::error::ErrorInternalServerError("Failed to fetch"))?;

    Ok(HttpResponse::Ok().json(messages))
}
