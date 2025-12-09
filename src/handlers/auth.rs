use crate::{
    models::user::{AuthResponse, LoginRequest, RegisterRequest, User},
    utils::jwt::create_jwt,
};
use actix_web::{web, HttpResponse};
use bcrypt::{hash, verify, DEFAULT_COST};
use sqlx::PgPool;
use std::env;

pub async fn register(
    pool: web::Data<PgPool>,
    req: web::Json<RegisterRequest>,
) -> Result<HttpResponse, actix_web::Error> {
    // hash password
    let password_hash = hash(&req.password, DEFAULT_COST)
        .map_err(|_| actix_web::error::ErrorInternalServerError("Failed to hash password"))?;

    let user = sqlx::query_as::<_, User>(
        r#"
        INSERT INTO users (username, email, password_hash)
        VALUES ($1, $2, $3)
        RETURNING id, username, email, password_hash, created_at
        "#,
    )
    .bind(&req.username)
    .bind(&req.email)
    .bind(&password_hash)
    .fetch_one(pool.get_ref())
    .await
    .map_err(|e| match e {
        sqlx::Error::Database(db_err) => {
            if db_err.constraint().is_some() {
                actix_web::error::ErrorConflict("Username or email already exists")
            } else {
                actix_web::error::ErrorInternalServerError("Database error")
            }
        }
        _ => actix_web::error::ErrorInternalServerError("Database error"),
    })?;

    let secret = env::var("JWT_SECRET").unwrap_or_else(|_| "secret".to_string());
    let token = create_jwt(user.id, &req.username, &secret)
        .map_err(|_| actix_web::error::ErrorInternalServerError("Failed to create token"))?;

    Ok(HttpResponse::Created().json(AuthResponse {
        token,
        user: user.into(),
    }))
}

pub async fn login(
    pool: web::Data<PgPool>,
    req: web::Json<LoginRequest>,
) -> Result<HttpResponse, actix_web::Error> {
    let user = sqlx::query_as::<_, User>(
        r#"
        SELECT id, username, email, password_hash, created_at
        FROM users
        WHERE email = $1
        "#,
    )
    .bind(&req.email)
    .fetch_optional(pool.get_ref())
    .await
    .map_err(|_| actix_web::error::ErrorInternalServerError("Database error"))?
    .ok_or_else(|| actix_web::error::ErrorUnauthorized("Invalid credentials"))?;

    let valid = verify(&req.password, &user.password_hash)
        .map_err(|_| actix_web::error::ErrorInternalServerError("Password verification failed"))?;

    if !valid {
        return Err(actix_web::error::ErrorUnauthorized("Invalid credentials"));
    }

    let secret = env::var("JWT_SECRET").unwrap_or_else(|_| "secret".to_string());
    let token = create_jwt(user.id, &user.username, &secret)
        .map_err(|_| actix_web::error::ErrorInternalServerError("Failed to create token"))?;

    Ok(HttpResponse::Ok().json(AuthResponse {
        token,
        user: user.into(),
    }))
}
