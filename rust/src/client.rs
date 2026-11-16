use std::collections::hash_map::RandomState;
use std::fmt;
use std::hash::{BuildHasher, Hasher};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use reqwest::header::{ACCEPT, HeaderMap, HeaderValue, RETRY_AFTER, USER_AGENT};
use reqwest::{Response, Url};
use serde::de::DeserializeOwned;

use crate::error::Error;

/// The API's address, used unless [`ClientBuilder::base_url`] says otherwise.
pub const DEFAULT_BASE_URL: &str = "https://tracefast.xyz/api/v1";

const MAX_RETRY_AFTER_S: u64 = 60;

/// The TraceFast API client.
///
/// Cheap to clone: clones share one connection pool. Methods are grouped by
/// namespace — `client.contracts().proxy(address)`, `client.blocks().latest()` —
/// and [`Client::resolve`] lives on the client itself.
///
/// ```no_run
/// # async fn run() -> Result<(), Box<dyn std::error::Error>> {
/// let client = tracefast::Client::builder()
///     .api_key(std::env::var("TRACEFAST_API_KEY")?)
///     .user_agent("my-app/1.2")
///     .build()?;
/// let latest = client.blocks().latest().await?;
/// println!("block {}", latest.data.number);
/// # Ok(())
/// # }
/// ```
#[derive(Clone)]
pub struct Client {
    inner: Arc<Inner>,
}

