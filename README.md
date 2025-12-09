# Live Chat Server — Run Guide

This repository contains a real-time chat backend built with Rust (`actix-web`, WebSockets) and PostgreSQL using `sqlx` for database access and migrations.

## Prerequisites
- Rust `1.89+` and `cargo`
- Docker and Docker Compose (recommended for the database)
- PostgreSQL 16 (if you prefer a local database instead of Docker)

## Quick Start (Local + Docker DB)
1. Clone the repository and change into the project directory.
2. Copy the environment template and adjust values:
   - `cp .env.example .env`
   - Edit `.env` as needed. Defaults:
     - `HOST=localhost`
     - `PORT=8080`
     - `DATABASE_URL=postgresql://postgres:postgres@localhost:5431/postgres`
     - `JWT_SECRET=your-secret-value`
3. Start PostgreSQL via Docker Compose:
   - `docker-compose up -d db`
4. Run the server:
   - `cargo run`

The server listens on `http://localhost:8080`. Database migrations are applied automatically at startup.

## Environment Variables
- `HOST`: Bind host for the HTTP server (default: `localhost`).
- `PORT`: Port for the HTTP server (default: `8080`).
- `DATABASE_URL`: PostgreSQL connection string. Matches Docker Compose defaults: `postgresql://postgres:postgres@localhost:5431/postgres`.
- `JWT_SECRET`: Secret used to sign/verify JWT tokens.

## Endpoints (for sanity check)
- `POST /api/auth/register`: Create a new user.
- `POST /api/auth/login`: Obtain a JWT token.
- `GET /api/channels` (requires Bearer token)
- `POST /api/channels` (requires Bearer token)
- WebSocket: `GET /ws/{channel_id}` (requires Bearer token in `Authorization` header)

Example register request:

```bash
curl -X POST http://localhost:8080/api/auth/register \
  -H 'Content-Type: application/json' \
  -d '{"username":"alice","email":"alice@example.com","password":"StrongPass123"}'
```

## Running Everything with Docker (Optional)
This project includes a `Dockerfile` intended to containerize the Rust server. If you prefer running the application in a container:

1. Ensure the database is up via Compose:
   - `docker-compose up -d db`
2. Build the application image, providing a `DATABASE_URL` for SQLx compile-time checks (use the in-network host `db`):
   - `docker build -t live-chat . --build-arg DATABASE_URL=postgresql://postgres:postgres@db:5432/postgres`
3. Run the application container, attaching it to the same network and passing the environment file:
   - `docker run --env-file .env --network live-chat_chat-app-networks -p 8080:8080 live-chat`

Notes:
- The Compose network name typically becomes `live-chat_chat-app-networks` (folder-based prefix). Adjust if your Compose version uses a different naming scheme.
- The container waits for the database to become ready before starting the server.

## Troubleshooting
- "Connection refused" to Postgres: verify `docker-compose ps` shows `db` healthy and that `DATABASE_URL` matches your setup.
- Port 8080 in use: change `PORT` in `.env` and restart.
- Missing `.env`: copy from `.env.example` and set `JWT_SECRET`.
- Slow startup: the server blocks until the database is reachable and migrations finish.

## Tech Stack
- `actix-web` for HTTP APIs
- `actix-ws` for WebSockets
- `sqlx` (PostgreSQL) for database access and migrations

## Contributing
- Fork the repository and create a feature branch (e.g., `feat/your-change`).
- Set up local development:
  - Copy `.env.example` to `.env` and adjust values.
  - Start Postgres: `docker-compose up -d db`.
  - Run locally: `cargo run`.
- Keep code quality high:
  - Format: `cargo fmt --all`.
  - Lint: `cargo clippy --all-targets --all-features`.
  - Test: `cargo test`.
- Database changes:
  - Add SQL files under `migrations/` and rely on auto-run at startup.
  - Ensure queries compile with SQLx. If building Docker images, pass `--build-arg DATABASE_URL=postgresql://postgres:postgres@db:5432/postgres`.
- Security and secrets:
  - Do not commit `.env` or secrets; use `.env.example` for placeholders.
- Open a Pull Request:
  - Describe the change, rationale, and testing steps.
  - Link related issues if applicable.
