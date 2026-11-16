#![doc = include_str!("../README.md")]
#![cfg_attr(docsrs, feature(doc_cfg))]
#![forbid(unsafe_code)]
#![warn(missing_docs)]

mod client;
mod de;
mod error;
mod generated;
mod pager;

pub use client::{Client, ClientBuilder, ConfigError, DEFAULT_BASE_URL, ResponseFuture};
pub use error::{Error, ErrorKind, Result};
pub use generated::api::*;
pub use pager::{Items, Paged, Pager};

/// The response types: one struct per schema of the API catalogue.
///
/// Every struct is `#[non_exhaustive]`, so a field the catalogue adds is not a
/// breaking change; build test values with `serde_json::from_value` or start
/// from `Default::default()` and set fields.
pub mod types {
    pub use crate::generated::types::*;
}

/// The version of this crate, sent in the `User-Agent` header.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
