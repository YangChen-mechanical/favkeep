use std::sync::Arc;

use axum::{
    Extension, Json, Router,
    extract::Path,
    routing::{get, post, put},
};
use sea_orm::DatabaseConnection;
use serde::Deserialize;
use serde_json::{Value, json};
use validator::Validate;

use crate::{
    api::wrapper::{ApiError, ApiResponse},
    bilibili::{BiliClient, Credential},
    config::{Config, VersionedConfig},
    notifier::{Message, Notifier},
};

pub(super) fn router() -> Router {
    Router::new()
        .route("/config", get(get_config).put(update_config))
        .route("/config/credential", put(update_credential))
        .route("/config/notifiers", get(get_notifiers).post(create_notifier))
        .route(
            "/config/notifiers/{index}",
            put(update_notifier).delete(delete_notifier),
        )
        .route("/config/notifiers/ping", post(ping_notifiers))
}

fn bad(message: &str) -> ApiError {
    crate::api::error::InnerApiError::BadRequest(message.into()).into()
}

fn public_config(config: &Config) -> Value {
    let mut value = serde_json::to_value(config).expect("Config serialization");
    let object = value.as_object_mut().unwrap();
    for key in ["auth_token", "credential", "notifiers"] {
        object.remove(key);
    }
    object.insert(
        "credential_configured".into(),
        Value::Bool(credential_complete(&config.credential)),
    );
    value
}

fn credential_complete(value: &Credential) -> bool {
    [
        &value.sessdata,
        &value.bili_jct,
        &value.buvid3,
        &value.dedeuserid,
        &value.ac_time_value,
    ]
    .iter()
    .all(|v| !v.trim().is_empty())
}

pub async fn get_config() -> Result<ApiResponse<Value>, ApiError> {
    Ok(ApiResponse::ok(public_config(&VersionedConfig::get().read())))
}

pub async fn update_config(
    Extension(db): Extension<DatabaseConnection>,
    Json(input): Json<Value>,
) -> Result<ApiResponse<Value>, ApiError> {
    let config = VersionedConfig::get();
    let _lock = config.update_lock.lock().await;
    let current = config.snapshot();
    let next = merge_public(&current, input)?;
    let saved = config.update_locked(next, &db).await?;
    Ok(ApiResponse::ok(public_config(&saved)))
}

fn merge_public(current: &Config, mut input: Value) -> Result<Config, ApiError> {
    let fields = input.as_object_mut().ok_or_else(|| bad("配置必须是对象"))?;
    fields.remove("credential_configured");
    let allowed = public_config(current);
    if fields.keys().any(|key| !allowed.as_object().unwrap().contains_key(key)) {
        return Err(bad("普通配置不能写入凭据或未知字段"));
    }
    if !fields.contains_key("version") {
        return Err(bad("缺少配置版本，请刷新页面"));
    }
    let mut value = serde_json::to_value(current)?;
    value.as_object_mut().unwrap().extend(fields.clone());
    let next: Config = serde_json::from_value(value).map_err(|_| bad("配置格式无效"))?;
    next.validate().map_err(|_| bad("配置校验失败"))?;
    next.check()?;
    Ok(next)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CredentialUpdate {
    version: u64,
    credential: Credential,
}

async fn update_credential(
    Extension(db): Extension<DatabaseConnection>,
    Json(input): Json<CredentialUpdate>,
) -> Result<ApiResponse<Value>, ApiError> {
    if !credential_complete(&input.credential) {
        return Err(bad("请完整填写 B 站凭据"));
    }
    let config = VersionedConfig::get();
    let _lock = config.update_lock.lock().await;
    let mut next = config.read().as_ref().clone();
    next.version = input.version;
    next.credential = input.credential;
    let saved = config.update_locked(next, &db).await?;
    Ok(ApiResponse::ok(public_config(&saved)))
}

fn secret_fields(kind: &str) -> &'static [&'static str] {
    match kind {
        "telegram" => &["bot_token"],
        "smtp" => &["password"],
        "webhook" => &["url", "headers"],
        _ => &[],
    }
}

