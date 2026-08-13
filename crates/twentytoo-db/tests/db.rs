//! Integration tests against a live PostgreSQL: migrations + the typed
//! access layer, end to end.
//!
//! Without `DATABASE_URL` the tests skip (CI with no database stays
//! green). With it, the target database is created on first use if
//! missing, then migrated:
//!
//! ```sh
//! DATABASE_URL=postgres://localhost/twentytoo_test cargo test -p twentytoo-db
//! ```

use chrono::{Duration, Utc};
use sqlx::postgres::PgPoolOptions;
use twentytoo_db::users::UserStatus;
use twentytoo_db::{Db, DbError};
use uuid::Uuid;

/// Connect to `DATABASE_URL`, creating the database if it does not exist
/// yet, and apply migrations. `None` when `DATABASE_URL` is unset — the
/// skip case. Any real connection failure panics.
async fn connect_test_db() -> Option<Db> {
    let url = match std::env::var("DATABASE_URL") {
        Ok(url) => url,
        Err(_) => {
            eprintln!("skipping db tests: DATABASE_URL not set");
            return None;
        }
    };
    let db = match Db::connect(&url).await {
        Ok(db) => db,
        Err(sqlx::Error::Database(e)) if e.code().as_deref() == Some("3D000") => {
            ensure_database(&url).await;
            Db::connect(&url).await.expect("reconnect after create")
        }
        Err(e) => panic!("cannot connect to DATABASE_URL: {e}"),
    };
    db.migrate().await.expect("migrations apply");
    return Some(db);
}

/// Create the database named in `url` via a connection to the `postgres`
/// maintenance database. The name is validated to alphanumerics, `_`, `-`
/// before interpolation.
async fn ensure_database(url: &str) {
    let (base, query) = match url.split_once('?') {
        Some((b, q)) => (b, Some(q)),
        None => (url, None),
    };
    let dbname = base.rsplit('/').next().expect("url names a database");
    assert!(
        !dbname.is_empty()
            && dbname
                .chars()
                .all(|c| return c.is_ascii_alphanumeric() || c == '_' || c == '-'),
        "unsafe database name in DATABASE_URL: {dbname}"
    );
    let admin = match query {
        Some(q) => format!("{}/postgres?{q}", &base[..base.len() - dbname.len() - 1]),
        None => format!("{}/postgres", &base[..base.len() - dbname.len() - 1]),
    };
    let pool = PgPoolOptions::new()
        .connect(&admin)
        .await
        .expect("connect to maintenance database");
    let exists: bool =
        sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM pg_database WHERE datname = $1)")
            .bind(dbname)
            .fetch_one(&pool)
            .await
            .expect("check database existence");
    if !exists {
        let result = sqlx::query(&format!("CREATE DATABASE \"{dbname}\""))
            .execute(&pool)
            .await;
        // Parallel tests race the create; a unique violation on
        // pg_database means another connection won — that's success.
        match result {
            Ok(_) => {}
            Err(sqlx::Error::Database(e)) if e.code().as_deref() == Some("23505") => {}
            Err(e) => panic!("create test database: {e}"),
        }
    }
    pool.close().await;
}

/// A unique-per-test email (tests share one database and run in parallel).
fn unique_email(tag: &str) -> String {
    return format!("{tag}-{}@example.com", Uuid::new_v4().simple());
}

/// A unique-per-run suffix for role keys and permission codes, which carry
/// global uniqueness constraints and must not collide with leftovers from
/// earlier test runs against the same database.
fn run_suffix() -> String {
    static SUFFIX: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    return SUFFIX
        .get_or_init(|| return format!("t{}", Uuid::new_v4().simple()))
        .clone();
}

// ---------------------------------------------------------------------------
// Users
// ---------------------------------------------------------------------------

#[tokio::test]
async fn create_user_normalizes_email_and_roundtrips() {
    let Some(db) = connect_test_db().await else {
        return;
    };
    let email = unique_email("normalize");
    let user = db
        .create_user(&format!("  {email}  "), "Ada Lovelace", None)
        .await
        .expect("create");
    assert_eq!(user.email, email, "email stored lowercase");
    assert_eq!(user.name, "Ada Lovelace");
    assert_eq!(user.password_hash, None);
    assert_eq!(user.status, UserStatus::Active);

    let by_id = db.get_user(&user.id).await.expect("get by id");
    assert_eq!(by_id.expect("found").email, email);
    let by_email = db.get_user_by_email(&email).await.expect("get by email");
    assert_eq!(by_email.expect("found").id, user.id);
}

