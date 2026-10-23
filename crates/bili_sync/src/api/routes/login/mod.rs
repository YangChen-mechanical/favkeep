use crate::{
    api::{
        error::InnerApiError,
        request::PollQrcodeRequest,
        response::GenerateQrcodeResponse,
        wrapper::{ApiError, ApiResponse},
    },
    bilibili::{BiliClient, Credential, PollStatus},
    config::VersionedConfig,
};
use axum::{Extension, Json, Router, routing::post};
use sea_orm::DatabaseConnection;
use serde_json::{Value, json};
use std::sync::Arc;
use tower_sessions::Session;

pub(super) fn router() -> Router {
    Router::new()
        .route("/login/qrcode/generate", post(generate_qrcode))
        .route("/login/qrcode/poll", post(poll_qrcode))
}

pub async fn generate_qrcode(
    Extension(client): Extension<Arc<BiliClient>>,
    session: Session,
) -> Result<ApiResponse<GenerateQrcodeResponse>, ApiError> {
    let qrcode = Credential::generate_qrcode(&client.client)
        .await
        .map_err(|_| InnerApiError::BadRequest("二维码生成失败，请稍后重试".into()))?;
    session.insert("bili_qrcode", qrcode.qrcode_key.clone()).await?;
    Ok(ApiResponse::ok(qrcode))
}

pub async fn poll_qrcode(
    Extension(client): Extension<Arc<BiliClient>>,
    Extension(db): Extension<DatabaseConnection>,
    session: Session,
    Json(params): Json<PollQrcodeRequest>,
) -> Result<ApiResponse<Value>, ApiError> {
    if session.get::<String>("bili_qrcode").await?.as_deref() != Some(params.qrcode_key.as_str()) {
        return Err(InnerApiError::BadRequest("二维码不属于当前登录会话，请重新生成".into()).into());
    }
    let status = Credential::poll_qrcode(&client.client, &params.qrcode_key)
        .await
        .map_err(|_| InnerApiError::BadRequest("二维码状态查询失败，请稍后重试".into()))?;
    let value = match status {
        PollStatus::Success { credential } => {
            let config = VersionedConfig::get().update_credential(credential, &db).await?;
            session.remove::<String>("bili_qrcode").await?;
            json!({"status":"success","version":config.version})
        }
        status => serde_json::to_value(status)?,
    };
    Ok(ApiResponse::ok(value))
}
