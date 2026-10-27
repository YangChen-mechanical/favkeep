#[derive(Debug, Clone, thiserror::Error, PartialEq, Eq)]
pub enum Error {
    #[error("account_busy")]
    Busy,
    #[error("account_paused_or_cooling_down")]
    Paused,
    #[error("selection_changed_during_scan")]
    SelectionChanged,
    #[error("disk_full")]
    DiskFull,
    #[error("invalid_platform_data")]
    InvalidData,
    #[error("incomplete_pagination")]
    Incomplete,
    #[error("platform_rejected")]
    Rejected,
    #[error("login_expired")]
    LoginExpired,
    #[error("verification_required")]
    Challenge,
    #[error("identity_unverified")]
    IdentityUnverified,
    #[error("account_changed")]
    AccountChanged,
    #[error("browser_failed")]
    Browser,
    #[error("network_failed")]
    Network,
    #[error("private_file_failed")]
    File,
    #[error("media_unavailable")]
    MediaUnavailable,
    #[error("browser_token_unavailable")]
    TokenUnavailable,
}

pub type Result<T> = std::result::Result<T, Error>;

impl From<std::io::Error> for Error {
    fn from(error: std::io::Error) -> Self {
        if matches!(error.raw_os_error(), Some(28 | 112)) {
            Self::DiskFull
        } else {
            Self::File
        }
    }
}

pub fn valid_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 128 && id.bytes().all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b))
}
