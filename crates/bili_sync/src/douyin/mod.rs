use crate::utils::rc4 as codec;
pub mod client;
mod core;
pub mod login;
mod media;
pub mod native;
pub mod progress;
pub mod runtime;
use crate::utils::cookies;
pub mod store;
pub use core::{Error, Result, valid_id};

pub static RUNNING: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