struct Inner {
    http: reqwest::Client,
    headers: HeaderMap,
    base_url: String,
    chain_id: u64,
    timeout: Duration,
    max_retries: u32,
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
        self.inner.chain_id
    }

    /// The API's address the client talks to.
    pub fn base_url(&self) -> &str {
        &self.inner.base_url
    }

    /// One `GET`, with the retries of SPEC §5.
    pub(crate) async fn get<T: DeserializeOwned>(&self, path: &str, query: &[(&'static str, String)]) -> Result<T, Error> {
        let url = self.url(path, query)?;
        let inner = &self.inner;
        let mut attempt = 0;
        loop {
            let sent = inner.http.get(url.clone()).headers(inner.headers.clone()).timeout(inner.timeout).send().await;
            let res = match sent {
                Ok(res) => res,
                Err(e) if e.is_timeout() => return Err(timeout_error(inner.timeout, e)),
                Err(e) => {
                    if attempt < inner.max_retries {
                        tokio::time::sleep(backoff(attempt)).await;
                        attempt += 1;
                        continue;
                    }
                    return Err(Error::new(0, "connection_error", format!("could not reach {}", inner.base_url)).with_cause(e));
                }
            };
            let status = res.status().as_u16();
            if res.status().is_success() {
                let request_id = header(&res, "x-request-id");
                let body = res.bytes().await.map_err(|e| {
                    if e.is_timeout() {
                        timeout_error(inner.timeout, e)
                    } else {
                        Error::new(0, "connection_error", "the answer was cut off").with_cause(e)
                    }
                })?;
                return serde_json::from_slice(&body).map_err(|e| {
                    Error::new(status, "unexpected_response", format!("HTTP {status} with a body that is not the expected answer"))
                        .with_request_id(request_id)
                        .with_cause(e)
                });
            }
            let error = to_error(res).await;
            if let Some(wait) = retry_delay(status, error.retry_after(), attempt) {
                if attempt < inner.max_retries {
                    tokio::time::sleep(wait).await;
                    attempt += 1;
                    continue;
                }
            }
            return Err(error);
        }
    }

    fn url(&self, path: &str, query: &[(&'static str, String)]) -> Result<Url, Error> {
        let mut url = Url::parse(&format!("{}{}", self.inner.base_url, path))
            .map_err(|e| Error::new(0, "connection_error", "the request URL is not valid").with_cause(e))?;
        if !query.is_empty() {
            url.query_pairs_mut().extend_pairs(query.iter().map(|(k, v)| (*k, v.as_str())));
        }
        Ok(url)
    }
}

impl fmt::Debug for Client {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Client")
            .field("api_key", &"[redacted]")
            .field("base_url", &self.inner.base_url)
            .field("chain_id", &self.inner.chain_id)
            .field("timeout", &self.inner.timeout)
            .field("max_retries", &self.inner.max_retries)
            .finish()
    }
}

/// Settings of a [`Client`]. Only the API key is required.
#[derive(Clone)]
pub struct ClientBuilder {
    api_key: Option<String>,
    base_url: String,
    chain_id: u64,
    timeout: Duration,
    max_retries: u32,
    user_agent: Option<String>,
}

impl Default for ClientBuilder {
    fn default() -> Self {
        Self {
            api_key: None,
            base_url: DEFAULT_BASE_URL.to_string(),
            chain_id: 1,
            timeout: Duration::from_secs(30),
            max_retries: 2,
            user_agent: None,
        }
    }
}

impl ClientBuilder {
    /// Your TraceFast API key (`tf_live_…`). Required. Server-side only.
    pub fn api_key(mut self, api_key: impl Into<String>) -> Self {
        self.api_key = Some(api_key.into());
        self
    }

    /// Default [`DEFAULT_BASE_URL`].
    pub fn base_url(mut self, base_url: impl Into<String>) -> Self {
        self.base_url = base_url.into();
        self
    }

    /// Default `1`, Ethereum mainnet — the only chain served today.
    pub fn chain_id(mut self, chain_id: u64) -> Self {
        self.chain_id = chain_id;
        self
    }

    /// The longest one request may take, answer included. Default 30 seconds.
    pub fn timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    /// Retries of answers that say "try again" (`429` with `Retry-After`, `503`,
    /// a connection failure). Default 2.
    pub fn max_retries(mut self, max_retries: u32) -> Self {
        self.max_retries = max_retries;
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
        let mut key = HeaderValue::from_str(&key).map_err(|_| ConfigError::InvalidApiKey)?;
        key.set_sensitive(true);
        let base_url = self.base_url.trim_end_matches('/').to_string();
        match Url::parse(&base_url) {
            Ok(u) if matches!(u.scheme(), "http" | "https") && u.query().is_none() => {}
            _ => return Err(ConfigError::InvalidBaseUrl),
        }
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
            .build()
            .map_err(|e| ConfigError::Http(e.to_string()))?;
        Ok(Client {
            inner: Arc::new(Inner {
                http,
                headers,
                base_url,
                chain_id: self.chain_id,
                timeout: self.timeout,
                max_retries: self.max_retries,
            }),
        })
    }
}

impl fmt::Debug for ClientBuilder {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ClientBuilder")
            .field("api_key", &self.api_key.as_ref().map(|_| "[redacted]"))
            .field("base_url", &self.base_url)
            .field("chain_id", &self.chain_id)
            .field("timeout", &self.timeout)
            .field("max_retries", &self.max_retries)
            .field("user_agent", &self.user_agent)
            .finish()
    }
}

/// A client that cannot be built: the settings are wrong.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum ConfigError {
    /// No API key, or an empty one.
    MissingApiKey,
    /// The API key holds characters an HTTP header cannot carry.
    InvalidApiKey,
    /// The base URL is not an `http`/`https` URL without a query.
    InvalidBaseUrl,
    /// The user agent suffix holds characters an HTTP header cannot carry.
    InvalidUserAgent,
    /// The HTTP client could not start.
    Http(String),
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingApiKey => f.write_str("TraceFast: an API key is required"),
            Self::InvalidApiKey => f.write_str("TraceFast: the API key holds characters an HTTP header cannot carry"),
            Self::InvalidBaseUrl => f.write_str("TraceFast: the base URL must be an http or https URL without a query"),
            Self::InvalidUserAgent => f.write_str("TraceFast: the user agent suffix holds characters an HTTP header cannot carry"),
            Self::Http(e) => write!(f, "TraceFast: the HTTP client could not start: {e}"),
        }
    }
}

impl std::error::Error for ConfigError {}

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

fn header(res: &Response, name: &str) -> Option<String> {
    res.headers().get(name)?.to_str().ok().map(str::to_string)
}

fn timeout_error(timeout: Duration, cause: reqwest::Error) -> Error {
    Error::new(0, "timeout", format!("no answer within {} ms", timeout.as_millis())).with_cause(cause)
}

