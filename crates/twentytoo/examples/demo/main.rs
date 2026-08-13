//! The demo app: two resources on the in-memory adapter (`03` §15 — a
//! checkout with no database still boots), driven through the generated
//! CRUD views. Run: `cargo run -p twentytoo --example demo`.

mod policy;
mod stores;
mod users;

use std::sync::Arc;

use stores::{Store, StoreResource, seed_stores};
use twentytoo::prelude::*;
use users::{User, UserResource, seed_users};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let users = Arc::new(InMemoryAdapter::<User>::new());
    seed_users(&users);
    let stores = Arc::new(InMemoryAdapter::<Store>::new());
    seed_stores(&stores);

    let app = twentytoo::Twentytoo::builder()
        .resource(UserResource { adapter: users })
        .resource(StoreResource { adapter: stores })
        .default_actor(Actor {
            id: "admin".to_string(),
            email: "admin@example.com".to_string(),
            roles: vec!["admin".to_string()],
            permissions: vec!["*.*".to_string()],
            team_id: None,
        })
        .build()
        .await?;

    let addr = std::env::var("ADDR").unwrap_or_else(|_| return "127.0.0.1:3000".to_string());
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    println!("twentytoo demo → http://{addr}");
    axum::serve(listener, app.into_make_service()).await?;
    return Ok(());
}
