use super::{AuthError, AuthResult, now, statement};
use argon2::{Argon2, PasswordHasher, password_hash::SaltString};
use axum::http::StatusCode;
use sea_orm::{ConnectionTrait, DatabaseConnection, TransactionTrait};

pub(super) fn validate_password(password: &str) -> AuthResult<()> {
    if !(15..=128).contains(&password.chars().count()) {
        return Err(AuthError(StatusCode::BAD_REQUEST, "密码须为 15–128 个字符"));
    }
    Ok(())
}

pub(crate) async fn reset_password(db: &DatabaseConnection) -> anyhow::Result<()> {
    let password = rpassword::prompt_password("新管理员密码（15–128 字符）：")?;
    let confirmation = rpassword::prompt_password("再次输入新密码：")?;
    anyhow::ensure!(
        password == confirmation && validate_password(&password).is_ok(),
        "密码不匹配或长度无效"
    );
    let salt = SaltString::encode_b64(&rand::random::<[u8; 16]>()).map_err(|_| anyhow::anyhow!("生成密码盐失败"))?;
    let encoded = Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map_err(|_| anyhow::anyhow!("密码哈希失败"))?
        .to_string();
    let txn = db.begin().await?;
    let result = txn
        .execute(statement(
            "UPDATE app_identity SET password_hash=?,auth_version=auth_version+1,updated_at=? WHERE id=1",
            vec![encoded.into(), now().into()],
        ))
        .await?;
    anyhow::ensure!(result.rows_affected() == 1, "尚未初始化管理员，请使用初始化页");
    txn.execute(statement("DELETE FROM app_session", vec![])).await?;
    txn.commit().await?;
    println!("密码已重置，全部会话已撤销。");
    Ok(())
}
