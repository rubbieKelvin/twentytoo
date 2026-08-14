//! End-to-end auth tests: the built app driven through real HTTP against a
//! live PostgreSQL. Without `DATABASE_URL` the tests skip (CI with no
//! database stays green); with it, the database is created and migrated on
//! first use, exactly like the `twentytoo-db` tests.

use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use axum::Router;
use axum::body::Body;
use axum::http::{HeaderMap, Request, StatusCode, header};
use http_body_util::BodyExt;
use sqlx::postgres::PgPoolOptions;
use tower::ServiceExt;
use twentytoo::Twentytoo;
use twentytoo::application::auth::{AuthConfig, BootstrapAdmin, CodeSender, hash_password};
use twentytoo_db::Db;
use twentytoo_db::entities::UserStatus;
use uuid::Uuid;

const ADMIN_PASSWORD: &str = "admin-password-1";
const SESSION_COOKIE: &str = "twentytoo_session";
const STEP_COOKIE: &str = "twentytoo_login_step";

// ---------------------------------------------------------------------------
// Harness
// ---------------------------------------------------------------------------

/// A `CodeSender` that records `(email, code)` pairs so tests can read the
/// code the user "received".
struct TestCodeSender(Arc<Mutex<Vec<(String, String)>>>);

impl TestCodeSender {
    /// The code most recently sent to `email`.
    fn code_for(&self, email: &str) -> String {
        let sent = self.0.lock().unwrap();
        let (_, code) = sent
            .iter()
            .rev()
            .find(|(sent_to, _)| return sent_to == email)
            .expect("a code was sent for this email");
        return code.clone();
    }
}

#[async_trait]
#[allow(clippy::implicit_return)]
impl CodeSender for TestCodeSender {
    async fn send(&self, email: &str, code: &str) {
        self.0
            .lock()
            .unwrap()
            .push((email.to_string(), code.to_string()));
    }
}

