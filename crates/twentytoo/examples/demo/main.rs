//! The demo app: two demo resources on the in-memory adapter, now behind
//! the real login flow — with auth enabled, unauthenticated requests
//! redirect to `/login`, sessions live in PostgreSQL, and the framework's
//! `/users` area manages the accounts.
//!
//! Requires the compose Postgres for the framework-owned tables (start it
//! with `docker compose up -d db`, or point `DATABASE_URL` elsewhere).
//! Sign in with the bootstrap admin: admin@example.com / admin1234.

mod policy;
mod stores;
mod users;

use std::sync::Arc;

use stores::{Store, StoreResource, seed_stores};
use twentytoo::application::auth::{AuthConfig, BootstrapAdmin};
use twentytoo::prelude::*;
use users::{User, UserResource, seed_users};

/// The compose database (`compose.yaml`), unless `DATABASE_URL` overrides.
const DEFAULT_DATABASE_URL: &str = "postgres://twentytoo:twentytoo@localhost:5433/twentytoo";

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // The framework-owned tables (users, sessions, login tokens, audit)
    // live in PostgreSQL — no database, no login to gate with. The
    // builder connects and migrates at boot.
    let db_url =
        std::env::var("DATABASE_URL").unwrap_or_else(|_| return DEFAULT_DATABASE_URL.to_string());

    let users = Arc::new(InMemoryAdapter::<User>::new());
    let stores = Arc::new(InMemoryAdapter::<Store>::new());

    // seed
    seed_users(&users);
    seed_stores(&stores);

    let app = match Twentytoo::builder()
        .resource(UserResource { adapter: users })
        .resource(StoreResource { adapter: stores })
        .db(&db_url)
        .migrate()
        .auth(AuthConfig {
            bootstrap_admin: Some(BootstrapAdmin {
                email: "admin@example.com".to_string(),
                name: "Admin".to_string(),
                password: "admin1234".to_string(),
            }),
            ..Default::default()
        })
        .build()
        .await
    {
        Ok(app) => app,
        Err(e) => {
            eprintln!("cannot start: {e}");
            eprintln!("start the database first: docker compose up -d db   (or set DATABASE_URL)");
            return Err(e.into());
        }
    };

    let addr = std::env::var("ADDR").unwrap_or_else(|_| return "127.0.0.1:3000".to_string());
    let listener = tokio::net::TcpListener::bind(&addr).await?;

    println!("twentytoo demo -> http://{addr}");
    println!("sign in: admin@example.com / admin1234");
    axum::serve(listener, app.into_make_service()).await?;

    return Ok(());
}
