use std::error::Error as StdError;
use std::fmt;
use std::time::Duration;

use serde_json::{Map, Value};

use crate::client::ConfigError;

type Cause = Box<dyn StdError + Send + Sync + 'static>;

/// `Result` with [`Error`] as the default error type.
pub type Result<T, E = Error> = std::result::Result<T, E>;

/// What kind of failure an [`Error`] is, for a `match`.
///
/// New kinds may be added, so a `match` needs a `_` arm.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ErrorKind {
    /// `400`: the request is outside the contract; [`Error::reason`] says which rule.
    BadRequest,
    /// `401`: the API key is missing, unknown or revoked.
    Authentication,
    /// `403`: the key's plan does not include the method (`reason` is
    /// `plan_scope_missing`), or another permission refusal.
    PermissionDenied,
    /// `404`: the thing asked about is not known.
    NotFound,
    /// `422`: the request is valid, but the data cannot answer it (for example
    /// `block_not_available`, `coverage_gap`).
    UnprocessableEntity,
    /// `429`: a rate limit, a concurrency cap or a quota; see [`Error::bucket`].
    RateLimit,
    /// `5xx`: the API failed.
    InternalServer,
    /// No complete answer arrived: connecting, sending or reading the body failed.
    Connection,
    /// No complete answer within the client's timeout.
    Timeout,
    /// An answer that is not the API's: a redirect, a proxy page, a body that is
    /// not JSON or does not match the method's type, an empty or repeated page
    /// cursor.
    UnexpectedResponse,
    /// The SDK refused the call before sending it: a path value that is empty,
    /// `.` or `..`, or a client that could not be built (from a [`ConfigError`]).
    InvalidArgument,
    /// Any other answer of the API, for example a status this SDK does not
    /// classify.
    Other,
}

/// Every failure of a TraceFast call: a refusal from the API, an answer that is
/// not the API's, a network failure, a timeout, or an argument the SDK refused.
///
/// Branch on [`kind`](Self::kind), [`code`](Self::code) and
/// [`reason`](Self::reason) — never on the message, which is for humans and may
/// be reworded. `Display` prints `"<status> <code>: <message>"` (without the
/// status when no answer arrived).
///
/// ```no_run
/// # async fn run(client: tracefast::Client) {
/// use tracefast::ErrorKind;
///
/// match client.blocks().get(99_999_999).await {
///     Ok(block) => println!("{}", block.data.hash),
///     Err(e) if e.kind() == ErrorKind::NotFound => println!("no such block"),
///     Err(e) if e.code() == "block_not_available" => println!("not covered yet: {:?}", e.details()),
///     Err(e) => eprintln!("{e} (request {:?})", e.request_id()),
/// }
/// # }
/// ```
pub struct Error(Box<Inner>);

struct Inner {
    status: u16,
    code: String,
    message: String,
    reason: Option<String>,
    bucket: Option<String>,
    details: Option<Map<String, Value>>,
    body: Option<Map<String, Value>>,
    request_id: Option<String>,
    retry_after: Option<Duration>,
    cause: Option<Cause>,
}

impl Error {
    pub(crate) fn new(status: u16, code: impl Into<String>, message: impl Into<String>) -> Self {
        Self(Box::new(Inner {
            status,
            code: code.into(),
            message: message.into(),
            reason: None,
            bucket: None,
            details: None,
            body: None,
            request_id: None,
            retry_after: None,
            cause: None,
        }))
    }

    /// An argument the SDK refuses before any request.
    pub(crate) fn invalid_argument(message: impl Into<String>) -> Self {
        Self::new(0, "invalid_argument", message)
    }

    /// The API's error object (the `error` of `{"error": {...}}`) of an answer
    /// with this status, or `None` when it has no string `code`.
    pub(crate) fn from_body(status: u16, body: Map<String, Value>) -> Option<Self> {
        let code = body.get("code")?.as_str()?.to_string();
        let text = |m: Option<&Map<String, Value>>, k: &str| m.and_then(|m| m.get(k)).and_then(Value::as_str).map(str::to_string);
        let message = text(Some(&body), "message").unwrap_or_else(|| code.clone());
        let details = body.get("details").and_then(Value::as_object).cloned();
        let mut e = Self::new(status, code, message);
        e.0.reason = text(Some(&body), "reason").or_else(|| text(details.as_ref(), "reason"));
        e.0.bucket = text(Some(&body), "bucket");
        e.0.request_id = text(Some(&body), "requestId");
        e.0.details = details;
        e.0.body = Some(body);
        Some(e)
    }

    pub(crate) fn with_cause(mut self, cause: impl Into<Cause>) -> Self {
        self.0.cause = Some(cause.into());
        self
    }

    /// Sets the request id unless the body already gave one.
    pub(crate) fn with_header_request_id(mut self, request_id: Option<String>) -> Self {
        if self.0.request_id.is_none() {
            self.0.request_id = request_id;
        }
        self
    }

    pub(crate) fn with_retry_after(mut self, retry_after: Option<Duration>) -> Self {
        self.0.retry_after = retry_after;
        self
    }

    /// HTTP status of the answer; `0` when no answer arrived.
    pub fn status(&self) -> u16 {
        self.0.status
    }

    /// The stable word to branch on: the API's registry (`bad_request`,
    /// `invalid_cursor`, `not_found`, `block_not_available`, `coverage_gap`,
    /// `quota_exceeded`, `service_unavailable`, …), the gateway's (`auth_failed`,
    /// `forbidden`, `rate_limited`, `concurrency_limited`), or the SDK's own:
    /// `unexpected_response`, `connection_error`, `timeout`, `invalid_argument`.
    pub fn code(&self) -> &str {
        &self.0.code
    }

