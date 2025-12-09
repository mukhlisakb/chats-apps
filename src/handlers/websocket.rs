use crate::models::WsMessage;
use crate::models::{ClientMessage, Message as DbMessage};
use actix_web::{web, HttpRequest, HttpResponse};
use actix_ws::Message as WsFrameMessage;
use futures_util::StreamExt;
use sqlx::PgPool;
use std::time::Duration;
use std::time::Instant;
use std::{
    collections::{HashMap, HashSet},
    env,
};
use tokio::sync::mpsc;
use uuid::Uuid;

static CON_ID_COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

fn next_conn_id() -> ConnId {
    CON_ID_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
}

const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(5);
const CLIENT_TIMEOUT: Duration = Duration::from_secs(10);

type ConnId = u64;
type Msg = String;

#[derive(Debug)]
enum Command {
    Connect {
        conn_id: ConnId,
        user_id: Uuid,
        username: String,
        channel_id: Uuid,
        tx: mpsc::UnboundedSender<Msg>,
    },
    Disconnect {
        conn_id: ConnId,
    },
    Message {
        conn_id: ConnId,
        channel_id: Uuid,
        message: WsMessage,
    },
}

pub struct ChatServer {
    sessions: HashMap<ConnId, mpsc::UnboundedSender<Msg>>,
    session_info: HashMap<ConnId, (Uuid, String, Uuid)>,
    channels: HashMap<Uuid, HashSet<ConnId>>,
    #[allow(dead_code)]
    db_pool: PgPool,
    cmd_rx: mpsc::UnboundedReceiver<Command>,
}

impl ChatServer {
    pub fn new(db_pool: PgPool) -> (Self, ChatServerHandle) {
        let (cmd_tx, cmd_rx) = mpsc::unbounded_channel();

        let server = Self {
            sessions: HashMap::new(),
            session_info: HashMap::new(),
            channels: HashMap::new(),
            db_pool,
            cmd_rx,
        };

        let handle = ChatServerHandle { cmd_tx };

        (server, handle)
    }

    pub async fn run(mut self) {
        while let Some(cmd) = self.cmd_rx.recv().await {
            match cmd {
                Command::Connect {
                    conn_id,
                    user_id,
                    username,
                    channel_id,
                    tx,
                } => {
                    self.sessions.insert(conn_id, tx);
                    self.session_info
                        .insert(conn_id, (user_id, username.clone(), channel_id));
                    self.channels.entry(channel_id).or_default().insert(conn_id);
                    let join_message = WsMessage::UserJoined { user_id, username };
                    self.send_to_channel(&channel_id, join_message, Some(conn_id));
                }
                Command::Disconnect { conn_id } => {
                    self.sessions.remove(&conn_id);
                    if let Some((user_id, username, channel_id)) =
                        self.session_info.remove(&conn_id)
                    {
                        if let Some(sessions) = self.channels.get_mut(&channel_id) {
                            sessions.remove(&conn_id);
                            if sessions.is_empty() {
                                self.channels.remove(&channel_id);
                            }
                        }

                        let leave_msg = WsMessage::UserLeft { user_id, username };
                        self.send_to_channel(&channel_id, leave_msg, None);
                    }
                }
                Command::Message {
                    conn_id,
                    channel_id,
                    message,
                } => {
                    self.send_to_channel(&channel_id, message, Some(conn_id));
                }
            }
        }
    }

    fn send_to_channel(&self, channel_id: &Uuid, message: WsMessage, skip: Option<ConnId>) {
        if let Some(sessions) = self.channels.get(channel_id) {
            let msg_text = serde_json::to_string(&message).unwrap();
            for &conn_id in sessions {
                if let Some(skip_id) = skip {
                    if conn_id == skip_id {
                        continue;
                    }
                }
                if let Some(tx) = self.sessions.get(&conn_id) {
                    let _ = tx.send(msg_text.clone());
                }
            }
        }
    }
}

#[derive(Clone)]
pub struct ChatServerHandle {
    cmd_tx: mpsc::UnboundedSender<Command>,
}

impl ChatServerHandle {
    pub fn connect(
        &self,
        conn_id: ConnId,
        user_id: Uuid,
        username: String,
        channel_id: Uuid,
        tx: mpsc::UnboundedSender<Msg>,
    ) {
        let _ = self.cmd_tx.send(Command::Connect {
            conn_id,
            user_id,
            username,
            channel_id,
            tx,
        });
    }

    pub fn disconnect(&self, conn_id: ConnId) {
        let _ = self.cmd_tx.send(Command::Disconnect { conn_id });
    }

    pub fn send_message(&self, conn_id: ConnId, channel_id: Uuid, message: WsMessage) {
        println!("{:?}", message);
        let _ = self.cmd_tx.send(Command::Message {
            conn_id,
            channel_id,
            message,
        });
    }
}

