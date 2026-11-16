use std::collections::hash_map::RandomState;
use std::fmt;
use std::future::Future;
use std::hash::{BuildHasher, Hasher};
use std::net::{Ipv4Addr, Ipv6Addr};
use std::pin::Pin;
use std::sync::{Arc, Mutex, PoisonError};
use std::task::{Context, Poll};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use reqwest::Url;
use reqwest::header::{ACCEPT, HeaderMap, HeaderValue, RETRY_AFTER, USER_AGENT};
use serde::de::DeserializeOwned;
use serde_json::Value;

use crate::error::{Error, Result};

/// The API's address, used unless [`ClientBuilder::base_url`] says otherwise.
pub const DEFAULT_BASE_URL: &str = "https://tracefast.xyz/api/v1";

/// The longest wait between two attempts when the API gave no `Retry-After`.
const MAX_BACKOFF: Duration = Duration::from_secs(8);

/// The TraceFast API client.
///
/// Cheap to clone: clones share one connection pool. Methods are grouped by
/// namespace — `client.contracts().proxy(address)`, `client.blocks().latest()` —
/// and [`Client::resolve`] lives on the client itself. A method's future borrows
/// only the client, so futures can be built in a loop and awaited together.
///
/// ```no_run
/// # async fn run() -> Result<(), Box<dyn std::error::Error>> {
/// use std::time::Duration;
///
/// let client = tracefast::Client::builder()
///     .api_key(std::env::var("TRACEFAST_API_KEY")?)
///     .user_agent("my-app/1.2")
///     .build()?;
/// let latest = client.blocks().latest().await?;
/// println!("block {}", latest.data.number);
///
/// // A copy with other settings, sharing the connection pool.
/// let patient = client.with_timeout(Duration::from_secs(120)).with_max_retries(4);
/// let block = patient.blocks().get(20_000_000).await?;
/// # let _ = block;
/// # Ok(())
/// # }
/// ```
#[derive(Clone)]
pub struct Client {
    http: reqwest::Client,
    base_url: Arc<str>,
    settings: Settings,
}

#[derive(Debug, Clone, Copy)]
struct Settings {
    chain_id: u64,
    timeout: Duration,
    max_retries: u32,
    max_retry_wait: Duration,
}

impl Client {
    /// A client with the default settings and this API key (`tf_live_…`).
    pub fn new(api_key: impl Into<String>) -> Result<Self, ConfigError> {
        Self::builder().api_key(api_key).build()
    }

    /// A builder for a client with other settings.
    pub fn builder() -> ClientBuilder {
        ClientBuilder::default()
    }

    /// The chain every request is about (`1`, Ethereum mainnet, by default).
    pub fn chain_id(&self) -> u64 {
        self.settings.chain_id
    }

    /// The API's address the client talks to.
    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    /// The longest one attempt may take, the body included.
    pub fn timeout(&self) -> Duration {
        self.settings.timeout
    }

    /// How many times an answer that says "try again" is retried.
    pub fn max_retries(&self) -> u32 {
        self.settings.max_retries
    }

    /// The longest `Retry-After` the client waits out before retrying.
    pub fn max_retry_wait(&self) -> Duration {
        self.settings.max_retry_wait
    }

    /// A copy of this client with another [timeout](ClientBuilder::timeout),
    /// sharing the connection pool.
    #[must_use]
    pub fn with_timeout(&self, timeout: Duration) -> Self {
        let mut c = self.clone();
        c.settings.timeout = timeout;
        c
    }

    /// A copy of this client with another [`max_retries`](ClientBuilder::max_retries),
    /// sharing the connection pool.
    #[must_use]
    pub fn with_max_retries(&self, max_retries: u32) -> Self {
        let mut c = self.clone();
        c.settings.max_retries = max_retries;
        c
    }

    /// A copy of this client with another [`max_retry_wait`](ClientBuilder::max_retry_wait),
    /// sharing the connection pool.
    #[must_use]
    pub fn with_max_retry_wait(&self, max_retry_wait: Duration) -> Self {
        let mut c = self.clone();
        c.settings.max_retry_wait = max_retry_wait;
        c
    }

    /// A copy of this client about another [chain](ClientBuilder::chain_id),
    /// sharing the connection pool.
    #[must_use]
    pub fn with_chain_id(&self, chain_id: u64) -> Self {
        let mut c = self.clone();
        c.settings.chain_id = chain_id;
        c
    }

