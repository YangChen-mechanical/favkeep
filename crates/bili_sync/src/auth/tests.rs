use super::*;
use axum::{
    Extension, Router,
    http::{HeaderMap, StatusCode, header},
};
use axum::{
    body::{Body, to_bytes},
    http::Request,
};
use bili_sync_migration::{Migrator, MigratorTrait};
use sea_orm::{ConnectionTrait, Database};
use serde_json::{Value, json};
use std::{sync::Arc, time::Duration};
use tower::ServiceExt;

const ORIGIN: &str = "http://127.0.0.1:24188";
const PASSWORD: &str = "correct horse battery staple";

async fn fixture() -> (Router, Arc<AuthState>, tempfile::TempDir) {
    let db = Database::connect("sqlite::memory:").await.unwrap();
    Migrator::up(&db, None).await.unwrap();
    let dir = tempfile::tempdir().unwrap();
    let state = AuthState::new(db.clone(), ORIGIN, dir.path().join("setup.secret"), true)
        .await
        .unwrap();
    let app = crate::api::router()
        .layer(Extension(db))
        .layer(Extension(state.clone()))
        .layer(session_layer(&state, true));
    (app, state, dir)
}
async fn call(
    app: &Router,
    method: &str,
    url: &str,
    body: Value,
    cookie: Option<&str>,
    csrf: Option<&str>,
    origin: Option<&str>,
) -> (StatusCode, HeaderMap, Value) {
    let mut req = Request::builder()
        .method(method)
        .uri(url)
        .header(header::CONTENT_TYPE, "application/json");
    if let Some(v) = cookie {
        req = req.header(header::COOKIE, v);
    }
    if let Some(v) = csrf {
        req = req.header("x-csrf-token", v);
    }
    if let Some(v) = origin {
        req = req.header(header::ORIGIN, v);
    }
    let response = app
        .clone()
        .oneshot(req.body(Body::from(body.to_string())).unwrap())
        .await
        .unwrap();
    let status = response.status();
    let headers = response.headers().clone();
    let bytes = to_bytes(response.into_body(), 1_000_000).await.unwrap();
    (status, headers, serde_json::from_slice(&bytes).unwrap_or(Value::Null))
}
fn cookie(headers: &HeaderMap) -> String {
    headers[header::SET_COOKIE]
        .to_str()
        .unwrap()
        .split(';')
        .next()
        .unwrap()
        .into()
}
async fn initialize(app: &Router, state: &AuthState) -> String {
    let code = tokio::fs::read_to_string(&state.setup_path).await.unwrap();
    let (status, headers, _) = call(
        app,
        "POST",
        "/api/auth/setup",
        json!({"setup_code":code,"username":"admin","password":PASSWORD}),
        None,
        None,
        Some(ORIGIN),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    cookie(&headers)
}
async fn csrf(app: &Router, cookie: &str) -> String {
    let (status, _, value) = call(app, "GET", "/api/auth/session", Value::Null, Some(cookie), None, None).await;
    assert_eq!(status, StatusCode::OK);
    value["data"]["csrf_token"].as_str().unwrap().into()
}

#[tokio::test]
async fn setup_login_csrf_logout_password_and_legacy_bypass() {
    let (app, state, _dir) = fixture().await;
    for path in [
        "/api/videos",
        "/api/config",
        "/api/platforms",
        "/api/media/pages/999",
        "/api/media/videos/999/cover",
        "/api/ws",
    ] {
        assert_eq!(
            call(&app, "GET", path, Value::Null, None, None, None).await.0,
            StatusCode::UNAUTHORIZED
        );
    }
    let (_, _, value) = call(&app, "GET", "/api/auth/session", Value::Null, None, None, None).await;
    assert_eq!(value["data"]["setup_required"], true);
    assert_eq!(
        call(
            &app,
            "POST",
            "/api/auth/setup",
            json!({"setup_code":"bad","username":"admin","password":PASSWORD}),
            None,
            None,
            Some(ORIGIN)
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    let first = initialize(&app, &state).await;
    assert!(!state.setup_path.exists());
    assert_eq!(
        call(
            &app,
            "POST",
            "/api/auth/setup",
            json!({"setup_code":"bad","username":"attacker","password":PASSWORD}),
            None,
            None,
            Some(ORIGIN)
        )
        .await
        .0,
        StatusCode::CONFLICT
    );
    let token = csrf(&app, &first).await;
    assert_eq!(
        call(
            &app,
            "GET",
            "/api/media/pages/999",
            Value::Null,
            Some(&first),
            None,
            None
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        call(&app, "HEAD", "/api/media/pages/999", Value::Null, None, None, None)
            .await
            .0,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        call(
            &app,
            "POST",
            "/api/auth/logout",
            Value::Null,
            Some(&first),
            None,
            Some(ORIGIN)
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        call(
            &app,
            "POST",
            "/api/auth/logout",
            Value::Null,
            Some(&first),
            Some(&token),
            Some("https://evil.example")
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        call(
            &app,
            "POST",
            "/api/auth/logout",
            Value::Null,
            Some(&first),
            Some(&token),
            None
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        call(
            &app,
            "POST",
            "/api/auth/login",
            json!({"username":"admin","password":"wrong"}),
            None,
            None,
            Some(ORIGIN)
        )
        .await
        .0,
        StatusCode::UNAUTHORIZED
    );
    let (status, headers, _) = call(
        &app,
        "POST",
        "/api/auth/login",
        json!({"username":"admin","password":PASSWORD}),
        None,
        None,
        Some(ORIGIN),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let second = cookie(&headers);
    assert_ne!(first, second);
    let other_csrf = csrf(&app, &second).await;
    let (status, headers, _) = call(
        &app,
        "PUT",
        "/api/auth/password",
        json!({"current_password":PASSWORD,"new_password":"changed password for testing"}),
        Some(&second),
        Some(&other_csrf),
        Some(ORIGIN),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let rotated = cookie(&headers);
    assert_ne!(rotated, second);
    for old in [&first, &second] {
        assert_eq!(
            call(&app, "GET", "/api/config", Value::Null, Some(old), None, None)
                .await
                .0,
            StatusCode::UNAUTHORIZED
        );
    }
    let token = csrf(&app, &rotated).await;
    assert_eq!(
        call(
            &app,
            "POST",
            "/api/auth/logout",
            Value::Null,
            Some(&rotated),
            Some(&token),
            Some(ORIGIN)
        )
        .await
        .0,
        StatusCode::OK
    );
    assert_eq!(
        call(&app, "GET", "/api/config", Value::Null, Some(&rotated), None, None)
            .await
            .0,
        StatusCode::UNAUTHORIZED
    );
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/config")
                .header(
                    header::AUTHORIZATION,
                    crate::config::VersionedConfig::get().read().auth_token.clone(),
                )
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    let (status, _, _) = call(&app, "GET", "/api/media-session", Value::Null, None, None, None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn session_hash_persistence_expiry_and_no_resurrection() {
    use tower_sessions::session_store::SessionStore;
    let (app, state, _dir) = fixture().await;
    let login = initialize(&app, &state).await;
    let raw = login.split_once('=').unwrap().1;
    let id: tower_sessions::session::Id = raw.parse().unwrap();
    let store = SqliteSessionStore(state.db.clone());
    let record = store.load(&id).await.unwrap().unwrap();
    let hash = session_hash(&id);
    let row = state
        .db
        .query_one(statement("SELECT session_hash,data FROM app_session", vec![]))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(row.try_get::<String>("", "session_hash").unwrap(), hash);
    assert!(!row.try_get::<String>("", "data").unwrap().contains(raw));
    assert!(store.load(&id).await.unwrap().is_some());
    store.delete(&id).await.unwrap();
    store.save(&record).await.unwrap();
    assert!(store.load(&id).await.unwrap().is_none());
    let mut expired = record.clone();
    expired.id = Default::default();
    expired.expiry_date = cookie::time::OffsetDateTime::now_utc() - cookie::time::Duration::seconds(1);
    store.create(&mut expired).await.unwrap();
    assert!(store.load(&expired.id).await.unwrap().is_none());
    let mut old = record.clone();
    old.id = Default::default();
    let mut meta: IdentitySession = serde_json::from_value(old.data["identity"].clone()).unwrap();
    meta.created_at = now() - MAX_SECONDS - 1;
    old.data.insert("identity".into(), serde_json::to_value(meta).unwrap());
    store.create(&mut old).await.unwrap();
    assert!(store.load(&old.id).await.unwrap().is_none());
}

#[tokio::test]
async fn login_rate_limit_and_parallel_setup() {
    let (app, state, _dir) = fixture().await;
    let code = tokio::fs::read_to_string(&state.setup_path).await.unwrap();
    let input = json!({"setup_code":code,"username":"admin","password":PASSWORD});
    let (a, b) = tokio::join!(
        call(&app, "POST", "/api/auth/setup", input.clone(), None, None, Some(ORIGIN)),
        call(&app, "POST", "/api/auth/setup", input, None, None, Some(ORIGIN))
    );
    assert!([a.0, b.0].contains(&StatusCode::OK));
    assert!([a.0, b.0].contains(&StatusCode::CONFLICT));
    for _ in 0..5 {
        assert_eq!(
            call(
                &app,
                "POST",
                "/api/auth/login",
                json!({"username":"admin","password":"bad"}),
                None,
                None,
                Some(ORIGIN)
            )
            .await
            .0,
            StatusCode::UNAUTHORIZED
        );
    }
    let (status, headers, _) = call(
        &app,
        "POST",
        "/api/auth/login",
        json!({"username":"admin","password":PASSWORD}),
        None,
        None,
        Some(ORIGIN),
    )
    .await;
    assert_eq!(status, StatusCode::TOO_MANY_REQUESTS);
    assert!(headers.contains_key(header::RETRY_AFTER));
}

#[tokio::test]
async fn secure_cookie_origin_and_socket_revocation() {
    let (_, state, _dir) = fixture().await;
    let app = crate::api::router()
        .layer(Extension(state.db.clone()))
        .layer(Extension(state.clone()))
        .layer(session_layer(&state, false));
    let code = tokio::fs::read_to_string(&state.setup_path).await.unwrap();
    let (status, headers, _) = call(
        &app,
        "POST",
        "/api/auth/setup",
        json!({"setup_code":code,"username":"admin","password":PASSWORD}),
        None,
        None,
        Some(ORIGIN),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let header = headers[header::SET_COOKIE].to_str().unwrap();
    for value in [
        "__Host-favkeep_session=",
        "Secure",
        "HttpOnly",
        "SameSite=Lax",
        "Path=/",
    ] {
        assert!(header.contains(value));
    }
    assert!(!header.contains("Domain="));
    let login = cookie(&headers);
    let csrf = csrf(&app, &login).await;
    for origin in [None, Some("https://evil.example")] {
        let mut request = Request::builder()
            .uri("/api/ws")
            .header(header::COOKIE, &login)
            .header(header::UPGRADE, "websocket");
        if let Some(origin) = origin {
            request = request.header(header::ORIGIN, origin);
        }
        let response = app.clone().oneshot(request.body(Body::empty()).unwrap()).await.unwrap();
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }
    assert_eq!(
        call(
            &app,
            "POST",
            "/api/media-session",
            Value::Null,
            Some(&login),
            Some(&csrf),
            Some(ORIGIN)
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        call(
            &app,
            "GET",
            "/api/login/qrcode/poll?qrcode_key=legacy",
            Value::Null,
            Some(&login),
            None,
            None
        )
        .await
        .0,
        StatusCode::METHOD_NOT_ALLOWED
    );
    let id: tower_sessions::session::Id = login.split_once('=').unwrap().1.parse().unwrap();
    let hash = session_hash(&id);
    let watcher_state = state.clone();
    let watcher = tokio::spawn(async move { watcher_state.wait_revoked(&hash).await });
    assert_eq!(
        call(
            &app,
            "POST",
            "/api/auth/logout",
            Value::Null,
            Some(&login),
            Some(&csrf),
            Some(ORIGIN)
        )
        .await
        .0,
        StatusCode::OK
    );
    tokio::time::timeout(Duration::from_secs(1), watcher)
        .await
        .unwrap()
        .unwrap();
}

#[tokio::test]
async fn xiaohongshu_routes_are_protected_and_status_reads_do_not_create_records() {
    let (app, state, _dir) = fixture().await;
    for path in ["/api/xiaohongshu/status", "/api/video-sources/xiaohongshu"] {
        assert_eq!(
            call(&app, "GET", path, Value::Null, None, None, None).await.0,
            StatusCode::UNAUTHORIZED
        );
    }
    let login = initialize(&app, &state).await;
    for path in ["/api/xiaohongshu/status", "/api/video-sources/xiaohongshu"] {
        assert_eq!(
            call(&app, "GET", path, Value::Null, Some(&login), None, None).await.0,
            StatusCode::OK
        );
    }
    for (method, path) in [
        ("POST", "/api/xiaohongshu/resume"),
        ("POST", "/api/task/xiaohongshu"),
        ("PUT", "/api/config/xiaohongshu"),
        ("PUT", "/api/video-sources/xiaohongshu"),
    ] {
        assert_eq!(
            call(&app, method, path, json!({}), Some(&login), None, Some(ORIGIN))
                .await
                .0,
            StatusCode::FORBIDDEN
        );
    }
    let row = state
        .db
        .query_one(statement(
            "SELECT (SELECT COUNT(*) FROM xiaohongshu_account)+(SELECT COUNT(*) FROM xiaohongshu_source) AS n",
            vec![],
        ))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(row.try_get::<i64>("", "n").unwrap(), 0);
    let (_, _, catalog) = call(&app, "GET", "/api/platforms", Value::Null, Some(&login), None, None).await;
    assert_eq!(catalog["data"].as_array().unwrap().len(), 3);
    assert!(
        catalog["data"]
            .as_array()
            .unwrap()
            .iter()
            .any(|p| p["id"] == "xiaohongshu")
    );
}