    /// The kind of failure, from the status and the SDK's own codes.
    pub fn kind(&self) -> ErrorKind {
        match (self.0.code.as_str(), self.0.status) {
            ("unexpected_response", _) => ErrorKind::UnexpectedResponse,
            ("connection_error", _) => ErrorKind::Connection,
            ("timeout", _) => ErrorKind::Timeout,
            ("invalid_argument", 0) => ErrorKind::InvalidArgument,
            (_, 400) => ErrorKind::BadRequest,
            (_, 401) => ErrorKind::Authentication,
            (_, 403) => ErrorKind::PermissionDenied,
            (_, 404) => ErrorKind::NotFound,
            (_, 422) => ErrorKind::UnprocessableEntity,
            (_, 429) => ErrorKind::RateLimit,
            (_, 500..=599) => ErrorKind::InternalServer,
            _ => ErrorKind::Other,
        }
    }

    /// The API's own message (or the SDK's), without the status and the code.
    /// For humans: never parse it.
    pub fn message(&self) -> &str {
        &self.0.message
    }

    /// The stable sub-code: `error.reason` on the gateway's key, plan and limit
    /// refusals (`api_key_required`, `plan_scope_missing`, …), otherwise
    /// `details.reason`.
    pub fn reason(&self) -> Option<&str> {
        self.0.reason.as_deref()
    }

    /// On a `429`: which limit refused (`base`, `class:…`, `concurrency`,
    /// `quota_daily`, `quota_method`, …).
    pub fn bucket(&self) -> Option<&str> {
        self.0.bucket.as_deref()
    }

    /// What to act on, when the API states it (`maxAvailableBlock`, `maxLimit`,
    /// `minimumPlan`, `currentPlan`, `upgrade`, …).
    pub fn details(&self) -> Option<&Map<String, Value>> {
        self.0.details.as_ref()
    }

    /// The raw `error` object of the answer, so a field this SDK does not know
    /// survives. `None` when the answer carried no such object.
    pub fn body(&self) -> Option<&Map<String, Value>> {
        self.0.body.as_ref()
    }

    /// The request's id, from the body or else the `X-Request-Id` header. Quote
    /// it when asking for support.
    pub fn request_id(&self) -> Option<&str> {
        self.0.request_id.as_deref()
    }

    /// The wait the API asked for in `Retry-After`, exactly as sent — not capped
    /// by `max_retry_wait`. A daily quota may ask for hours.
    pub fn retry_after(&self) -> Option<Duration> {
        self.0.retry_after
    }

    /// [`ErrorKind::BadRequest`].
    pub fn is_bad_request(&self) -> bool {
        self.kind() == ErrorKind::BadRequest
    }

    /// [`ErrorKind::Authentication`].
    pub fn is_authentication(&self) -> bool {
        self.kind() == ErrorKind::Authentication
    }

    /// [`ErrorKind::PermissionDenied`], a plan refusal included.
    pub fn is_permission_denied(&self) -> bool {
        self.kind() == ErrorKind::PermissionDenied
    }

    /// [`ErrorKind::NotFound`].
    pub fn is_not_found(&self) -> bool {
        self.kind() == ErrorKind::NotFound
    }

    /// [`ErrorKind::UnprocessableEntity`].
    pub fn is_unprocessable_entity(&self) -> bool {
        self.kind() == ErrorKind::UnprocessableEntity
    }

    /// [`ErrorKind::RateLimit`]: a rate limit, a concurrency cap or a quota.
    pub fn is_rate_limited(&self) -> bool {
        self.kind() == ErrorKind::RateLimit
    }

    /// [`ErrorKind::InternalServer`].
    pub fn is_server_error(&self) -> bool {
        self.kind() == ErrorKind::InternalServer
    }

    /// [`ErrorKind::Connection`].
    pub fn is_connection(&self) -> bool {
        self.kind() == ErrorKind::Connection
    }

    /// [`ErrorKind::Timeout`].
    pub fn is_timeout(&self) -> bool {
        self.kind() == ErrorKind::Timeout
    }

    /// [`ErrorKind::UnexpectedResponse`].
    pub fn is_unexpected_response(&self) -> bool {
        self.kind() == ErrorKind::UnexpectedResponse
    }

    /// [`ErrorKind::InvalidArgument`].
    pub fn is_invalid_argument(&self) -> bool {
        self.kind() == ErrorKind::InvalidArgument
    }
}

impl fmt::Debug for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let e = &self.0;
        f.debug_struct("Error")
            .field("status", &e.status)
            .field("code", &e.code)
            .field("message", &e.message)
            .field("reason", &e.reason)
            .field("bucket", &e.bucket)
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
        if e.status != 0 {
            write!(f, "{} ", e.status)?;
        }
        write!(f, "{}: {}", e.code, e.message)
    }
}

impl StdError for Error {
    fn source(&self) -> Option<&(dyn StdError + 'static)> {
        self.0.cause.as_deref().map(|c| c as &(dyn StdError + 'static))
    }
}

/// A client that cannot be built becomes an [`ErrorKind::InvalidArgument`]
/// error with the code `invalid_argument` and the [`ConfigError`] as its source,
/// so `?` works in a function that returns [`Result`].
impl From<ConfigError> for Error {
    fn from(e: ConfigError) -> Self {
        Self::invalid_argument(e.to_string()).with_cause(e)
    }
}