    /// The request path of a template (`/chains/{chainId}/blocks/{number}`), each
    /// placeholder but `chainId` taking the next value, percent-encoded. An empty,
    /// `.` or `..` value is refused: URL normalisation would send the call to
    /// another endpoint.
    pub(crate) fn path(&self, template: &'static str, values: &[&str]) -> Result<String> {
        let mut out = String::with_capacity(template.len() + 80);
        let mut values = values.iter();
        let mut rest = template;
        while let Some(open) = rest.find('{') {
            let close = open + rest[open..].find('}').expect("a closed placeholder");
            out.push_str(&rest[..open]);
            let name = &rest[open + 1..close];
            if name == "chainId" {
                out.push_str(&self.settings.chain_id.to_string());
            } else {
                let value = values.next().expect("a value for each placeholder");
                if matches!(*value, "" | "." | "..") {
                    return Err(Error::invalid_argument(format!(
                        "`{name}` is {value:?}: an empty, `.` or `..` path value would send the call to another endpoint"
                    )));
                }
                out.push_str(&encode_path(value));
            }
            rest = &rest[close + 1..];
        }
        out.push_str(rest);
        Ok(out)
    }

    /// The future of one method call. It owns its path and query and borrows only
    /// the client.
    pub(crate) fn fetch<T: DeserializeOwned + 'static>(
        &self,
        path: Result<String>,
        query: Vec<(&'static str, String)>,
    ) -> ResponseFuture<'_, T> {
        ResponseFuture::new(async move { self.get(&path?, &query).await })
    }

    /// One `GET` with the retries of SPEC §5, its body decoded.
    pub(crate) async fn get<T: DeserializeOwned>(&self, path: &str, query: &[(&'static str, String)]) -> Result<T> {
        let answer = self.send(path, query).await?;
        serde_json::from_slice(&answer.body).map_err(|e| {
            Error::new(answer.status, "unexpected_response", "a body that is not the method's answer")
                .with_header_request_id(answer.request_id)
                .with_cause(e)
        })
    }

    /// One `GET` with the retries of SPEC §5: a `2xx` answer, or the error.
    /// Not generic, so it is compiled once.
    async fn send(&self, path: &str, query: &[(&'static str, String)]) -> Result<Answer> {
        let mut url = Url::parse(&format!("{}{path}", self.base_url))
            .map_err(|e| Error::invalid_argument("the request URL is not valid").with_cause(e))?;
        if !query.is_empty() {
            url.query_pairs_mut().extend_pairs(query.iter().map(|(k, v)| (*k, v.as_str())));
        }
        let mut attempt = 0;
        loop {
            let error = match self.attempt(url.clone()).await {
                Ok(answer) if (200..300).contains(&answer.status) => return Ok(answer),
                Ok(answer) => to_error(answer),
                Err(error) => error,
            };
            match retry_delay(&error, attempt, self.settings.max_retry_wait) {
                Some(wait) if attempt < self.settings.max_retries => {
                    tokio::time::sleep(wait).await;
                    attempt += 1;
                }
                _ => return Err(error),
            }
        }
    }

    /// One attempt: the request and its whole body, under one timeout.
    async fn attempt(&self, url: Url) -> Result<Answer> {
        let timeout = self.settings.timeout;
        let res = self.http.get(url).timeout(timeout).send().await.map_err(|e| transport_error(0, timeout, e))?;
        let status = res.status().as_u16();
        let request_id = res.headers().get("x-request-id").and_then(|v| v.to_str().ok()).map(str::to_string);
        let retry_after = res.headers().get(RETRY_AFTER).and_then(|v| v.to_str().ok()).and_then(parse_retry_after);
        let body = res.bytes().await.map_err(|e| transport_error(status, timeout, e).with_header_request_id(request_id.clone()))?;
        Ok(Answer { status, request_id, retry_after, body: body.to_vec() })
    }
}

impl fmt::Debug for Client {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Client")
            .field("api_key", &"[redacted]")
            .field("base_url", &self.base_url)
            .field("chain_id", &self.settings.chain_id)
            .field("timeout", &self.settings.timeout)
            .field("max_retries", &self.settings.max_retries)
            .field("max_retry_wait", &self.settings.max_retry_wait)
            .finish()
    }
}

/// The future of one method call: the whole answer, or the [`Error`].
///
/// It borrows only the [`Client`] (`'a`), never the arguments, so futures can be
/// collected and awaited together. It is `Send` and `Sync`.
#[must_use = "a request is sent only when its future is awaited"]
pub struct ResponseFuture<'a, T> {
    // A `Mutex` only to make the future `Sync`; it is reached through `&mut` and
    // never locked.
    inner: Mutex<Pin<Box<dyn Future<Output = Result<T>> + Send + 'a>>>,
}

impl<'a, T> ResponseFuture<'a, T> {
    fn new(future: impl Future<Output = Result<T>> + Send + 'a) -> Self {
        Self { inner: Mutex::new(Box::pin(future)) }
    }
}

impl<T> Future for ResponseFuture<'_, T> {
    type Output = Result<T>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        self.get_mut().inner.get_mut().unwrap_or_else(PoisonError::into_inner).as_mut().poll(cx)
    }
}

