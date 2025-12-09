use crate::{
    models::invitation::{InvitationResponse, InviteByEmailRequest, RespondToInvitationRequest},
    utils::jwt::Claims,
};
use actix_web::{web, HttpMessage, HttpRequest, HttpResponse};
use sqlx::PgPool;
use uuid::Uuid;

pub async fn invite_user(
    pool: web::Data<PgPool>,
    req: HttpRequest,
    path: web::Path<Uuid>,
    body: web::Json<InviteByEmailRequest>,
) -> Result<HttpResponse, actix_web::Error> {
    let claims = req
        .extensions()
        .get::<Claims>()
        .cloned()
        .ok_or_else(|| actix_web::error::ErrorUnauthorized("No claims found"))?;

    let inviter_id = Uuid::parse_str(&claims.sub)
        .map_err(|_| actix_web::error::ErrorInternalServerError("Invalid user id"))?;

    let channel_id = path.into_inner();

    let is_admin = sqlx::query_scalar::<_, bool>(
        r#"
        SELECT EXISTS(
            SELECT 1 from channel_members
            WHERE channel_id = $1 AND user_id = $2 AND role = 'admin'
        )
        "#,
    )
    .bind(channel_id)
    .bind(inviter_id)
    .fetch_one(pool.get_ref())
    .await
    .map_err(|_| actix_web::error::ErrorInternalServerError("Database error"))?;

    if !is_admin {
        return Err(actix_web::error::ErrorForbidden(
            "Only admins can invite users",
        ));
    }

    let invitee_id = sqlx::query_scalar::<_, Uuid>(
        r#"
        SELECT id FROM users WHERE email = $1
        "#,
    )
    .bind(&body.email)
    .fetch_optional(pool.get_ref())
    .await
    .map_err(|_| actix_web::error::ErrorInternalServerError("Database error"))?
    .ok_or_else(|| actix_web::error::ErrorNotFound("User not found"))?;

    let is_member = sqlx::query_scalar::<_, bool>(
        r#"
        SELECT EXISTS(
            SELECT 1 FROM channel_members
            WHERE channel_id = $1 AND user_id = $2
        )
    "#,
    )
    .bind(channel_id)
    .bind(invitee_id)
    .fetch_one(pool.get_ref())
    .await
    .map_err(|_| actix_web::error::ErrorInternalServerError("Database error"))?;

    if is_member {
        return Err(actix_web::error::ErrorConflict("User is already a member"));
    }

    let invitation_id = sqlx::query_scalar::<_, Uuid>(
        r#"
        INSERT INTO invitations (channel_id, inviter_id, invitee_id, status)
        VALUES ($1, $2, $3, 'pending')
        ON CONFLICT (channel_id, invitee_id)
        DO UPDATE SET status = 'pending', created_at = NOW()
        RETURNING id
        "#,
    )
    .bind(channel_id)
    .bind(inviter_id)
    .bind(invitee_id)
    .fetch_one(pool.get_ref())
    .await
    .map_err(|_| actix_web::error::ErrorInternalServerError("Failed to create new invitation"))?;

    let invitation = sqlx::query_as::<_, InvitationResponse>(
        r#"
          SELECT
            i.id, i.channel_id, c.name as channel_name,
            i.inviter_id, u.username as inviter_username,
            i.status, i.created_at
          FROM invitations i
          INNER JOIN channels c ON i.channel_id = c.id
          INNER JOIN users u ON i.inviter_id = u.id
          WHERE i.id = $1
        "#,
    )
    .bind(invitation_id)
    .fetch_one(pool.get_ref())
    .await
    .map_err(|_| actix_web::error::ErrorInternalServerError("Database error"))?;

    Ok(HttpResponse::Created().json(invitation))
}

pub async fn list_invitations(
    pool: web::Data<PgPool>,
    req: HttpRequest,
) -> Result<HttpResponse, actix_web::Error> {
    let claims = req
        .extensions()
        .get::<Claims>()
        .cloned()
        .ok_or_else(|| actix_web::error::ErrorUnauthorized("No claims found"))?;

    let user_id = Uuid::parse_str(&claims.sub)
        .map_err(|_| actix_web::error::ErrorInternalServerError("Invalid user id"))?;

    let invitations = sqlx::query_as::<_, InvitationResponse>(
        r#"
        SELECT
            i.id, i.channel_id, c.name as channel_name,
            i.inviter_id, u.username as inviter_username,
            i.status, i.created_at
        FROM invitations i
        INNER JOIN channels c ON i.channel_id = c.id 
        INNER JOIN users u ON i.inviter_id = u.id 
        WHERE i.invitee_id = $1 AND i.status = 'pending'
        ORDER BY i.created_at DESC
        "#,
    )
    .bind(user_id)
    .fetch_all(pool.get_ref())
    .await
    .map_err(|_| actix_web::error::ErrorInternalServerError("Failed to fetch invitations"))?;

    Ok(HttpResponse::Ok().json(invitations))
}

pub async fn respond_to_invitation(
    pool: web::Data<PgPool>,
    req: HttpRequest,
    path: web::Path<Uuid>,
    body: web::Json<RespondToInvitationRequest>,
) -> Result<HttpResponse, actix_web::Error> {
    let claims = req
        .extensions()
        .get::<Claims>()
        .cloned()
        .ok_or_else(|| actix_web::error::ErrorUnauthorized("No claims found"))?;

    let user_id = Uuid::parse_str(&claims.sub)
        .map_err(|_| actix_web::error::ErrorInternalServerError("Invalid user id"))?;

    let invitation_id = path.into_inner();

    #[derive(sqlx::FromRow)]
    struct InvitationRow {
        channel_id: Uuid,
        invitee_id: Uuid,
        status: String,
    }

    let invitation = sqlx::query_as::<_, InvitationRow>(
        r#"
    SELECT channel_id, invitee_id, status
    FROM invitations
    WHERE id = $1
    "#,
    )
    .bind(invitation_id)
    .fetch_optional(pool.get_ref())
    .await
    .map_err(|_| actix_web::error::ErrorInternalServerError("Database error"))?
    .ok_or_else(|| actix_web::error::ErrorNotFound("Invitation not found"))?;

    if invitation.invitee_id != user_id {
        return Err(actix_web::error::ErrorForbidden("Not your invitation"));
    }

    if invitation.status != "pending" {
        return Err(actix_web::error::ErrorConflict(
            "Invitation already processed",
        ));
    }

    let new_status = if body.accept { "accepted" } else { "rejected" };

    sqlx::query(
        r#"
        UPDATE invitations
        SET status = $1
        WHERE id = $2
        "#,
    )
    .bind(new_status)
    .bind(invitation_id)
    .execute(pool.get_ref())
    .await
    .map_err(|_| {
        actix_web::error::ErrorInternalServerError("Failed to update status invitation")
    })?;

    if body.accept {
        sqlx::query(
            r#"
            INSERT INTO channel_members (channel_id, user_id, role)
            VALUES ($1, $2, 'member')
            "#,
        )
        .bind(invitation.channel_id)
        .bind(user_id)
        .execute(pool.get_ref())
        .await
        .map_err(|_| actix_web::error::ErrorInternalServerError("Failed to add members"))?;
    }

    let response = if body.accept {
        "Invitation accepted"
    } else {
        "Invitation rejected"
    };

    Ok(HttpResponse::Ok().json(response))
}
