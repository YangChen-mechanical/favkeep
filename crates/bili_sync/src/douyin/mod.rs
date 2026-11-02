use crate::utils::rc4 as codec;
mod client;
pub(crate) mod login;
mod media;
mod native;
pub(crate) use native::Auth;
pub(crate) mod progress;
pub(crate) mod runtime;
use crate::utils::cookies;
pub(crate) mod store;
pub(crate) use crate::platform::{Error, Result, valid_id};

pub static RUNNING: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