pub async fn websocket_handler(
    req: HttpRequest,
    stream: web::Payload,
    path: web::Path<Uuid>,
    server: web::Data<ChatServerHandle>,
    pool: web::Data<PgPool>,
    query: web::Query<HashMap<String, String>>,
) -> Result<HttpResponse, actix_web::Error> {
    // /ws/{channel_id}
    let channel_id = path.into_inner();

    // ?token=<token>
    let token = query
        .get("token")
        .ok_or_else(|| actix_web::error::ErrorUnauthorized("No token provided"))?;

    let secret = env::var("JWT_SECRET").unwrap_or_else(|_| "secret".to_string());
    let claims = crate::utils::jwt::decode_jwt(token, secret)
        .map_err(|_| actix_web::error::ErrorUnauthorized("Invalid token"))?;

    let user_id = Uuid::parse_str(&claims.sub)
        .map_err(|_| actix_web::error::ErrorInternalServerError("Invalid user ID"))?;

    let is_member = sqlx::query_scalar::<_, bool>(
        r#"
        SELECT EXISTS(
            SELECT 1 FROM channel_members
            WHERE channel_id = $1 AND user_id = $2
        )"#,
    )
    .bind(channel_id)
    .bind(user_id)
    .fetch_one(pool.get_ref())
    .await
    .map_err(|_| actix_web::error::ErrorInternalServerError("Database error"))?;

    if !is_member {
        return Err(actix_web::error::ErrorForbidden(
            "Not a member for this channel",
        ));
    };

    let (response, session, msg_stream) = actix_ws::handle(&req, stream)?;

    let conn_id = next_conn_id();
    let username = claims.username;
    let server = server.get_ref().clone();
    let db_pool = pool.get_ref().clone();

    tokio::task::spawn_local(chat_ws_handler(
        session, msg_stream, server, conn_id, user_id, username, channel_id, db_pool,
    ));

    Ok(response)
}

async fn chat_ws_handler(
    mut session: actix_ws::Session,
    mut msg_stream: actix_ws::MessageStream,
    server: ChatServerHandle,
    conn_id: ConnId,
    user_id: Uuid,
    username: String,
    channel_id: Uuid,
    db_pool: PgPool,
) {
    let (tx, mut rx) = mpsc::unbounded_channel();

    server.connect(conn_id, user_id, username.clone(), channel_id, tx);

    let mut last_heartbeat = Instant::now();
    let mut interval = tokio::time::interval(HEARTBEAT_INTERVAL);

    loop {
        tokio::select! {
            Some(msg) = rx.recv() => {
                if session.text(msg).await.is_err() {
                    break;
                }
            }
            Some(Ok(msg)) = msg_stream.next() => {
                match msg {
                    WsFrameMessage::Text(text) => {
                        last_heartbeat = Instant::now();

                        if let Ok(client_msg) = serde_json::from_str::<ClientMessage>(&text) {
                            match client_msg {
                                ClientMessage::SendMessage { content } => {
                                    let channel_id_clone = channel_id;
                                    let user_id_clone = user_id;
                                    let username_clone = username.clone();
                                    let db_pool_clone = db_pool.clone();
                                    let server_clone = server.clone();

                                    tokio::spawn(async move {
                                        if let Ok(msg) = sqlx::query_as::<_, DbMessage>(r#"
                                        INSERT INTO messages (channel_id, user_id, content)
                                        VALUES ($1, $2, $3)
                                        RETURNING id, channel_id, user_id, content, created_at
                                            "#,)
                                            .bind(channel_id_clone)
                                            .bind(user_id_clone)
                                            .bind(&content)
                                            .fetch_one(&db_pool_clone)
                                            .await {
                                                println!("{:?}", msg);
                                                let ws_msg = WsMessage::ChatMessage {
                                                    id: msg.id,
                                                    user_id: user_id_clone,
                                                    username: username_clone,
                                                    content: msg.content,
                                                    created_at: msg.created_at
                                                };

                                                server_clone.send_message(conn_id, channel_id, ws_msg);
                                        }
                                    });
                                }
                                ClientMessage::Typing { is_typing } => {
                                    let typing_msg = WsMessage::TypingIndicator {
                                        user_id,
                                        username: username.clone(),
                                        is_typing,
                                    };

                                   server.send_message(conn_id, channel_id, typing_msg);
                                }
                            }
                        }
                    }
                    WsFrameMessage::Ping(bytes) => {
                        last_heartbeat = Instant::now();
                        if session.pong(&bytes).await.is_err() {
                            break;
                        }
                    }
                    WsFrameMessage::Pong(_) => {
                        last_heartbeat = Instant::now();
                    }
                    WsFrameMessage::Close(_) => break,
                    _ => {}
                }
            }
            _ = interval.tick() => {
                if Instant::now().duration_since(last_heartbeat) > CLIENT_TIMEOUT {
                    break;
                }

                if session.ping(b"").await.is_err() {
                    break
                }
            }
            else => break,
        }
    }

    server.disconnect(conn_id);
    let _ = session.close(None).await;
}