fn public_notifier(notifier: &Notifier, index: usize) -> Value {
    let mut value = serde_json::to_value(notifier).unwrap();
    let fields = secret_fields(value["type"].as_str().unwrap());
    let object = value.as_object_mut().unwrap();
    let mut configured = serde_json::Map::new();
    for key in fields {
        let present = object
            .remove(*key)
            .is_some_and(|v| !v.is_null() && v != "" && v != json!({}));
        configured.insert((*key).into(), Value::Bool(present));
    }
    object.insert("configured".into(), Value::Object(configured));
    object.insert("index".into(), json!(index));
    value
}

fn notifier_response(config: &Config) -> Value {
    json!({"version":config.version,"items":config.notifiers.as_deref().map(|items| items.iter().enumerate().map(|(i,n)| public_notifier(n,i)).collect::<Vec<_>>()).unwrap_or_default()})
}

async fn get_notifiers() -> ApiResponse<Value> {
    ApiResponse::ok(notifier_response(&VersionedConfig::get().read()))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct NotifierUpdate {
    version: u64,
    notifier: Value,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Version {
    version: u64,
}

fn merge_notifier(mut input: Value, old: Option<&Notifier>) -> Result<Notifier, ApiError> {
    let object = input.as_object_mut().ok_or_else(|| bad("通知器必须是对象"))?;
    object.remove("configured");
    object.remove("index");
    let old = old.map(|v| serde_json::to_value(v).unwrap());
    if let Some(old) = old
        .as_ref()
        .filter(|old| old["type"] == object.get("type").cloned().unwrap_or(Value::Null))
    {
        let kind = old["type"].as_str().unwrap();
        for field in secret_fields(kind) {
            if !object.contains_key(*field) {
                object.insert((*field).into(), old[*field].clone());
            }
        }
    }
    let notifier: Notifier = serde_json::from_value(input).map_err(|_| bad("通知器字段不完整或格式无效"))?;
    match &notifier {
        Notifier::Telegram { bot_token, chat_id, .. } if bot_token.trim().is_empty() || chat_id.trim().is_empty() => {
            return Err(bad("Telegram Token 和 Chat ID 不能为空"));
        }
        Notifier::Webhook { url, .. } => {
            let parsed = reqwest::Url::parse(url).map_err(|_| bad("Webhook URL 无效"))?;
            if !matches!(parsed.scheme(), "http" | "https") {
                return Err(bad("Webhook URL 必须使用 HTTP 或 HTTPS"));
            }
        }
        Notifier::Smtp {
            port, host, from, to, ..
        } if *port == 0 || host.trim().is_empty() || from.trim().is_empty() || to.trim().is_empty() => {
            return Err(bad("SMTP 配置不完整"));
        }
        _ => {}
    }
    Ok(notifier)
}

async fn save_notifier(
    db: DatabaseConnection,
    version: u64,
    index: Option<usize>,
    input: Option<Value>,
) -> Result<ApiResponse<Value>, ApiError> {
    let config = VersionedConfig::get();
    let _lock = config.update_lock.lock().await;
    let mut next = config.read().as_ref().clone();
    let mut items = next.notifiers.as_deref().cloned().unwrap_or_default();
    if let Some(index) = index {
        let old = items.get(index).ok_or_else(|| bad("通知器不存在，请刷新页面"))?;
        if let Some(input) = input {
            items[index] = merge_notifier(input, Some(old))?;
        } else {
            items.remove(index);
        }
    } else {
        items.push(merge_notifier(input.unwrap(), None)?);
    }
    next.version = version;
    next.notifiers = Some(Arc::new(items));
    let saved = config.update_locked(next, &db).await?;
    Ok(ApiResponse::ok(notifier_response(&saved)))
}

async fn create_notifier(
    Extension(db): Extension<DatabaseConnection>,
    Json(input): Json<NotifierUpdate>,
) -> Result<ApiResponse<Value>, ApiError> {
    save_notifier(db, input.version, None, Some(input.notifier)).await
}
async fn update_notifier(
    Extension(db): Extension<DatabaseConnection>,
    Path(index): Path<usize>,
    Json(input): Json<NotifierUpdate>,
) -> Result<ApiResponse<Value>, ApiError> {
    save_notifier(db, input.version, Some(index), Some(input.notifier)).await
}
async fn delete_notifier(
    Extension(db): Extension<DatabaseConnection>,
    Path(index): Path<usize>,
    Json(input): Json<Version>,
) -> Result<ApiResponse<Value>, ApiError> {
    save_notifier(db, input.version, Some(index), None).await
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Ping {
    version: u64,
    index: usize,
}

async fn ping_notifiers(
    Extension(bili_client): Extension<Arc<BiliClient>>,
    Json(input): Json<Ping>,
) -> Result<ApiResponse<()>, ApiError> {
    let current = VersionedConfig::get().snapshot();
    if input.version != current.version {
        return Err(bad("配置版本不匹配，请刷新页面"));
    }
    let mut notifier = current
        .notifiers
        .as_deref()
        .and_then(|items| items.get(input.index))
        .cloned()
        .ok_or_else(|| bad("通知器不存在"))?;
    if let Notifier::Webhook { ignore_cache, .. } = &mut notifier {
        *ignore_cache = Some(());
    }
    notifier
        .notify(
            bili_client.inner_client(),
            Message {
                message: "This is a test notification from Favkeep.".into(),
                image_url: None,
            },
        )
        .await
        .map_err(|_| bad("测试通知发送失败，请检查服务器连接和通知器设置"))?;
    Ok(ApiResponse::ok(()))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn secrets_are_omitted_and_partial_updates_preserve_them() {
        let mut config = Config::default();
        config.credential = Credential {
            sessdata: "SESSDATA_SECRET".into(),
            bili_jct: "CSRF_SECRET".into(),
            buvid3: "BUVID_SECRET".into(),
            dedeuserid: "USER_SECRET".into(),
            ac_time_value: "REFRESH_SECRET".into(),
        };
        let value = public_config(&config).to_string();
        let changed = merge_public(&config, json!({"version":config.version,"video_name":"changed"})).ok().unwrap();
        assert_eq!(changed.credential.sessdata, "SESSDATA_SECRET");
        assert_eq!(changed.auth_token, config.auth_token);
        assert_eq!(changed.video_name, "changed");
        assert!(merge_public(&config, json!({"version":config.version,"credential":{}})).is_err());
        assert!(merge_public(&config, json!({"version":config.version,"auth_token":"bypass"})).is_err());
        assert!(merge_public(&config, json!({"video_name":"missing-version"})).is_err());
        for secret in [
            "SESSDATA_SECRET",
            "CSRF_SECRET",
            "BUVID_SECRET",
            "USER_SECRET",
            "REFRESH_SECRET",
            &config.auth_token,
        ] {
            assert!(!value.contains(secret));
        }
        let old = Notifier::Webhook {
            url: "https://example.com/PRIVATE_TOKEN".into(),
            template: None,
            headers: Some([(String::from("Authorization"), String::from("HEADER_SECRET"))].into()),
            ignore_cache: None,
        };
        let public = public_notifier(&old, 0).to_string();
        assert!(!public.contains("PRIVATE_TOKEN"));
        assert!(!public.contains("HEADER_SECRET"));
        let updated = merge_notifier(json!({"type":"webhook","template":"new"}), Some(&old))
            .ok()
            .unwrap();
        let value = serde_json::to_value(updated).unwrap();
        assert_eq!(value["url"], "https://example.com/PRIVATE_TOKEN");
        assert_eq!(value["headers"]["Authorization"], "HEADER_SECRET");
        let cleared = merge_notifier(json!({"type":"webhook","headers":null}), Some(&old)).ok().unwrap();
        assert!(serde_json::to_value(cleared).unwrap()["headers"].is_null());
    }
}
