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

impl Error {
    pub fn message(&self) -> &'static str {
        match self {
            Self::Busy => "账号正在处理其他操作，请稍后重试",
            Self::Paused => "账号已暂停或正在冷却，请处理后验证连接与恢复",
            Self::SelectionChanged => "扫描期间收藏来源发生变化，本夹未提交，请重新同步",
            Self::DiskFull => "磁盘空间不足，已暂停，请释放空间后恢复",
            Self::Rejected => "平台拒绝请求，进入冷却；连续三次拒绝后暂停",
            Self::LoginExpired => "二维码或登录会话已过期／取消，请重新登录",
            Self::Challenge => "平台要求安全验证，已暂停，请按平台提示处理后重试",
            Self::IdentityUnverified => "未验证官方当前账号，未保存凭据",
            Self::AccountChanged => "与已绑定账号不符，未保存新账号",
            Self::InvalidData => "平台数据无效，本夹未提交",
            Self::Incomplete => "分页扫描不完整，本夹未提交，请稍后重试",
            Self::Browser => "浏览器运行失败，请检查浏览器配置后重试",
            Self::Network => "网络请求失败或超时，请稍后重试",
            Self::File => "本地文件或数据库操作失败，请检查权限与存储",
            Self::MediaUnavailable => "媒体不可用或校验失败，请重试失败项",
            Self::TokenUnavailable => "未取得实时浏览器令牌，请稍后重试",
        }
    }
}

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