#[tokio::test]
async fn create_user_duplicate_email_conflicts() {
    let Some(db) = connect_test_db().await else {
        return;
    };
    let email = unique_email("dup");
    db.create_user(&email, "First", None).await.expect("create");
    let err = db
        .create_user(&email, "Second", None)
        .await
        .expect_err("duplicate");
    assert!(matches!(err, DbError::Conflict(_)), "got {err:?}");
}

#[tokio::test]
async fn get_user_missing_is_none() {
    let Some(db) = connect_test_db().await else {
        return;
    };
    let user = db.get_user(&Uuid::new_v4()).await.expect("query");
    assert!(user.is_none());
}

#[tokio::test]
async fn set_password_and_status_persist() {
    let Some(db) = connect_test_db().await else {
        return;
    };
    let user = db
        .create_user(&unique_email("creds"), "Creds", None)
        .await
        .expect("create");

    db.set_user_password(&user.id, "hash-v1")
        .await
        .expect("set password");
    db.set_user_status(&user.id, UserStatus::Invited)
        .await
        .expect("set status");
    let reloaded = db.get_user(&user.id).await.expect("get").expect("found");
    assert_eq!(reloaded.password_hash.as_deref(), Some("hash-v1"));
    assert_eq!(reloaded.status, UserStatus::Invited);

    db.set_user_status(&user.id, UserStatus::Disabled)
        .await
        .expect("set status");
    let reloaded = db.get_user(&user.id).await.expect("get").expect("found");
    assert_eq!(reloaded.status, UserStatus::Disabled);
}

#[tokio::test]
async fn set_password_missing_user_not_found() {
    let Some(db) = connect_test_db().await else {
        return;
    };
    let err = db
        .set_user_password(&Uuid::new_v4(), "hash")
        .await
        .expect_err("missing");
    assert!(matches!(err, DbError::NotFound), "got {err:?}");
}

// ---------------------------------------------------------------------------
// Sessions
// ---------------------------------------------------------------------------

#[tokio::test]
async fn session_roundtrip_with_team_and_metadata() {
    let Some(db) = connect_test_db().await else {
        return;
    };
    let user = db
        .create_user(&unique_email("session"), "Session", None)
        .await
        .expect("create user");
    let team = db
        .create_team(
            "Sessions Team",
            &format!("sessions-{}", Uuid::new_v4().simple()),
        )
        .await
        .expect("create team");

    let token = format!("tok-{}", Uuid::new_v4().simple());
    let expires = Utc::now() + Duration::hours(8);
    let session = db
        .create_session(
            &token,
            &user.id,
            Some(&team.id),
            expires,
            Some("test-agent"),
            Some("127.0.0.1"),
        )
        .await
        .expect("create session");
    assert_eq!(session.team_id, Some(team.id));
    assert_eq!(session.expires_at, expires);
    assert_eq!(session.user_agent.as_deref(), Some("test-agent"));
    assert_eq!(session.ip.as_deref(), Some("127.0.0.1"));
    assert!(session.last_seen_at.is_none());

    let loaded = db.get_session(&token).await.expect("get").expect("found");
    assert_eq!(loaded.user_id, user.id);
    assert_eq!(loaded.team_id, Some(team.id));

    db.touch_session(&token).await.expect("touch");
    let touched = db.get_session(&token).await.expect("get").expect("found");
    assert!(touched.last_seen_at.is_some());

    assert!(db.get_session("hash-unknown").await.expect("get").is_none());
}

#[tokio::test]
async fn expired_sessions_are_invisible_and_cleanable() {
    let Some(db) = connect_test_db().await else {
        return;
    };
    let user = db
        .create_user(&unique_email("expiry"), "Expiry", None)
        .await
        .expect("create user");

    let old_token = format!("old-{}", Uuid::new_v4().simple());
    let fresh_token = format!("fresh-{}", Uuid::new_v4().simple());
    db.create_session(
        &old_token,
        &user.id,
        None,
        Utc::now() - Duration::minutes(1),
        None,
        None,
    )
    .await
    .expect("create expired");
    db.create_session(
        &fresh_token,
        &user.id,
        None,
        Utc::now() + Duration::hours(1),
        None,
        None,
    )
    .await
    .expect("create fresh");

    assert!(db.get_session(&old_token).await.expect("get").is_none());
    assert!(db.get_session(&fresh_token).await.expect("get").is_some());

    let removed = db.delete_expired_sessions().await.expect("cleanup");
    assert!(removed >= 1, "removed {removed}");
    assert!(db.get_session(&fresh_token).await.expect("get").is_some());
}

