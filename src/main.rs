mod db;
mod handlers;
mod middleware;
mod models;
mod utils;

use crate::{
    db::pool::{create_pool, run_migrations},
    handlers::websocket::ChatServer,
};
use actix_cors::Cors;
use actix_web::{
    http::header::{ACCEPT, AUTHORIZATION, CONTENT_TYPE},
    web::{self},
    App, HttpServer,
};
use actix_web_httpauth::middleware::HttpAuthentication;
use dotenv::dotenv;
use env_logger::Env;
use std::env;

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    dotenv().ok();

    env_logger::init_from_env(Env::default().default_filter_or("info"));

    let database_url = env::var("DATABASE_URL").expect("DATABASE_URL must be set in .env file!");

    let pool = create_pool(&database_url)
        .await
        .expect("Failed to create database pool!");

    run_migrations(&pool)
        .await
        .expect("Failed to run migrations!");

    let (chat_server, chat_server_handle) = ChatServer::new(pool.clone());
    tokio::spawn(chat_server.run());

    let host = env::var("HOST").unwrap_or_else(|_| "localhost".to_string());
    let port = env::var("PORT").unwrap_or_else(|_| "8080".to_string());
    let address = format!("{}:{}", host, port);

    HttpServer::new(move || {
        let auth = HttpAuthentication::bearer(middleware::auth::jwt_validator);
        App::new()
            .wrap(actix_web::middleware::Logger::default())
            .wrap(
                Cors::default()
                    .allow_any_origin()
                    .allow_any_method()
                    .send_wildcard()
                    .allowed_headers(vec![AUTHORIZATION, ACCEPT])
                    .allowed_header(CONTENT_TYPE)
                    .max_age(3600),
            )
            .app_data(web::Data::new(pool.clone()))
            .app_data(web::Data::new(chat_server_handle.clone()))
            .service(
                // public
                web::scope("/api/auth")
                    .route("/login", web::post().to(handlers::auth::login))
                    .route("/register", web::post().to(handlers::auth::register)),
            )
            .service(
                // private
                web::scope("/api")
                    .wrap(auth)
                    .route("/channels", web::get().to(handlers::channel::list_channels))
                    .route(
                        "/channels",
                        web::post().to(handlers::channel::create_channel),
                    )
                    .route(
                        "/channels/{id}",
                        web::get().to(handlers::channel::get_channel),
                    )
                    .route(
                        "/channels/{id}/invite",
                        web::post().to(handlers::invitation::invite_user),
                    )
                    .route(
                        "/channels/{id}/messages",
                        web::get().to(handlers::channel::get_messages),
                    )
                    .route(
                        "/invitations",
                        web::get().to(handlers::invitation::list_invitations),
                    )
                    .route(
                        "/invitations/{id}/respond",
                        web::post().to(handlers::invitation::respond_to_invitation),
                    ),
            )
            .route(
                "/ws/{channel_id}",
                web::get().to(handlers::websocket::websocket_handler),
            )
    })
    .bind(&address)?
    .run()
    .await
}
