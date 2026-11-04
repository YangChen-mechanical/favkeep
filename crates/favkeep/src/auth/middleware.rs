use super::{
    AuthError, AuthResult, AuthState, AuthenticatedSession, IDLE_SECONDS, IdentitySession, MAX_SECONDS, internal, now,
    session_hash, unauthorized,
};
use axum::response::IntoResponse;
use axum::{
    Extension,
    extract::Request,
    http::{Method, StatusCode, header},
    middleware::Next,
    response::Response,
};
use std::sync::Arc;
use subtle::ConstantTimeEq;
use tower_sessions::Session;

pub async fn protect(
    Extension(state): Extension<Arc<AuthState>>,
    session: Session,
    axum::extract::OriginalUri(uri): axum::extract::OriginalUri,
    mut request: Request,
    next: Next,
) -> Response {
    let path = uri.path();
    let public = matches!(path, "/api/auth/setup" | "/api/auth/login" | "/api/auth/session");
    let write = !matches!(*request.method(), Method::GET | Method::HEAD | Method::OPTIONS);
    let websocket = request.headers().contains_key(header::UPGRADE);
    let result: AuthResult<()> = async {
        if (write || websocket) && !state.origin_matches(request.headers()) {
            return Err(AuthError(StatusCode::FORBIDDEN, "请求来源不受信任"));
        }
        if !public {
            let identity: IdentitySession = session
                .get("identity")
                .await
                .map_err(internal)?
                .ok_or_else(unauthorized)?;
            let hash = session.id().map(|id| session_hash(&id)).ok_or_else(unauthorized)?;
            if identity.created_at + MAX_SECONDS <= now() || state.valid_hash(&hash).await?.is_none() {
                return Err(unauthorized());
            }
            if write {
                let csrf = request
                    .headers()
                    .get("x-csrf-token")
                    .and_then(|h| h.to_str().ok())
                    .unwrap_or("");
                if !bool::from(csrf.as_bytes().ct_eq(identity.csrf.as_bytes())) {
                    return Err(AuthError(StatusCode::FORBIDDEN, "CSRF 校验失败，请刷新页面"));
                }
            }
            session.set_expiry(Some(tower_sessions::Expiry::AtDateTime(
                cookie::time::OffsetDateTime::from_unix_timestamp(
                    (now() + IDLE_SECONDS).min(identity.created_at + MAX_SECONDS),
                )
                .map_err(internal)?,
            )));
            request.extensions_mut().insert(AuthenticatedSession(hash));
        }
        Ok(())
    }
    .await;
    let mut response = match result {
        Ok(()) => next.run(request).await,
        Err(error) => error.into_response(),
    };
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, "private, no-store".parse().unwrap());
    response
}
