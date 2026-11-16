//! The official Rust SDK for the [TraceFast API](https://docs.tracefast.xyz/api):
//! Ethereum blocks, transactions, traces, tokens, contracts and what TraceFast
//! states about an address.
//!
//! ```no_run
//! # async fn run() -> Result<(), Box<dyn std::error::Error>> {
//! use tracefast::{AddressesTransactionsParams, Client};
//!
//! let client = Client::new(std::env::var("TRACEFAST_API_KEY")?)?;
//!
//! // The whole envelope comes back: `data` and `meta`.
//! let proxy = client.contracts().proxy("0xa0b86991c6218b36c1d19d4a2e9eb0ce3606eb48").await?;
//! println!("{:?} as of {:?}", proxy.data, proxy.meta);
//!
//! // A paged method walks its pages until `data.nextCursor` is absent.
//! let params = AddressesTransactionsParams::new().limit(50);
//! let mut pages = client
//!     .addresses()
//!     .transactions_pages("0xd8da6bf26964af9d7eed9e03e53415d37aa96045", &params);
//! while let Some(page) = pages.next_page().await? {
//!     println!("{} transactions", page.data.transactions.len());
//! }
//! # Ok(())
//! # }
//! ```
//!
//! # The key
//!
//! The API key travels only in the `x-api-key` header. It never appears in a URL,
//! an error or `Debug` output. It is for server-side code: do not ship it inside a
//! program that runs on someone else's machine or in a browser.
//!
//! # Answers
//!
//! - Every method returns the whole envelope (`data` and `meta`), never `data` alone.
//! - Absent, `null` and a value are three different answers. A field the API may
//!   leave out or set to `null` is an `Option`; a field that may be either is an
//!   `Option<Option<_>>`: `None` is absent, `Some(None)` is `null`.
//! - Types are open: an unknown field is ignored, and an enum is a `String`.
//! - Amounts, balances and wei are strings, as in the API.
//!
//! # Errors and retries
//!
//! Every failure is an [`Error`]. Branch on [`Error::code`] and [`Error::reason`],
//! never on the message. Only `429` with `Retry-After`, `503` and a connection
//! failure are retried, at most `max_retries` times (default 2).
#![forbid(unsafe_code)]
#![warn(missing_docs)]

mod client;
mod de;
mod error;
mod generated;
mod pager;

pub use client::{Client, ClientBuilder, ConfigError, DEFAULT_BASE_URL};
pub use error::Error;
pub use generated::api::*;
pub use pager::{Paged, Pager};

/// The response types: one struct per schema of the API catalogue.
pub mod types {
    pub use crate::generated::types::*;
}

/// The version of this crate, sent in the `User-Agent` header.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
