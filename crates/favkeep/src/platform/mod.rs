mod error;
pub(crate) use error::{Error, Result, valid_id};

use serde::{Deserialize, Serialize};

#[derive(Debug, thiserror::Error)]
#[error("不支持的平台")]
pub struct UnsupportedPlatform;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Platform {
    #[default]
    Bilibili,
    Douyin,
    Xiaohongshu,
}

impl Platform {
    pub fn parse(value: &str) -> std::result::Result<Self, UnsupportedPlatform> {
        match value {
            "bilibili" => Ok(Self::Bilibili),
            "douyin" => Ok(Self::Douyin),
            "xiaohongshu" => Ok(Self::Xiaohongshu),
            _ => Err(UnsupportedPlatform),
        }
    }

    pub fn id(self) -> &'static str {
        match self {
            Self::Bilibili => "bilibili",
            Self::Douyin => "douyin",
            Self::Xiaohongshu => "xiaohongshu",
        }
    }

    pub fn source_url(self, id: Option<&str>) -> Option<String> {
        let id = id.filter(|id| !id.is_empty())?;
        // IDs from storage must never change the URL's host, path or query.
        let base = match self {
            Self::Bilibili => "https://www.bilibili.com/video/",
            Self::Douyin => "https://www.douyin.com/video/",
            Self::Xiaohongshu => "https://www.xiaohongshu.com/explore/",
        };
        let mut url = reqwest::Url::parse(base).expect("static URL");
        {
            let mut path = url.path_segments_mut().expect("static URL path");
            path.pop_if_empty().push(id);
            if self == Self::Bilibili {
                path.push("");
            }
        }
        Some(url.into())
    }
}

#[derive(Serialize)]
pub struct PlatformInfo {
    pub id: Platform,
    pub name: &'static str,
    pub capabilities: &'static [&'static str],
}

pub fn platforms() -> [PlatformInfo; 3] {
    [
        PlatformInfo {
            id: Platform::Bilibili,
            name: "B站",
            capabilities: &[
                "status_edit",
                "force_retry",
                "clear_redownload",
                "danmaku",
                "subtitles",
                "multipart",
                "download_settings",
                "qr_refresh",
            ],
        },
        PlatformInfo {
            id: Platform::Xiaohongshu,
            name: "小红书",
            capabilities: &["retry_failed"],
        },
        PlatformInfo {
            id: Platform::Douyin,
            name: "抖音",
            capabilities: &["retry_failed"],
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_unknown_platforms_and_encodes_source_ids() {
        assert!(Platform::parse("unknown").is_err());
        assert!(Platform::Douyin.source_url(None).is_none());
        assert_eq!(
            Platform::Douyin.source_url(Some("a/b?c")).unwrap(),
            "https://www.douyin.com/video/a%2Fb%3Fc"
        );
    }
}
