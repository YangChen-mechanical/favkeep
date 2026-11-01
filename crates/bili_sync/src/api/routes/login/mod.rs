use crate::{
    api::{
        error::InnerApiError,
        request::{GenerateQrcodeRequest, LoginPlatform, PollQrcodeRequest},
        response::GenerateQrcodeResponse,
        wrapper::{ApiError, ApiResponse, ValidatedJson},
    },
    auth::{AuthState, AuthenticatedSession},
    bilibili::{BiliClient, Credential, PollStatus},
    config::VersionedConfig,
    douyin,
};
use axum::extract::Query;
use axum::{Extension, Router, routing::post};
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
    Extension(auth_state): Extension<Arc<AuthState>>,
    Extension(identity): Extension<AuthenticatedSession>,
    session: Session,
    Query(params): Query<GenerateQrcodeRequest>,
) -> Result<ApiResponse<GenerateQrcodeResponse>, ApiError> {
    match params.platform {
        LoginPlatform::Xiaohongshu => {
            return Ok(ApiResponse::ok(
                crate::xiaohongshu::login::generate(identity.0, auth_state)
                    .await
                    .map_err(xiaohongshu_error)?,
            ));
        }
        LoginPlatform::Douyin => {
            return Ok(ApiResponse::ok(
                douyin::login::generate(identity.0, auth_state, params.force_qr)
                    .await
                    .map_err(douyin_error)?,
            ));
        }
        LoginPlatform::Bilibili => {}
    }
    let qrcode = Credential::generate_qrcode(&client.client)
        .await
        .map_err(|_| InnerApiError::BadRequest("二维码生成失败，请稍后重试".into()))?;
    session.insert("bili_qrcode", qrcode.qrcode_key.clone()).await?;
    Ok(ApiResponse::ok(GenerateQrcodeResponse {
        url: qrcode.url,
        qrcode_key: qrcode.qrcode_key,
        image: None,
        status: None,
        message: None,
    }))
}

pub async fn poll_qrcode(
    Extension(client): Extension<Arc<BiliClient>>,
    Extension(db): Extension<DatabaseConnection>,
    Extension(identity): Extension<AuthenticatedSession>,
    session: Session,
    ValidatedJson(params): ValidatedJson<PollQrcodeRequest>,
) -> Result<ApiResponse<Value>, ApiError> {
    match params.platform {
        LoginPlatform::Xiaohongshu => {
            let result = if params.cancel {
                crate::xiaohongshu::login::cancel(&identity.0, &params.qrcode_key).await
            } else {
                crate::xiaohongshu::login::poll(&identity.0, &params.qrcode_key).await
            };
            return Ok(ApiResponse::ok(serde_json::to_value(
                result.map_err(xiaohongshu_error)?,
            )?));
        }
        LoginPlatform::Douyin => {
            let result = if params.cancel {
                douyin::login::cancel(&identity.0, &params.qrcode_key).await
            } else {
                douyin::login::poll(&identity.0, &params.qrcode_key).await
            };
            return Ok(ApiResponse::ok(serde_json::to_value(result.map_err(douyin_error)?)?));
        }
        LoginPlatform::Bilibili => {}
    }
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

fn douyin_error(error: douyin::Error) -> InnerApiError {
    InnerApiError::BadRequest(
        match error {
            douyin::Error::Busy => "已有抖音登录请求进行中，请等待该次登录结束后重试",
            douyin::Error::Paused => "抖音正在冷却，请等待后再验证连接",
            douyin::Error::InvalidData => "二维码不属于当前登录会话，请重新生成",
            _ => "抖音连接服务暂时不可用，请稍后重试",
        }
        .into(),
    )
}

fn xiaohongshu_error(error: crate::xiaohongshu::Error) -> InnerApiError {
    InnerApiError::BadRequest(match error {
        douyin::Error::InvalidData => "二维码不属于当前登录会话，请重新生成".into(),
        _ => error.message().into(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_qr_requests_default_to_bilibili_and_reject_unknown_platforms() {
        let generate: GenerateQrcodeRequest = serde_json::from_value(json!({})).unwrap();
        let poll: PollQrcodeRequest = serde_json::from_value(json!({"qrcode_key":"fixture"})).unwrap();
        assert_eq!(generate.platform, LoginPlatform::Bilibili);
        assert!(!generate.force_qr);
        let generate: GenerateQrcodeRequest =
            serde_json::from_value(json!({"platform":"douyin","force_qr":true})).unwrap();
        assert_eq!(generate.platform, LoginPlatform::Douyin);
        assert!(generate.force_qr);
        assert_eq!(poll.platform, LoginPlatform::Bilibili);
        let poll: PollQrcodeRequest =
            serde_json::from_value(json!({"qrcode_key":"fixture","platform":"douyin"})).unwrap();
        assert_eq!(poll.platform, LoginPlatform::Douyin);
        assert!(
            serde_json::from_value::<PollQrcodeRequest>(json!({"qrcode_key":"fixture","platform":"unknown"})).is_err()
        );
        let response = GenerateQrcodeResponse {
            url: "https://example.test/fixture".into(),
            qrcode_key: "fixture".into(),
            image: None,
            status: None,
            message: None,
        };
        assert_eq!(
            serde_json::to_value(response).unwrap(),
            json!({"url":"https://example.test/fixture","qrcode_key":"fixture"})
        );
    }
}