impl<T> fmt::Debug for ResponseFuture<'_, T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ResponseFuture").finish_non_exhaustive()
    }
}

/// Settings of a [`Client`]. Only the API key is required.
#[derive(Clone)]
pub struct ClientBuilder {
    api_key: Option<String>,
    base_url: String,
    settings: Settings,
    user_agent: Option<String>,
}

impl Default for ClientBuilder {
    fn default() -> Self {
        Self {
            api_key: None,
            base_url: DEFAULT_BASE_URL.to_string(),
            settings: Settings { chain_id: 1, timeout: Duration::from_secs(30), max_retries: 2, max_retry_wait: Duration::from_secs(60) },
            user_agent: None,
        }
    }
}

impl ClientBuilder {
    /// Your TraceFast API key (`tf_live_…`). Required. Server-side only.
    ///
    /// It must be non-empty and made only of visible ASCII characters (`0x21` to
    /// `0x7E`): a key with a space, a line break or a typographic character is
    /// refused by [`build`](Self::build).
    pub fn api_key(mut self, api_key: impl Into<String>) -> Self {
        self.api_key = Some(api_key.into());
        self
    }

    /// Default [`DEFAULT_BASE_URL`]. Must be `https` (a loopback host may use
    /// `http`), without a query or a fragment.
    pub fn base_url(mut self, base_url: impl Into<String>) -> Self {
        self.base_url = base_url.into();
        self
    }

    /// Default `1`, Ethereum mainnet — the only chain served today.
    pub fn chain_id(mut self, chain_id: u64) -> Self {
        self.settings.chain_id = chain_id;
        self
    }

    /// The longest one attempt may take, from connecting to the last byte of the
    /// body. Default 30 seconds. A timeout is not retried, so the worst case of a
    /// call is `(max_retries + 1) × timeout` plus the waits between attempts.
    pub fn timeout(mut self, timeout: Duration) -> Self {
        self.settings.timeout = timeout;
        self
    }

    /// Retries of answers that say "try again" (`429` with a short `Retry-After`,
    /// `503`, a connection failure). Default 2; `0` turns retries off.
    pub fn max_retries(mut self, max_retries: u32) -> Self {
        self.settings.max_retries = max_retries;
        self
    }

    /// The longest `Retry-After` the client waits out before retrying. Default 60
    /// seconds. An answer that asks for a longer wait is returned at once, its
    /// [`Error::retry_after`] telling the real wait.
    pub fn max_retry_wait(mut self, max_retry_wait: Duration) -> Self {
        self.settings.max_retry_wait = max_retry_wait;
        self
    }

    /// Appended to the `User-Agent` header, e.g. `my-app/1.2`.
    pub fn user_agent(mut self, suffix: impl Into<String>) -> Self {
        self.user_agent = Some(suffix.into());
        self
    }

