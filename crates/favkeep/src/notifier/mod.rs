mod info;
mod message;

use std::collections::HashMap;

use anyhow::{Result, anyhow};
use futures::future;
pub use info::DownloadNotifyInfo;
use lettre::message::{Attachment, MultiPart, SinglePart};
use lettre::transport::smtp::authentication::Credentials;
use lettre::{AsyncSmtpTransport, AsyncTransport, Tokio1Executor};
pub use message::Message;
use reqwest::header;
use serde::{Deserialize, Serialize};

use crate::config::TEMPLATE;

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", tag = "type")]
pub enum Notifier {
    Telegram {
        bot_token: String,
        chat_id: String,
        #[serde(default)]
        skip_image: bool,
    },
    Webhook {
        url: String,
        template: Option<String>,
        #[serde(default)]
        headers: Option<HashMap<String, String>>,
        #[serde(skip)]
        // 一个内部辅助字段，用于决定是否强制渲染当前模板，在测试时使用
        ignore_cache: Option<()>,
    },
    Smtp {
        host: String,
        port: u16,
        encryption: SmtpEncryption,
        #[serde(default)]
        username: String,
        #[serde(default)]
        password: String,
        from: String,
        to: String,
    },
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum SmtpEncryption {
    None,
    Tls,
    StartTls,
}

pub fn webhook_template_key(url: &str) -> String {
    format!("payload_{}", url)
}

pub fn webhook_template_content(template: &Option<String>) -> &str {
    template
        .as_deref()
        .filter(|t| !t.trim().is_empty())
        .unwrap_or(r#"{"text": "{{{message}}}"}"#)
}

pub trait NotifierAllExt {
    async fn notify_all<'a>(&self, client: &reqwest::Client, message: impl Into<Message<'a>>) -> Result<()>;
}

impl NotifierAllExt for Vec<Notifier> {
    async fn notify_all<'a>(&self, client: &reqwest::Client, message: impl Into<Message<'a>>) -> Result<()> {
        let message = message.into();
        future::join_all(self.iter().map(|notifier| notifier.notify_internal(client, &message))).await;
        Ok(())
    }
}

impl Notifier {
    pub async fn notify<'a>(&self, client: &reqwest::Client, message: impl Into<Message<'a>>) -> Result<()> {
        self.notify_internal(client, &message.into()).await
    }

    async fn notify_internal<'a>(&self, client: &reqwest::Client, message: &Message<'a>) -> Result<()> {
        match self {
            Notifier::Telegram {
                bot_token,
                chat_id,
                skip_image,
            } => {
                if let Some(img_url) = &message.image_url
                    && !*skip_image
                {
                    let url = format!("https://api.telegram.org/bot{}/sendPhoto", bot_token);
                    let params = [
                        ("chat_id", chat_id.as_str()),
                        ("photo", img_url.as_str()),
                        ("caption", message.message.as_ref()),
                    ];
                    client.post(&url).form(&params).send().await?;
                } else {
                    let url = format!("https://api.telegram.org/bot{}/sendMessage", bot_token);
                    let params = [("chat_id", chat_id.as_str()), ("text", message.message.as_ref())];
                    client.post(&url).form(&params).send().await?;
                }
            }
            Notifier::Webhook {
                url,
                template,
                headers,
                ignore_cache,
            } => {
                let key = webhook_template_key(url);
                let handlebar = TEMPLATE.read();
                let payload = match ignore_cache {
                    Some(_) => handlebar.render_template(webhook_template_content(template), &message)?,
                    None => handlebar.render(&key, &message)?,
                };
                let mut headers_map = header::HeaderMap::new();
                headers_map.insert(header::CONTENT_TYPE, "application/json".try_into()?);

                if let Some(custom_headers) = headers {
                    for (key, value) in custom_headers {
                        if let (Ok(key), Ok(value)) =
                            (header::HeaderName::try_from(key), header::HeaderValue::try_from(value))
                        {
                            headers_map.insert(key, value);
                        }
                    }
                }

                client.post(url).headers(headers_map).body(payload).send().await?;
            }
            Notifier::Smtp {
                host,
                port,
                encryption,
                username,
                password,
                from,
                to,
            } => {
                let email = lettre::Message::builder()
                    .from(from.parse()?)
                    .to(to.parse()?)
                    .subject("BiliSync 通知");
                let email = if let Some(image_url) = &message.image_url {
                    let response = client.get(image_url).send().await?.error_for_status()?;
                    let content_type = response
                        .headers()
                        .get(header::CONTENT_TYPE)
                        .ok_or_else(|| anyhow!("Missing image Content-Type"))?
                        .to_str()?
                        .parse()?;
                    let image = response.bytes().await?.to_vec();
                    let content_id = uuid::Uuid::new_v4().to_string();
                    let html = format!(
                        r#"<p>{}</p><img src="cid:{content_id}" style="max-width: 100%;" alt="">"#,
                        handlebars::html_escape(&message.message).replace('\n', "<br>")
                    );
                    email.multipart(
                        MultiPart::alternative()
                            .singlepart(SinglePart::plain(message.message.to_string()))
                            .multipart(
                                MultiPart::related()
                                    .singlepart(SinglePart::html(html))
                                    .singlepart(Attachment::new_inline(content_id).body(image, content_type)),
                            ),
                    )?
                } else {
                    email.singlepart(SinglePart::plain(message.message.to_string()))?
                };
                let mut mailer = match encryption {
                    SmtpEncryption::None => AsyncSmtpTransport::<Tokio1Executor>::builder_dangerous(host),
                    SmtpEncryption::Tls => AsyncSmtpTransport::<Tokio1Executor>::relay(host)?,
                    SmtpEncryption::StartTls => AsyncSmtpTransport::<Tokio1Executor>::starttls_relay(host)?,
                }
                .port(*port);
                if !username.is_empty() || !password.is_empty() {
                    mailer = mailer.credentials(Credentials::new(username.clone(), password.clone()));
                }
                mailer.build().send(email).await?;
            }
        }
        Ok(())
    }
}