/// Connect to `DATABASE_URL`, creating the database on first use —
/// copied from the `twentytoo-db` test helper. Returns the URL (for the
/// builder) and the pool (for direct audit queries); migrations run when
/// the app builds.
async fn connect_test_db() -> Option<(String, Db)> {
    let url = match std::env::var("DATABASE_URL") {
        Ok(url) => url,
        Err(_) => {
            eprintln!("skipping auth tests: DATABASE_URL not set");
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
    return Some((url, db));
}

/// Create the database named in `url` via the `postgres` maintenance
/// database (name validated to alphanumerics, `_`, `-`).
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

/// The default auth config with the given bootstrap admin.
fn admin_config(email: &str) -> AuthConfig {
    return AuthConfig {
        bootstrap_admin: Some(BootstrapAdmin {
            email: email.to_string(),
            name: "Admin".to_string(),
            password: ADMIN_PASSWORD.to_string(),
        }),
        ..Default::default()
    };
}

/// Build the app with auth against `db_url`, capturing codes in `sender`.
/// The builder connects and migrates at boot.
async fn build_app(db_url: &str, config: AuthConfig, sender: Box<dyn CodeSender>) -> Router<()> {
    return Twentytoo::builder()
        .db(db_url)
        .migrate()
        .auth(config)
        .code_sender(sender)
        .build()
        .await
        .expect("app builds")
        .into_make_service();
}

/// One HTTP round trip. `cookie` is the raw `Cookie` header value.
async fn send(
    app: &Router<()>,
    method: &str,
    uri: &str,
    form: Option<&str>,
    cookie: Option<&str>,
) -> (StatusCode, String, HeaderMap) {
    let mut builder = Request::builder().method(method).uri(uri);
    if form.is_some() {
        builder = builder.header(header::CONTENT_TYPE, "application/x-www-form-urlencoded");
    }
    if let Some(cookie) = cookie {
        builder = builder.header(header::COOKIE, cookie);
    }
    let body = match form {
        Some(f) => Body::from(f.to_string()),
        None => Body::empty(),
    };
    let res = app
        .clone()
        .oneshot(builder.body(body).unwrap())
        .await
        .unwrap();
    let status = res.status();
    let headers = res.headers().clone();
    let body = res.into_body().collect().await.unwrap().to_bytes().to_vec();
    return (status, String::from_utf8(body).unwrap(), headers);
}

/// The value of a `Set-Cookie` header for `name`.
fn cookie_from(headers: &HeaderMap, name: &str) -> Option<String> {
    for value in headers.get_all(header::SET_COOKIE) {
        let raw = value.to_str().ok()?;
        for pair in raw.split(';') {
            let pair = pair.trim();
            if let Some(v) = pair.strip_prefix(&format!("{name}=")) {
                return Some(v.to_string());
            }
        }
    }
    return None;
}
/// The full `Cookie` header value for the step cookie a login-step
/// response just set.
fn step_cookie(headers: &HeaderMap) -> String {
    let value = cookie_from(headers, STEP_COOKIE).expect("step cookie set");
    return format!("{STEP_COOKIE}={value}");
}

/// The full `Cookie` header value for the session cookie a login response
/// just set.
fn session_cookie(headers: &HeaderMap) -> String {
    let value = cookie_from(headers, SESSION_COOKIE).expect("session cookie set");
    return format!("{SESSION_COOKIE}={value}");
}

/// Drive the plain (no-code) login flow: email → password → session cookie.
async fn login(app: &Router<()>, email: &str, password: &str) -> String {
    let (status, _, headers) = send(
        app,
        "POST",
        "/login/email",
        Some(&format!("email={email}")),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::SEE_OTHER, "email step redirects");
    let step = step_cookie(&headers);
    let (status, _, headers) = send(
        app,
        "POST",
        "/login/password",
        Some(&format!("password={password}")),
        Some(&step),
    )
    .await;
    assert_eq!(status, StatusCode::SEE_OTHER, "password step redirects");
    return session_cookie(&headers);
}

/// The number of audit rows for `resource` + `action`, optionally scoped to
/// an actor email.
async fn audit_count(db: &Db, resource: &str, action: &str, actor_email: Option<&str>) -> i64 {
    return match actor_email {
        Some(email) => {
            sqlx::query_scalar::<_, i64>(
                "SELECT COUNT(*) FROM audit_log WHERE resource = $1 AND action = $2 AND actor_email = $3",
            )
            .bind(resource)
            .bind(action)
            .bind(email)
            .fetch_one(db.pool())
            .await
            .expect("audit count")
        }
        None => {
            sqlx::query_scalar::<_, i64>(
                "SELECT COUNT(*) FROM audit_log WHERE resource = $1 AND action = $2",
            )
            .bind(resource)
            .bind(action)
            .fetch_one(db.pool())
            .await
            .expect("audit count")
        }
    };
}

// ---------------------------------------------------------------------------
// The flow
// ---------------------------------------------------------------------------

#[tokio::test]
async fn unauthenticated_get_redirects_to_login() {
    let Some((url, _db)) = connect_test_db().await else {
        return;
    };
    let admin = unique_email("boot");
    let app = build_app(&url, admin_config(&admin), Box::new(ConsoleSenderStub)).await;

    let (status, _, headers) = send(&app, "GET", "/", None, None).await;
    assert_eq!(status, StatusCode::SEE_OTHER);
    assert_eq!(headers.get(header::LOCATION).unwrap(), "/login");
}

/// The default code sender (auth tests never need its output; the code
/// tests use `TestCodeSender`).
struct ConsoleSenderStub;

#[async_trait]
#[allow(clippy::implicit_return)]
impl CodeSender for ConsoleSenderStub {
    async fn send(&self, email: &str, code: &str) {
        println!("[twentytoo] login code for {email}: {code}");
    }
}

#[tokio::test]
async fn email_then_password_login_sets_session_cookie() {
    let Some((url, db)) = connect_test_db().await else {
        return;
    };
    let admin = unique_email("login");
    let app = build_app(&url, admin_config(&admin), Box::new(ConsoleSenderStub)).await;

    let (status, _, headers) = send(
        &app,
        "POST",
        "/login/email",
        Some(&format!("email={admin}")),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::SEE_OTHER);
    assert_eq!(headers.get(header::LOCATION).unwrap(), "/login/password");
    let step = step_cookie(&headers);

    // The password screen renders for a valid step token.
    let (status, _, _) = send(&app, "GET", "/login/password", None, Some(&step)).await;
    assert_eq!(status, StatusCode::OK);

    let (status, _, headers) = send(
        &app,
        "POST",
        "/login/password",
        Some(&format!("password={ADMIN_PASSWORD}")),
        Some(&step),
    )
    .await;
    assert_eq!(status, StatusCode::SEE_OTHER);
    let session = session_cookie(&headers);

    // The session cookie now opens protected routes.
    let (status, body, _) = send(&app, "GET", "/", None, Some(&session)).await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("Twentytoo"));

    // And the sign-in was audited.
    let rows = audit_count(&db, "users", "login", Some(&admin)).await;
    assert!(rows >= 1, "login audit row for {admin}");
}

#[tokio::test]
async fn wrong_password_rerenders_without_session() {
    let Some((url, _db)) = connect_test_db().await else {
        return;
    };
    let admin = unique_email("wrongpw");
    let app = build_app(&url, admin_config(&admin), Box::new(ConsoleSenderStub)).await;

    let (status, _, headers) = send(
        &app,
        "POST",
        "/login/email",
        Some(&format!("email={admin}")),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::SEE_OTHER);
    let step = step_cookie(&headers);

    let (status, body, headers) = send(
        &app,
        "POST",
        "/login/password",
        Some("password=definitely-wrong"),
        Some(&step),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert!(body.contains("Incorrect password."));
    assert!(cookie_from(&headers, SESSION_COOKIE).is_none());
}

#[tokio::test]
async fn unknown_email_without_domain_access_fails() {
    let Some((url, db)) = connect_test_db().await else {
        return;
    };
    let admin = unique_email("noaccount");
    let app = build_app(&url, admin_config(&admin), Box::new(ConsoleSenderStub)).await;

    let unknown = unique_email("nobody");
    let (status, body, _) = send(
        &app,
        "POST",
        "/login/email",
        Some(&format!("email={unknown}")),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("No account found with that email address."));

    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM users WHERE email = $1")
        .bind(&unknown)
        .fetch_one(db.pool())
        .await
        .expect("user count");
    assert_eq!(count, 0, "no user row for the unknown email");
}

#[tokio::test]
async fn domain_access_creates_account_by_login_attempt() {
    let Some((url, db)) = connect_test_db().await else {
        return;
    };
    let admin = unique_email("domainadmin");
    let config = AuthConfig {
        allowed_domains: vec!["example.com".to_string()],
        ..admin_config(&admin)
    };
    let app = build_app(&url, config, Box::new(ConsoleSenderStub)).await;

    let local = format!("domain-{}", Uuid::new_v4().simple());
    let email = format!("{local}@example.com");
    let (status, _, headers) = send(
        &app,
        "POST",
        "/login/email",
        Some(&format!("email={email}")),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::SEE_OTHER);
    let step = step_cookie(&headers);

    let (status, _, headers) = send(
        &app,
        "POST",
        "/login/password",
        Some("password=password123"),
        Some(&step),
    )
    .await;
    assert_eq!(status, StatusCode::SEE_OTHER);
    assert!(cookie_from(&headers, SESSION_COOKIE).is_some());

    // The account exists: name from the local part, created via the login
    // attempt, and audited as a create.
    let user = db
        .get_user_by_email(&email)
        .await
        .expect("user lookup")
        .expect("account created");
    assert_eq!(user.name, local);
    assert_eq!(user.status, UserStatus::Active);
    let rows = audit_count(&db, "users", "create", Some(&email)).await;
    assert!(rows >= 1, "create audit row for {email}");
}

#[tokio::test]
async fn email_confirmation_requires_code_before_password() {
    let Some((url, _db)) = connect_test_db().await else {
        return;
    };
    let admin = unique_email("codeadmin");
    let sender = Arc::new(Mutex::new(Vec::new()));
    let config = AuthConfig {
        email_confirmation: true,
        ..admin_config(&admin)
    };
    let app = build_app(&url, config, Box::new(TestCodeSender(sender.clone()))).await;

    let (status, _, headers) = send(
        &app,
        "POST",
        "/login/email",
        Some(&format!("email={admin}")),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::SEE_OTHER);
    assert_eq!(headers.get(header::LOCATION).unwrap(), "/login/code");
    let step = step_cookie(&headers);

    // The code screen renders for the browser's GET after the 303.
    let (status, _, _) = send(&app, "GET", "/login/code", None, Some(&step)).await;
    assert_eq!(status, StatusCode::OK);
    // Skipping the code step is refused: the email_ok token cannot open a
    // session, and the flow bounces back to /login.
    let (status, _, headers) = send(
        &app,
        "POST",
        "/login/password",
        Some(&format!("password={ADMIN_PASSWORD}")),
        Some(&step),
    )
    .await;
    assert_eq!(status, StatusCode::SEE_OTHER);
    assert_eq!(headers.get(header::LOCATION).unwrap(), "/login");
    assert!(cookie_from(&headers, SESSION_COOKIE).is_none());

    // The correct code exchanges the token for a password-step token.
    let code = TestCodeSender(sender.clone()).code_for(&admin);
    let (status, _, headers) = send(
        &app,
        "POST",
        "/login/code",
        Some(&format!("code={code}")),
        Some(&step),
    )
    .await;
    assert_eq!(status, StatusCode::SEE_OTHER);
    assert_eq!(headers.get(header::LOCATION).unwrap(), "/login/password");
    let step = step_cookie(&headers);

    let (status, _, headers) = send(
        &app,
        "POST",
        "/login/password",
        Some(&format!("password={ADMIN_PASSWORD}")),
        Some(&step),
    )
    .await;
    assert_eq!(status, StatusCode::SEE_OTHER);
    assert!(cookie_from(&headers, SESSION_COOKIE).is_some());
}

#[tokio::test]
async fn wrong_code_locks_token_after_five_attempts() {
    let Some((url, _db)) = connect_test_db().await else {
        return;
    };
    let admin = unique_email("lockadmin");
    let sender = Arc::new(Mutex::new(Vec::new()));
    let config = AuthConfig {
        email_confirmation: true,
        ..admin_config(&admin)
    };
    let app = build_app(&url, config, Box::new(TestCodeSender(sender.clone()))).await;

    let (status, _, headers) = send(
        &app,
        "POST",
        "/login/email",
        Some(&format!("email={admin}")),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::SEE_OTHER);
    let step = step_cookie(&headers);

    let code = TestCodeSender(sender.clone()).code_for(&admin);
    let wrong = if code == "000000" { "000001" } else { "000000" };
    for attempt in 1..=5 {
        let (status, body, _) = send(
            &app,
            "POST",
            "/login/code",
            Some(&format!("code={wrong}")),
            Some(&step),
        )
        .await;
        assert_eq!(status, StatusCode::UNAUTHORIZED, "attempt {attempt}");
        if attempt < 5 {
            assert!(body.contains("Incorrect code."));
        } else {
            assert!(body.contains("Too many attempts. Start over."));
        }
    }

    // The token is consumed; even the right code must restart the flow.
    let (status, _, headers) = send(
        &app,
        "POST",
        "/login/code",
        Some(&format!("code={code}")),
        Some(&step),
    )
    .await;
    assert_eq!(status, StatusCode::SEE_OTHER);
    assert_eq!(headers.get(header::LOCATION).unwrap(), "/login");
}

#[tokio::test]
async fn admin_creates_user_with_audit_entry() {
    let Some((url, db)) = connect_test_db().await else {
        return;
    };
    let admin = unique_email("createadmin");
    let app = build_app(&url, admin_config(&admin), Box::new(ConsoleSenderStub)).await;
    let session = login(&app, &admin, ADMIN_PASSWORD).await;

    let new_email = unique_email("created");
    let (status, _, headers) = send(
        &app,
        "POST",
        "/users/new",
        Some(&format!(
            "email={new_email}&name=New%20User&password=password123"
        )),
        Some(&session),
    )
    .await;
    assert_eq!(status, StatusCode::SEE_OTHER, "create redirects");
    let location = headers
        .get(header::LOCATION)
        .expect("redirect location")
        .to_str()
        .unwrap()
        .to_string();
    assert!(
        location.starts_with("/users/"),
        "to the new user: {location}"
    );

    let user = db
        .get_user_by_email(&new_email)
        .await
        .expect("lookup")
        .expect("user exists");
    assert_eq!(user.name, "New User");
    assert!(user.password_hash.is_some());

    let rows = audit_count(&db, "users", "create", Some(&admin)).await;
    assert!(rows >= 1, "create audit row attributed to {admin}");
}

#[tokio::test]
async fn user_creation_requires_users_create_permission() {
    let Some((url, db)) = connect_test_db().await else {
        return;
    };
    let admin = unique_email("permadmin");
    let app = build_app(&url, admin_config(&admin), Box::new(ConsoleSenderStub)).await;

    // A second account with no roles: may log in, may not create users.
    let plain = unique_email("plain");
    let hash = hash_password("plain-password-1").expect("hash");
    db.create_user(&plain, "Plain", Some(&hash))
        .await
        .expect("create plain user");
    let session = login(&app, &plain, "plain-password-1").await;

    let (status, _, _) = send(
        &app,
        "POST",
        "/users/new",
        Some(&format!(
            "email={}&name=X&password=password123",
            unique_email("denied")
        )),
        Some(&session),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn logout_clears_session() {
    let Some((url, db)) = connect_test_db().await else {
        return;
    };
    let admin = unique_email("logoutadmin");
    let app = build_app(&url, admin_config(&admin), Box::new(ConsoleSenderStub)).await;
    let session = login(&app, &admin, ADMIN_PASSWORD).await;

    let (status, _, headers) = send(&app, "POST", "/logout", None, Some(&session)).await;
    assert_eq!(status, StatusCode::SEE_OTHER);
    assert_eq!(headers.get(header::LOCATION).unwrap(), "/login");
    // The session cookie is cleared and the old token no longer opens
    // protected routes.
    let cleared = cookie_from(&headers, SESSION_COOKIE).expect("session cookie cleared");
    assert!(cleared.is_empty());

    let (status, _, headers) = send(&app, "GET", "/", None, Some(&session)).await;
    assert_eq!(status, StatusCode::SEE_OTHER);
    assert_eq!(headers.get(header::LOCATION).unwrap(), "/login");

    // The sign-out was audited.
    let rows = audit_count(&db, "users", "logout", Some(&admin)).await;
    assert!(rows >= 1, "logout audit row for {admin}");
}

#[tokio::test]
async fn self_disable_is_rejected() {
    let Some((url, db)) = connect_test_db().await else {
        return;
    };
    let admin = unique_email("selfadmin");
    let app = build_app(&url, admin_config(&admin), Box::new(ConsoleSenderStub)).await;
    let session = login(&app, &admin, ADMIN_PASSWORD).await;

    let user = db
        .get_user_by_email(&admin)
        .await
        .expect("lookup")
        .expect("admin exists");

    let (status, body, _) = send(
        &app,
        "POST",
        &format!("/users/{}", user.id),
        Some("name=Admin&status=disabled&password="),
        Some(&session),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(body.contains("You cannot disable your own account."));

    let user = db
        .get_user(&user.id)
        .await
        .expect("lookup")
        .expect("exists");
    assert_eq!(user.status, UserStatus::Active, "status unchanged");
}

#[tokio::test]
async fn bootstrap_is_idempotent() {
    let Some((url, db)) = connect_test_db().await else {
        return;
    };
    let admin = unique_email("twiceadmin");
    let config = admin_config(&admin);

    let first = build_app(&url, config.clone(), Box::new(ConsoleSenderStub)).await;
    drop(first);
    // A second boot against the same database succeeds without duplicating
    // the framework rows.
    let second = build_app(&url, config, Box::new(ConsoleSenderStub)).await;
    drop(second);

    for code in ["users.view", "users.create", "users.update"] {
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM permissions WHERE code = $1")
            .bind(code)
            .fetch_one(db.pool())
            .await
            .expect("permission count");
        assert_eq!(count, 1, "one {code} permission");
    }
    let roles: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM roles WHERE key = 'admin'")
        .fetch_one(db.pool())
        .await
        .expect("role count");
    assert_eq!(roles, 1, "one admin role");
    let users: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM users WHERE email = $1")
        .bind(&admin)
        .fetch_one(db.pool())
        .await
        .expect("user count");
    assert_eq!(users, 1, "one admin user");
}