    /// The client.
    pub fn build(self) -> Result<Client, ConfigError> {
        let key = self.api_key.filter(|k| !k.is_empty()).ok_or(ConfigError::MissingApiKey)?;
        if !key.bytes().all(|b| (0x21..=0x7E).contains(&b)) {
            return Err(ConfigError::InvalidApiKey);
        }
        let mut key = HeaderValue::from_str(&key).map_err(|_| ConfigError::InvalidApiKey)?;
        key.set_sensitive(true);
        let base_url = self.base_url.trim_end_matches('/').to_string();
        check_base_url(&base_url)?;
        let agent = match &self.user_agent {
            Some(suffix) if !suffix.is_empty() => format!("tracefast-rust/{} {suffix}", crate::VERSION),
            _ => format!("tracefast-rust/{}", crate::VERSION),
        };
        let agent = HeaderValue::from_str(&agent).map_err(|_| ConfigError::InvalidUserAgent)?;
        let mut headers = HeaderMap::new();
        headers.insert(ACCEPT, HeaderValue::from_static("application/json"));
        headers.insert("x-api-key", key);
        headers.insert(USER_AGENT, agent);
        let http = reqwest::Client::builder()
            // A redirect is never followed, so the key never travels to another host.
            .redirect(reqwest::redirect::Policy::none())
            .default_headers(headers)
            .build()
            .map_err(|e| ConfigError::Http(Box::new(e)))?;
        Ok(Client { http, base_url: base_url.into(), settings: self.settings })
    }
}

impl fmt::Debug for ClientBuilder {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ClientBuilder")
            .field("api_key", &self.api_key.as_ref().map(|_| "[redacted]"))
            .field("base_url", &self.base_url)
            .field("chain_id", &self.settings.chain_id)
            .field("timeout", &self.settings.timeout)
            .field("max_retries", &self.settings.max_retries)
            .field("max_retry_wait", &self.settings.max_retry_wait)
            .field("user_agent", &self.user_agent)
            .finish()
    }
}

fn check_base_url(base_url: &str) -> Result<(), ConfigError> {
    let url = Url::parse(base_url).map_err(|_| ConfigError::InvalidBaseUrl("it is not a URL"))?;
    if url.query().is_some() || url.fragment().is_some() {
        return Err(ConfigError::InvalidBaseUrl("it must not carry a query or a fragment"));
    }
    let loopback = url.host_str().is_some_and(|h| {
        h == "localhost"
            || h.parse::<Ipv4Addr>().is_ok_and(|ip| ip.is_loopback())
            || h.trim_start_matches('[').trim_end_matches(']').parse::<Ipv6Addr>().is_ok_and(|ip| ip.is_loopback())
    });
    match url.scheme() {
        "https" => Ok(()),
        "http" if loopback => Ok(()),
        _ => Err(ConfigError::InvalidBaseUrl("it must use https (http only for a loopback host)")),
    }
}

/// A client that cannot be built: the settings are wrong. No message repeats
/// the API key.
#[derive(Debug)]
#[non_exhaustive]
pub enum ConfigError {
    /// No API key, or an empty one.
    MissingApiKey,
    /// The API key holds a character other than visible ASCII (`0x21` to `0x7E`):
    /// a space, a line break, a typographic character.
    InvalidApiKey,
    /// The base URL is refused; the text says why.
    InvalidBaseUrl(&'static str),
    /// The user agent suffix holds characters an HTTP header cannot carry.
    InvalidUserAgent,
    /// The HTTP client could not start; the source says why.
    Http(Box<dyn std::error::Error + Send + Sync>),
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingApiKey => f.write_str("an API key is required"),
            Self::InvalidApiKey => f.write_str(
                "the API key may hold only visible ASCII characters: check it for a space, a line break or a typographic character",
            ),
            Self::InvalidBaseUrl(why) => write!(f, "the base URL is refused: {why}"),
            Self::InvalidUserAgent => f.write_str("the user agent suffix holds characters an HTTP header cannot carry"),
            Self::Http(_) => f.write_str("the HTTP client could not start"),
        }
    }
}

impl std::error::Error for ConfigError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Http(e) => Some(e.as_ref()),
            _ => None,
        }
    }
}

/// An answer with its whole body.
struct Answer {
    status: u16,
    request_id: Option<String>,
    retry_after: Option<Duration>,
    body: Vec<u8>,
}