/// A non-2xx answer as an [`Error`]: the API's error envelope when it is one,
/// `unexpected_response` otherwise — and always for a `3xx`, which is never followed.
async fn to_error(res: Response) -> Error {
    let status = res.status().as_u16();
    let retry_after = res.headers().get(RETRY_AFTER).and_then(|v| v.to_str().ok()).and_then(parse_retry_after);
    let header_id = header(&res, "x-request-id");
    let body = res.bytes().await.unwrap_or_default();
    let envelope = serde_json::from_slice::<serde_json::Value>(&body).ok();
    let e = envelope.as_ref().and_then(|b| b.get("error")).and_then(|e| e.as_object());
    // A redirect is never followed (SPEC §1), whatever its body says.
    let code = if (300..400).contains(&status) { None } else { e.and_then(|e| e.get("code")).and_then(|c| c.as_str()) };
    let error = match code {
        Some(code) => {
            let e = e.expect("an error object");
            Error::new(status, code, e.get("message").and_then(|m| m.as_str()).unwrap_or(code))
                .with_details(e.get("details").and_then(|d| d.as_object()).cloned())
                .with_request_id(e.get("requestId").and_then(|r| r.as_str()).map(str::to_string).or(header_id))
        }
        None => Error::new(status, "unexpected_response", format!("HTTP {status} without the API's error body")).with_request_id(header_id),
    };
    error.with_retry_after(retry_after)
}

/// How long to wait before retrying this answer, or `None` if it is not retried (SPEC §5).
fn retry_delay(status: u16, retry_after: Option<u64>, attempt: u32) -> Option<Duration> {
    match (status, retry_after) {
        (429, Some(s)) => Some(Duration::from_secs(s)),
        (429, None) => None,
        (503, Some(s)) => Some(Duration::from_secs(s)),
        (503, None) => Some(backoff(attempt)),
        _ => None,
    }
}

/// `Retry-After` as seconds — a number of seconds or an HTTP date — capped at 60.
fn parse_retry_after(value: &str) -> Option<u64> {
    let value = value.trim();
    if let Ok(s) = value.parse::<f64>() {
        return (s.is_finite() && s >= 0.0).then(|| (s.ceil() as u64).min(MAX_RETRY_AFTER_S));
    }
    let at = parse_http_date(value)?;
    let now = SystemTime::now().duration_since(UNIX_EPOCH).ok()?.as_secs() as i64;
    Some((at - now).clamp(0, MAX_RETRY_AFTER_S as i64) as u64)
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

/// Exponential backoff from 0.5 s with jitter: 0.25–0.5 s, 0.5–1 s, 1–2 s, …
fn backoff(attempt: u32) -> Duration {
    let base = 500.0 * 2f64.powi(attempt.min(10) as i32);
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
    fn retry_after_seconds_and_dates() {
        assert_eq!(parse_retry_after("0"), Some(0));
        assert_eq!(parse_retry_after("7"), Some(7));
        assert_eq!(parse_retry_after("3600"), Some(60));
        assert_eq!(parse_retry_after("-1"), None);
        assert_eq!(parse_retry_after("soon"), None);
        assert_eq!(parse_http_date("Sun, 06 Nov 1994 08:49:37 GMT"), Some(784_111_777));
        assert_eq!(parse_retry_after("Sun, 06 Nov 1994 08:49:37 GMT"), Some(0));
    }

    #[test]
    fn only_try_again_answers_are_retried() {
        assert_eq!(retry_delay(429, Some(3), 0), Some(Duration::from_secs(3)));
        assert_eq!(retry_delay(429, None, 0), None);
        assert_eq!(retry_delay(503, Some(1), 0), Some(Duration::from_secs(1)));
        assert!(retry_delay(503, None, 0).is_some());
        for status in [400, 401, 402, 403, 404, 422, 500, 502] {
            assert_eq!(retry_delay(status, Some(1), 0), None, "status {status}");
        }
    }

    #[test]
    fn backoff_grows_with_jitter() {
        for _ in 0..50 {
            let d = backoff(0).as_millis();
            assert!((250..=500).contains(&d), "{d}");
            let d = backoff(2).as_millis();
            assert!((1000..=2000).contains(&d), "{d}");
        }
    }
}