#[tokio::test]
async fn delete_session_removes_token() {
    let Some(db) = connect_test_db().await else {
        return;
    };
    let user = db
        .create_user(&unique_email("logout"), "Logout", None)
        .await
        .expect("create user");
    let token = format!("logout-{}", Uuid::new_v4().simple());
    db.create_session(
        &token,
        &user.id,
        None,
        Utc::now() + Duration::hours(1),
        None,
        None,
    )
    .await
    .expect("create");

    db.delete_session(&token).await.expect("delete");
    assert!(db.get_session(&token).await.expect("get").is_none());
    // Deleting a gone token is a no-op, not an error.
    db.delete_session(&token).await.expect("delete again");
}

// ---------------------------------------------------------------------------
// Teams
// ---------------------------------------------------------------------------

#[tokio::test]
async fn team_roundtrip_and_membership() {
    let Some(db) = connect_test_db().await else {
        return;
    };
    let user = db
        .create_user(&unique_email("team"), "Teammate", None)
        .await
        .expect("create user");
    let slug = format!("team-{}", Uuid::new_v4().simple());
    let team = db.create_team("Ops", &slug).await.expect("create team");
    assert_eq!(team.slug, slug);

    let loaded = db.get_team(&team.id).await.expect("get").expect("found");
    assert_eq!(loaded.name, "Ops");

    db.add_member(&team.id, &user.id).await.expect("add");
    db.add_member(&team.id, &user.id)
        .await
        .expect("add again is no-op");
    let teams = db.list_teams_for_user(&user.id).await.expect("list");
    assert_eq!(teams.len(), 1);
    assert_eq!(teams[0].id, team.id);

    db.remove_member(&team.id, &user.id).await.expect("remove");
    db.remove_member(&team.id, &user.id)
        .await
        .expect("remove again is no-op");
    let teams = db.list_teams_for_user(&user.id).await.expect("list");
    assert!(teams.is_empty());
}

#[tokio::test]
async fn duplicate_team_slug_conflicts() {
    let Some(db) = connect_test_db().await else {
        return;
    };
    let slug = format!("slug-{}", Uuid::new_v4().simple());
    db.create_team("One", &slug).await.expect("create");
    let err = db.create_team("Two", &slug).await.expect_err("duplicate");
    assert!(matches!(err, DbError::Conflict(_)), "got {err:?}");
}

// ---------------------------------------------------------------------------
// Roles + permissions + actor loading
// ---------------------------------------------------------------------------

#[tokio::test]
async fn permission_and_role_roundtrip_with_grants() {
    let Some(db) = connect_test_db().await else {
        return;
    };
    let s = run_suffix();
    let res = format!("g{}", Uuid::new_v4().simple());
    let code = format!("{res}.view");
    let perm = db
        .create_permission(&code, "See stores")
        .await
        .expect("create permission");
    let err = db
        .create_permission(&code, "dup")
        .await
        .expect_err("duplicate code");
    assert!(matches!(err, DbError::Conflict(_)), "got {err:?}");
    let err = db
        .create_permission("Stores.view", "bad")
        .await
        .expect_err("malformed");
    assert!(matches!(err, DbError::Validation(_)), "got {err:?}");

    let role_key = format!("store_viewer.{s}");
    let role = db
        .create_role(&role_key, "Store Viewer", "Can see stores")
        .await
        .expect("create role");
    let err = db
        .create_role(&role_key, "dup", "")
        .await
        .expect_err("duplicate key");
    assert!(matches!(err, DbError::Conflict(_)), "got {err:?}");

    db.grant_permission(&role.id, &perm.id)
        .await
        .expect("grant");
    db.grant_permission(&role.id, &perm.id)
        .await
        .expect("grant again is no-op");
    let granted = db.list_role_permissions(&role.id).await.expect("list");
    assert_eq!(granted.len(), 1);
    assert_eq!(granted[0].code, code);

    db.revoke_permission(&role.id, &perm.id)
        .await
        .expect("revoke");
    let granted = db.list_role_permissions(&role.id).await.expect("list");
    assert!(granted.is_empty());
}