/// A path value, percent-encoded as JavaScript's `encodeURIComponent` does.
pub(crate) fn encode_path(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for b in value.bytes() {
        if b.is_ascii_alphanumeric() || b"-_.!~*'()".contains(&b) {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// A failure of the transport: `timeout`, or `connection_error` — with `status`
/// `0` when no status line arrived, the answer's status when its body was cut off.
fn transport_error(status: u16, timeout: Duration, cause: reqwest::Error) -> Error {
    if cause.is_timeout() {
        Error::new(status, "timeout", format!("no complete answer within {} ms", timeout.as_millis())).with_cause(cause)
    } else if status == 0 {
        Error::new(0, "connection_error", "the connection failed before an answer arrived").with_cause(cause)
    } else {
        Error::new(status, "connection_error", "the answer was cut off").with_cause(cause)
    }
}

/// A non-2xx answer as an [`Error`]: the API's error envelope when it is one,
/// `unexpected_response` otherwise — and always for a `3xx`, which is never followed.
fn to_error(answer: Answer) -> Error {
    let status = answer.status;
    let error = if (300..400).contains(&status) {
        Error::new(status, "unexpected_response", "a redirect, which is never followed")
    } else {
        match serde_json::from_slice::<Value>(&answer.body) {
            Ok(Value::Object(mut envelope)) => match envelope.remove("error") {
                Some(Value::Object(body)) => Error::from_body(status, body),
                _ => None,
            },
            _ => None,
        }
        .unwrap_or_else(|| Error::new(status, "unexpected_response", "an answer without the API's error body"))
    };
    error.with_header_request_id(answer.request_id).with_retry_after(answer.retry_after)
}

/// How long to wait before retrying after this error, or `None` when it is
/// returned at once (SPEC §5).
fn retry_delay(error: &Error, attempt: u32, max_retry_wait: Duration) -> Option<Duration> {
    if error.code() == "connection_error" {
        return Some(backoff(attempt));
    }
    match (error.status(), error.retry_after()) {
        (429, _) if error.code() == "quota_exceeded" => None,
        (429, Some(wait)) | (503, Some(wait)) => (wait <= max_retry_wait).then_some(wait),
        (503, None) => Some(backoff(attempt)),
        _ => None,
    }
}

/// `Retry-After` exactly as sent: a number of seconds, or an HTTP date as the
/// seconds from now (rounded up, never negative).
fn parse_retry_after(value: &str) -> Option<Duration> {
    let value = value.trim();
    if let Ok(s) = value.parse::<f64>() {
        return if s >= 0.0 { Duration::try_from_secs_f64(s).ok() } else { None };
    }
    let at = parse_http_date(value)?;
    let now = SystemTime::now().duration_since(UNIX_EPOCH).ok()?.as_secs_f64();
    Some(Duration::from_secs((at as f64 - now).ceil().max(0.0) as u64))
}

/// An IMF-fixdate (`Sun, 06 Nov 1994 08:49:37 GMT`) as Unix seconds.
fn parse_http_date(value: &str) -> Option<i64> {
    let parts: Vec<&str> = value.split_whitespace().collect();
    let [_, day, month, year, time, "GMT"] = parts.as_slice() else { return None };
    let month =
        ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"].iter().position(|m| m == month)? as i64 + 1;
    let (day, year): (i64, i64) = (day.parse().ok()?, year.parse().ok()?);
    let hms: Vec<i64> = time.split(':').map(|x| x.parse().ok()).collect::<Option<_>>()?;
    let [h, m, s] = hms.as_slice() else { return None };
    // Days from the civil date (Howard Hinnant's algorithm).
    let y = if month <= 2 { year - 1 } else { year };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (month + 9) % 12;
    let doy = (153 * mp + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    Some(days * 86_400 + h * 3600 + m * 60 + s)
}

/// Exponential backoff from 0.5 s with jitter, each wait at most 8 s:
/// 0.25–0.5 s, 0.5–1 s, 1–2 s, 2–4 s, 4–8 s, 4–8 s, …
fn backoff(attempt: u32) -> Duration {
    let base = (500.0 * 2f64.powi(attempt.min(16) as i32)).min(MAX_BACKOFF.as_millis() as f64);
    let jitter = RandomState::new().build_hasher().finish() as f64 / u64::MAX as f64;
    Duration::from_millis((base * (0.5 + jitter / 2.0)) as u64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encodes_like_encode_uri_component() {
        assert_eq!(encode_path("address_nature:eip7702_delegated_currently"), "address_nature%3Aeip7702_delegated_currently");
        assert_eq!(encode_path("a b/c?d#é"), "a%20b%2Fc%3Fd%23%C3%A9");
        assert_eq!(encode_path("0xAbC-_.!~*'()"), "0xAbC-_.!~*'()");
    }

    #[test]
    fn retry_after_is_kept_as_sent() {
        assert_eq!(parse_retry_after("0"), Some(Duration::ZERO));
        assert_eq!(parse_retry_after("7"), Some(Duration::from_secs(7)));
        assert_eq!(parse_retry_after("3600"), Some(Duration::from_secs(3600)), "not capped");
        assert_eq!(parse_retry_after("1.5"), Some(Duration::from_millis(1500)));
        assert_eq!(parse_retry_after("-1"), None);
        assert_eq!(parse_retry_after("1e300"), None);
        assert_eq!(parse_retry_after("soon"), None);
        assert_eq!(parse_http_date("Sun, 06 Nov 1994 08:49:37 GMT"), Some(784_111_777));
        assert_eq!(parse_retry_after("Sun, 06 Nov 1994 08:49:37 GMT"), Some(Duration::ZERO));
    }

    fn refusal(status: u16, code: &str, retry_after: Option<u64>) -> Error {
        Error::new(status, code, "m").with_retry_after(retry_after.map(Duration::from_secs))
    }

    #[test]
    fn only_try_again_answers_are_retried() {
        let max = Duration::from_secs(60);
        let wait = |e: &Error| retry_delay(e, 0, max);
        assert_eq!(wait(&refusal(429, "rate_limited", Some(3))), Some(Duration::from_secs(3)));
        assert_eq!(wait(&refusal(429, "rate_limited", Some(60))), Some(Duration::from_secs(60)));
        assert_eq!(wait(&refusal(429, "rate_limited", Some(61))), None, "longer than max_retry_wait");
        assert_eq!(wait(&refusal(429, "concurrency_limited", None)), None, "no Retry-After");
        assert_eq!(wait(&refusal(429, "quota_exceeded", Some(1))), None, "a quota is never retried");
        assert_eq!(wait(&refusal(503, "service_unavailable", Some(1))), Some(Duration::from_secs(1)));
        assert_eq!(wait(&refusal(503, "service_unavailable", Some(3600))), None);
        assert!(wait(&refusal(503, "service_unavailable", None)).is_some());
        assert!(wait(&refusal(0, "connection_error", None)).is_some());
        assert!(wait(&refusal(200, "connection_error", None)).is_some(), "a body cut off");
        assert_eq!(wait(&refusal(0, "timeout", None)), None);
        for status in [400, 401, 402, 403, 404, 422, 500, 502] {
            assert_eq!(wait(&refusal(status, "x", Some(1))), None, "status {status}");
        }
    }

    #[test]
    fn backoff_grows_with_jitter_and_is_capped() {
        for _ in 0..50 {
            let d = backoff(0).as_millis();
            assert!((250..=500).contains(&d), "{d}");
            let d = backoff(2).as_millis();
            assert!((1000..=2000).contains(&d), "{d}");
            let d = backoff(30).as_millis();
            assert!((4000..=8000).contains(&d), "{d}");
        }
    }

    #[test]
    fn path_values_are_encoded_and_dot_segments_refused() {
        let client = Client::new("k").unwrap();
        assert_eq!(client.path("/chains/{chainId}/x/{a}/{b}", &["a b", "0x1"]).unwrap(), "/chains/1/x/a%20b/0x1");
        for bad in ["", ".", ".."] {
            let e = client.path("/chains/{chainId}/x/{a}", &[bad]).unwrap_err();
            assert_eq!(e.code(), "invalid_argument");
            assert_eq!(e.status(), 0);
            assert!(e.is_invalid_argument());
        }
        assert_eq!(client.path("/x/{a}", &["..."]).unwrap(), "/x/...");
    }

    #[test]
    fn base_url_must_be_https_or_loopback() {
        for ok in ["https://example.com/api/v1", "http://localhost/api", "http://127.0.0.1/api", "http://[::1]/api", "https://a.b/"] {
            assert!(check_base_url(ok.trim_end_matches('/')).is_ok(), "{ok}");
        }
        for bad in ["http://example.com/api", "https://example.com/api?x=1", "https://example.com/api#f", "ftp://example.com", "nope"] {
            assert!(matches!(check_base_url(bad), Err(ConfigError::InvalidBaseUrl(_))), "{bad}");
        }
    }
}
