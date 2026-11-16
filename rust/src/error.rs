use std::error::Error as StdError;
use std::fmt;

use serde_json::{Map, Value};

type Cause = Box<dyn StdError + Send + Sync + 'static>;

/// Every failure of a TraceFast call: a refusal from the API, an answer that is
/// not the API's, a network failure or a timeout.
///
/// Branch on [`code`](Self::code) and [`reason`](Self::reason) — never on
/// [`message`](Self::message), which is for humans and may be reworded.
///
/// ```no_run
/// # async fn run(client: tracefast::Client) {
/// match client.blocks().get(99_999_999).await {
///     Ok(block) => println!("{:?}", block.data),
///     Err(e) if e.code() == "not_found" => println!("no such block"),
///     Err(e) if e.reason() == Some("block_not_available") => println!("not covered yet"),
///     Err(e) => eprintln!("{e} (request {:?})", e.request_id()),
/// }
/// # }
/// ```
pub struct Error(Box<Inner>);

#[derive(Debug)]
struct Inner {
    status: u16,
    code: String,
    message: String,
    details: Option<Map<String, Value>>,
    request_id: Option<String>,
    retry_after: Option<u64>,
    cause: Option<Cause>,
}

impl Error {
    pub(crate) fn new(status: u16, code: impl Into<String>, message: impl Into<String>) -> Self {
        Self(Box::new(Inner {
            status,
            code: code.into(),
            message: message.into(),
            details: None,
            request_id: None,
            retry_after: None,
            cause: None,
        }))
    }

    pub(crate) fn with_cause(mut self, cause: impl Into<Cause>) -> Self {
        self.0.cause = Some(cause.into());
        self
    }

    pub(crate) fn with_details(mut self, details: Option<Map<String, Value>>) -> Self {
        self.0.details = details;
        self
    }

    pub(crate) fn with_request_id(mut self, request_id: Option<String>) -> Self {
        self.0.request_id = request_id;
        self
    }

    pub(crate) fn with_retry_after(mut self, retry_after: Option<u64>) -> Self {
        self.0.retry_after = retry_after;
        self
    }

    /// HTTP status of the answer; `0` when no answer arrived.
    pub fn status(&self) -> u16 {
        self.0.status
    }

    /// The stable word from the API's error registry (`bad_request`, `not_found`,
    /// `block_not_available`, `rate_limited`, …), or one of the SDK's own:
    /// `unexpected_response`, `connection_error`, `timeout`.
    pub fn code(&self) -> &str {
        &self.0.code
    }

    /// What went wrong, for humans. Never parse it.
    pub fn message(&self) -> &str {
        &self.0.message
    }

    /// What to act on, when the API states it; `details.reason` is the stable sub-code.
    pub fn details(&self) -> Option<&Map<String, Value>> {
        self.0.details.as_ref()
    }

    /// `details.reason`, when the API gave one.
    pub fn reason(&self) -> Option<&str> {
        self.details()?.get("reason")?.as_str()
    }

    /// The request's id. Quote it when asking for support.
    pub fn request_id(&self) -> Option<&str> {
        self.0.request_id.as_deref()
    }

    /// Seconds the API asked to wait, from `Retry-After` (capped at 60).
    pub fn retry_after(&self) -> Option<u64> {
        self.0.retry_after
    }
}

impl fmt::Debug for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let e = &self.0;
        f.debug_struct("Error")
            .field("status", &e.status)
            .field("code", &e.code)
            .field("message", &e.message)
            .field("details", &e.details)
            .field("request_id", &e.request_id)
            .field("retry_after", &e.retry_after)
            .field("cause", &e.cause)
            .finish()
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let e = &self.0;
        if e.status == 0 {
            write!(f, "TraceFast {}: {}", e.code, e.message)?;
        } else {
            write!(f, "TraceFast {} (HTTP {}): {}", e.code, e.status, e.message)?;
        }
        if let Some(id) = &e.request_id {
            write!(f, " [request {id}]")?;
        }
        Ok(())
    }
}

impl StdError for Error {
    fn source(&self) -> Option<&(dyn StdError + 'static)> {
        self.0.cause.as_deref().map(|c| c as &(dyn StdError + 'static))
    }
}