#[tokio::test]
async fn actor_loads_roles_and_expanded_permissions() {
    let Some(db) = connect_test_db().await else {
        return;
    };
    let s = run_suffix();
    let user = db
        .create_user(&unique_email("actor"), "Actor", Some("hash"))
        .await
        .expect("create user");
    let role_key = format!("admin.{s}");
    let role = db.create_role(&role_key, "Admin", "").await.expect("role");
    let mut expected_perms: Vec<String> = Vec::new();
    let res = format!("a{}", Uuid::new_v4().simple());
    for code in [
        format!("{res}.view"),
        format!("{res}.delete"),
        format!("{res}.*"),
    ] {
        let p = db.create_permission(&code, "").await.expect("permission");
        expected_perms.push(p.code.clone());
        db.grant_permission(&role.id, &p.id).await.expect("grant");
    }
    db.assign_role(&user.id, &role.id, None)
        .await
        .expect("assign");
    db.assign_role(&user.id, &role.id, None)
        .await
        .expect("assign again is no-op");

    let actor = db
        .load_actor(&user.id, None)
        .await
        .expect("load")
        .expect("user exists");
    assert_eq!(actor.id, user.id.to_string());
    assert_eq!(actor.email, user.email);
    assert_eq!(actor.roles, vec![role_key.clone()]);
    assert_eq!(actor.team_id, None);
    let mut perms = actor.permissions.clone();
    perms.sort();
    expected_perms.sort();
    assert_eq!(perms, expected_perms);

    // Revoking the role removes everything.
    db.revoke_role(&user.id, &role.id, None)
        .await
        .expect("revoke");
    let actor = db
        .load_actor(&user.id, None)
        .await
        .expect("load")
        .expect("user exists");
    assert!(actor.roles.is_empty());
    assert!(actor.permissions.is_empty());
}

#[tokio::test]
async fn actor_loads_team_scoped_roles_only_in_that_team() {
    let Some(db) = connect_test_db().await else {
        return;
    };
    let s = run_suffix();
    let user = db
        .create_user(&unique_email("scoped"), "Scoped", None)
        .await
        .expect("create user");
    let team_a = db
        .create_team("Team A", &format!("a-{}", Uuid::new_v4().simple()))
        .await
        .expect("team a");
    let team_b = db
        .create_team("Team B", &format!("b-{}", Uuid::new_v4().simple()))
        .await
        .expect("team b");

    let scoped_key = format!("store_manager.{s}");
    let global_key = format!("auditor.{s}");
    let scoped = db
        .create_role(&scoped_key, "Store Manager", "")
        .await
        .expect("role");
    let global = db
        .create_role(&global_key, "Auditor", "")
        .await
        .expect("role");
    let res = format!("s{}", Uuid::new_v4().simple());
    let edit_code = format!("{res}.edit");
    let edit = db
        .create_permission(&edit_code, "")
        .await
        .expect("permission");
    db.grant_permission(&scoped.id, &edit.id)
        .await
        .expect("grant");
    let view_code = format!("{res}.view");
    let view = db
        .create_permission(&view_code, "")
        .await
        .expect("permission");
    db.grant_permission(&global.id, &view.id)
        .await
        .expect("grant");

    db.assign_role(&user.id, &scoped.id, Some(&team_a.id))
        .await
        .expect("scoped grant");
    db.assign_role(&user.id, &global.id, None)
        .await
        .expect("global grant");

    // No team context → only the global role.
    let actor = db
        .load_actor(&user.id, None)
        .await
        .expect("load")
        .expect("user");
    assert_eq!(actor.roles, vec![global_key.clone()]);
    assert_eq!(actor.permissions, vec![view_code.clone()]);
    assert_eq!(actor.team_id, None);

    // Team A context → scoped role applies, and its team_id is carried.
    let actor = db
        .load_actor(&user.id, Some(&team_a.id))
        .await
        .expect("load")
        .expect("user");
    let mut roles = actor.roles.clone();
    roles.sort();
    assert_eq!(roles, vec![global_key.clone(), scoped_key.clone()]);
    let mut perms = actor.permissions.clone();
    perms.sort();
    assert_eq!(perms, vec![edit_code.clone(), view_code.clone()]);
    assert_eq!(actor.team_id, Some(team_a.id.to_string()));

    // Team B context → the team-scoped grant does not apply.
    let actor = db
        .load_actor(&user.id, Some(&team_b.id))
        .await
        .expect("load")
        .expect("user");
    assert_eq!(actor.roles, vec![global_key.clone()]);
    assert_eq!(actor.team_id, Some(team_b.id.to_string()));
}

#[tokio::test]
async fn actor_missing_user_is_none() {
    let Some(db) = connect_test_db().await else {
        return;
    };
    let actor = db.load_actor(&Uuid::new_v4(), None).await.expect("load");
    assert!(actor.is_none());
}
