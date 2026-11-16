//! The behaviour every TraceFast SDK shares (SPEC.md): errors, retries, pages,
//! the client's configuration and the key's hygiene.

mod common;

use std::net::{Ipv4Addr, SocketAddr, TcpListener};
use std::time::Duration;

use serde_json::{Value, json};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tracefast::types::{BlockResponse, EntityTransactionsResponse, ProxyImplementation};
use tracefast::{
    AddressesPortfolioParams, AddressesTransactionsParams, Client, ConfigError, Error, ErrorKind, Items, Paged, Pager, ResponseFuture,
};
use wiremock::matchers::any;
use wiremock::{Mock, MockServer, ResponseTemplate};

use common::{KEY, client_for, fixture, header};

const USDC: &str = "0xa0b86991c6218b36c1d19d4a2e9eb0ce3606eb48";

/// Answers in this order, each once.
async fn server(answers: Vec<ResponseTemplate>) -> MockServer {
    let server = MockServer::start().await;
    for answer in answers {
        Mock::given(any()).respond_with(answer).up_to_n_times(1).mount(&server).await;
    }
    server
}

fn ok(body: &Value) -> ResponseTemplate {
    ResponseTemplate::new(200).set_body_json(body)
}

fn refusal(status: u16, code: &str, message: &str) -> ResponseTemplate {
    ResponseTemplate::new(status).set_body_json(json!({ "error": { "code": code, "message": message } }))
}

async fn calls(server: &MockServer) -> Vec<wiremock::Request> {
    server.received_requests().await.expect("recording")
}

// ---- Errors ---------------------------------------------------------------

#[tokio::test]
async fn a_refusal_becomes_error_with_code_reason_details_body_and_request_id() {
    let body = json!({ "error": {
        "code": "block_not_available",
        "message": "past the covered history",
        "details": { "reason": "block_not_available", "maxAvailableBlock": 25901228 },
        "requestId": "req-1",
        "somethingNew": 7
    } });
    let s = server(vec![ResponseTemplate::new(422).set_body_json(&body).insert_header("x-request-id", "header-id")]).await;
    let e = client_for(&s, 2).addresses().portfolio(USDC, &AddressesPortfolioParams::new().block(99_999_999)).await.expect_err("a refusal");
    assert_eq!(e.status(), 422);
    assert_eq!(e.code(), "block_not_available");
    assert_eq!(e.kind(), ErrorKind::UnprocessableEntity);
    assert!(e.is_unprocessable_entity());
    assert_eq!(e.reason(), Some("block_not_available"));
    assert_eq!(e.details().and_then(|d| d.get("maxAvailableBlock")), Some(&json!(25901228)));
    assert_eq!(e.request_id(), Some("req-1"), "the body's id wins over the header's");
    assert_eq!(e.message(), "past the covered history");
    assert_eq!(e.body().and_then(|b| b.get("somethingNew")), Some(&json!(7)), "an unknown field survives");
    assert_eq!(e.to_string(), "422 block_not_available: past the covered history");
    assert_eq!(calls(&s).await.len(), 1, "422 is not retried");
}

#[tokio::test]
async fn a_plan_refusal_is_permission_denied_with_the_gateway_reason() {
    let body = json!({ "error": {
        "code": "forbidden",
        "reason": "plan_scope_missing",
        "message": "the plan does not include this method",
        "details": { "minimumPlan": "investigation", "currentPlan": "data" }
    } });
    let s = server(vec![ResponseTemplate::new(403).set_body_json(body)]).await;
    let e = client_for(&s, 2).addresses().transactions(USDC, &AddressesTransactionsParams::new()).await.expect_err("403");
    assert_eq!(e.kind(), ErrorKind::PermissionDenied);
    assert!(e.is_permission_denied());
    assert_eq!(e.reason(), Some("plan_scope_missing"), "error.reason before details.reason");
    assert_eq!(e.details().unwrap()["minimumPlan"], "investigation");
    assert_eq!(e.details().unwrap()["currentPlan"], "data");
}

#[tokio::test]
async fn a_rate_limit_names_its_bucket_and_the_wait_as_sent() {
    let body = json!({ "error": { "code": "quota_exceeded", "bucket": "quota_daily", "message": "daily quota" } });
    let s = server(vec![ResponseTemplate::new(429).set_body_json(body).insert_header("retry-after", "36000")]).await;
    let e = client_for(&s, 2).blocks().latest().await.expect_err("429");
    assert!(e.is_rate_limited());
    assert_eq!(e.bucket(), Some("quota_daily"));
    assert_eq!(e.retry_after(), Some(Duration::from_secs(36000)), "not capped");
    assert_eq!(calls(&s).await.len(), 1);
}

#[tokio::test]
async fn kinds_follow_the_status() {
    for (status, kind) in [
        (400, ErrorKind::BadRequest),
        (401, ErrorKind::Authentication),
        (402, ErrorKind::Other),
        (404, ErrorKind::NotFound),
        (500, ErrorKind::InternalServer),
        (502, ErrorKind::InternalServer),
    ] {
        let s = server(vec![refusal(status, "x", "y")]).await;
        let e = client_for(&s, 0).blocks().latest().await.expect_err("refused");
        assert_eq!(e.kind(), kind, "status {status}");
    }
}

#[tokio::test]
async fn a_body_that_is_not_the_error_envelope_is_unexpected_response() {
    let s =
        server(vec![ResponseTemplate::new(502).set_body_string("<html>bad gateway</html>").insert_header("x-request-id", "req-9")]).await;
    let e = client_for(&s, 2).blocks().latest().await.expect_err("a proxy page");
    assert_eq!(e.code(), "unexpected_response");
    assert_eq!(e.kind(), ErrorKind::UnexpectedResponse);
    assert_eq!(e.status(), 502);
    assert_eq!(e.request_id(), Some("req-9"));
    assert!(e.body().is_none());
}

#[tokio::test]
async fn a_success_body_that_is_not_the_answer_is_unexpected_response() {
    let s = server(vec![ResponseTemplate::new(200).set_body_string("<html>hello</html>")]).await;
    let e = client_for(&s, 2).blocks().latest().await.expect_err("not JSON");
    assert_eq!(e.code(), "unexpected_response");
    assert_eq!(e.status(), 200);
    assert!(std::error::Error::source(&e).is_some(), "the parse error is attached");
}

#[tokio::test]
async fn a_success_body_of_the_wrong_shape_is_unexpected_response() {
    let s = server(vec![ok(&json!({ "data": { "number": "not a number" }, "meta": {} }))]).await;
    let e = client_for(&s, 2).blocks().latest().await.expect_err("wrong shape");
    assert_eq!(e.code(), "unexpected_response");
    assert_eq!(e.status(), 200);
    let cause = std::error::Error::source(&e).expect("the serde error is attached");
    assert!(cause.is::<serde_json::Error>());
    assert_eq!(calls(&s).await.len(), 1, "a 2xx is not retried");
}

#[tokio::test]
async fn a_redirect_is_not_followed_it_is_an_unexpected_response() {
    let target = server(vec![ok(&fixture("getLatestBlock"))]).await;
    let redirect = ResponseTemplate::new(302)
        .insert_header("location", format!("{}/api/v1/chains/1/blocks/latest", target.uri()))
        .set_body_json(json!({ "error": { "code": "not_found", "message": "a body that looks like the API's" } }));
    let s = server(vec![redirect]).await;
    let e = client_for(&s, 2).blocks().latest().await.expect_err("a redirect");
    assert_eq!(e.code(), "unexpected_response");
    assert_eq!(e.status(), 302);
    assert_eq!(calls(&s).await.len(), 1);
    assert!(calls(&target).await.is_empty(), "the key never goes to the redirect's target");
}

#[tokio::test]
async fn an_empty_or_dot_path_value_is_refused_before_any_request() {
    let s = server(vec![ok(&fixture("getContractProxy"))]).await;
    let client = client_for(&s, 2);
    for bad in ["", ".", ".."] {
        let e = client.contracts().proxy(bad).await.expect_err("refused");
        assert_eq!(e.kind(), ErrorKind::InvalidArgument);
        assert_eq!(e.code(), "invalid_argument");
        assert_eq!(e.status(), 0);
    }
    let mut pages = client.addresses().transactions_pages("..", &AddressesTransactionsParams::new());
    assert!(pages.next_page().await.expect_err("refused").is_invalid_argument());
    assert!(pages.next_page().await.unwrap().is_none(), "the walk ends");
    assert!(calls(&s).await.is_empty());
}

// ---- The key --------------------------------------------------------------

#[tokio::test]
async fn the_key_never_appears_in_an_error_or_debug_output() {
    let s = server(vec![refusal(404, "not_found", "no such block")]).await;
    let client = client_for(&s, 2);
    let e = client.blocks().get(1).await.expect_err("404");
    assert_eq!(e.to_string(), "404 not_found: no such block");
    for text in [format!("{e}"), format!("{e:?}"), format!("{client:?}")] {
        assert!(!text.contains(KEY), "{text}");
    }
    let builder = Client::builder().api_key(KEY);
    assert!(!format!("{builder:?}").contains(KEY));

    let unreachable = Client::builder().api_key(KEY).base_url(closed_port_url()).max_retries(0).build().unwrap();
    let e = unreachable.blocks().latest().await.expect_err("nothing listens");
    let mut texts = vec![format!("{e}"), format!("{e:?}")];
    let mut cause = std::error::Error::source(&e);
    while let Some(c) = cause {
        texts.push(format!("{c} {c:?}"));
        cause = c.source();
    }
    for text in texts {
        assert!(!text.contains(KEY), "{text}");
    }
}

#[tokio::test]
async fn the_key_travels_only_in_the_header() {
    let s = server(vec![ok(&fixture("getLatestBlock"))]).await;
    client_for(&s, 0).blocks().latest().await.expect("ok");
    let req = &calls(&s).await[0];
    assert_eq!(header(req, "x-api-key").as_deref(), Some(KEY));
    assert!(!req.url.as_str().contains(KEY));
    assert_eq!(req.url.query(), None);
}

#[test]
fn a_key_must_be_visible_ascii_and_is_never_repeated() {
    assert!(matches!(Client::new(""), Err(ConfigError::MissingApiKey)));
    assert!(matches!(Client::builder().build(), Err(ConfigError::MissingApiKey)));
    for bad in ["tf_test_with space", "tf_test_line\nbreak", "tf_test_tab\t", "tf_test_\u{2019}quote", "tf_test_\u{e9}", "tf_test_\u{7f}"] {
        let e = Client::new(bad).expect_err(bad);
        assert!(matches!(e, ConfigError::InvalidApiKey), "{bad:?}");
        for text in [e.to_string(), format!("{e:?}")] {
            assert!(!text.contains("tf_test_"), "{text}");
        }
    }
    assert!(Client::new("tf_test_!~visible").is_ok());
}

// ---- Configuration --------------------------------------------------------

#[test]
fn defaults() {
    let client = Client::new("k").unwrap();
    assert_eq!(client.base_url(), "https://tracefast.xyz/api/v1");
    assert_eq!(client.chain_id(), 1);
    assert_eq!(client.timeout(), Duration::from_secs(30));
    assert_eq!(client.max_retries(), 2);
    assert_eq!(client.max_retry_wait(), Duration::from_secs(60));
}

#[test]
fn base_url_must_be_https_except_on_loopback() {
    let build = |url: &str| Client::builder().api_key("k").base_url(url).build();
    for bad in ["not a url", "http://example.com/api/v1", "https://example.com/api/v1#top", "https://example.com/api/v1?x=1"] {
        assert!(matches!(build(bad), Err(ConfigError::InvalidBaseUrl(_))), "{bad}");
    }
    for good in ["https://example.com/api/v1/", "http://localhost/api/v1", "http://[::1]/api/v1"] {
        assert!(build(good).is_ok(), "{good}");
    }
}

#[test]
fn a_config_error_converts_into_an_error() {
    fn build() -> tracefast::Result<Client> {
        Ok(Client::new("bad key")?)
    }
    let e = build().expect_err("refused");
    assert_eq!(e.kind(), ErrorKind::InvalidArgument);
    assert_eq!(e.code(), "invalid_argument");
    let source = std::error::Error::source(&e).expect("the config error");
    assert!(source.is::<ConfigError>());
}

#[tokio::test]
async fn chain_id_comes_from_the_client() {
    let latest = fixture("getLatestBlock");
    let s = server(vec![ok(&latest), ok(&latest), ok(&latest)]).await;
    client_for(&s, 0).blocks().latest().await.unwrap();
    let other = Client::builder().api_key(KEY).base_url(format!("{}/api/v1/", s.uri())).chain_id(17000).build().unwrap();
    assert_eq!(other.chain_id(), 17000);
    other.blocks().latest().await.unwrap();
    other.with_chain_id(5).blocks().latest().await.unwrap();
    let reqs = calls(&s).await;
    assert_eq!(reqs[0].url.path(), "/api/v1/chains/1/blocks/latest");
    assert_eq!(reqs[1].url.path(), "/api/v1/chains/17000/blocks/latest");
    assert_eq!(reqs[2].url.path(), "/api/v1/chains/5/blocks/latest");
}

#[tokio::test]
async fn a_copy_with_other_options_leaves_the_original_alone() {
    let busy = || refusal(503, "service_unavailable", "busy").insert_header("retry-after", "0");
    let s = server(vec![busy(), busy(), ok(&fixture("getLatestBlock"))]).await;
    let client = client_for(&s, 2);
    let impatient = client.with_max_retries(0).with_timeout(Duration::from_secs(5)).with_max_retry_wait(Duration::from_secs(1));
    assert_eq!(impatient.max_retries(), 0);
    assert_eq!(impatient.timeout(), Duration::from_secs(5));
    assert_eq!(impatient.max_retry_wait(), Duration::from_secs(1));
    assert_eq!(client.max_retries(), 2);
    impatient.blocks().latest().await.expect_err("not retried");
    assert_eq!(calls(&s).await.len(), 1);
    client.blocks().latest().await.expect("retried");
    assert_eq!(calls(&s).await.len(), 3);
}

#[tokio::test]
async fn path_values_are_percent_encoded_and_integers_are_integers() {
    let s = server(vec![ok(&fixture("getLatestBlock")), ok(&fixture("getAddressAttributeProof"))]).await;
    let client = client_for(&s, 0);
    client.blocks().get(25_920_000).await.unwrap();
    let _ = client.addresses().attribute_proof(USDC, "a b/c").await;
    let reqs = calls(&s).await;
    assert_eq!(reqs[0].url.path(), "/api/v1/chains/1/blocks/25920000");
    assert_eq!(reqs[1].url.path(), format!("/api/v1/chains/1/entities/{USDC}/attributes/a%20b%2Fc/proof"));
}

#[tokio::test]
async fn user_agent_names_the_sdk_and_the_suffix() {
    let s = server(vec![ok(&fixture("getLatestBlock"))]).await;
    let client = Client::builder().api_key(KEY).base_url(format!("{}/api/v1", s.uri())).user_agent("my-app/1.2").build().unwrap();
    client.blocks().latest().await.unwrap();
    let req = &calls(&s).await[0];
    assert_eq!(header(req, "user-agent"), Some(format!("tracefast-rust/{} my-app/1.2", tracefast::VERSION)));
    assert_eq!(header(req, "accept").as_deref(), Some("application/json"));
}

// ---- Retries --------------------------------------------------------------

#[tokio::test]
async fn retry_429_only_with_a_short_retry_after() {
    let latest = fixture("getLatestBlock");
    let limited = refusal(429, "rate_limited", "slow down").insert_header("retry-after", "0");
    let s = server(vec![limited, ok(&latest)]).await;
    let res = client_for(&s, 2).blocks().latest().await.expect("retried");
    assert_eq!(serde_json::to_value(&res.data).unwrap(), latest["data"]);
    assert_eq!(calls(&s).await.len(), 2);

    let s = server(vec![refusal(429, "concurrency_limited", "too many at once"), ok(&latest)]).await;
    let e = client_for(&s, 2).blocks().latest().await.expect_err("no Retry-After: returned at once");
    assert_eq!(e.retry_after(), None);
    assert_eq!(calls(&s).await.len(), 1);

    let long = refusal(429, "rate_limited", "later").insert_header("retry-after", "2");
    let s = server(vec![long, ok(&latest)]).await;
    let e = client_for(&s, 2).with_max_retry_wait(Duration::from_secs(1)).blocks().latest().await.expect_err("longer than max_retry_wait");
    assert_eq!(e.retry_after(), Some(Duration::from_secs(2)));
    assert_eq!(calls(&s).await.len(), 1);

    let quota = refusal(429, "quota_exceeded", "daily quota").insert_header("retry-after", "0");
    let s = server(vec![quota, ok(&latest)]).await;
    let e = client_for(&s, 2).blocks().latest().await.expect_err("a quota is never retried");
    assert_eq!(e.code(), "quota_exceeded");
    assert_eq!(calls(&s).await.len(), 1);
}

#[tokio::test]
async fn retry_503_but_not_400_404_422_500() {
    let latest = fixture("getLatestBlock");
    let busy = refusal(503, "service_unavailable", "busy").insert_header("retry-after", "0");
    let s = server(vec![busy, ok(&latest)]).await;
    client_for(&s, 2).blocks().latest().await.expect("retried");
    assert_eq!(calls(&s).await.len(), 2);

    for status in [400, 401, 402, 403, 404, 422, 500] {
        let s = server(vec![refusal(status, "x", "y"), ok(&latest)]).await;
        let e = client_for(&s, 2).blocks().latest().await.expect_err("returned at once");
        assert_eq!(e.status(), status);
        assert_eq!(calls(&s).await.len(), 1, "status {status}");
    }
}

#[tokio::test]
async fn retry_503_without_retry_after_backs_off() {
    let s = server(vec![refusal(503, "service_unavailable", "busy"), ok(&fixture("getLatestBlock"))]).await;
    let started = std::time::Instant::now();
    client_for(&s, 1).blocks().latest().await.expect("retried");
    assert!(started.elapsed() >= Duration::from_millis(250), "backoff starts at 0.5 s with jitter");
    assert_eq!(calls(&s).await.len(), 2);
}

#[tokio::test]
async fn retries_stop_at_max_retries() {
    let busy = || refusal(503, "service_unavailable", "busy").insert_header("retry-after", "0");
    let s = server(vec![busy(), busy(), busy(), busy()]).await;
    let e = client_for(&s, 2).blocks().latest().await.expect_err("still busy");
    assert_eq!(e.status(), 503);
    assert_eq!(e.retry_after(), Some(Duration::ZERO));
    assert_eq!(calls(&s).await.len(), 3);
}

/// A local address nothing listens on.
fn closed_port_url() -> String {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).expect("a free port");
    let addr = listener.local_addr().unwrap();
    drop(listener);
    format!("http://{addr}/api/v1")
}

#[tokio::test]
async fn a_network_failure_becomes_connection_error_after_the_retries() {
    let client = Client::builder().api_key(KEY).base_url(closed_port_url()).max_retries(1).build().unwrap();
    let e: Error = client.blocks().latest().await.expect_err("nothing listens");
    assert_eq!(e.code(), "connection_error");
    assert_eq!(e.kind(), ErrorKind::Connection);
    assert_eq!(e.status(), 0);
    assert!(std::error::Error::source(&e).is_some(), "the cause is attached");
}

/// A server whose first answer promises a body it cuts off, then answers whole.
async fn cut_off_then_whole(body: &Value) -> SocketAddr {
    let listener = tokio::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
    let addr = listener.local_addr().unwrap();
    let whole = serde_json::to_string(body).unwrap();
    tokio::spawn(async move {
        for n in 0.. {
            let Ok((mut socket, _)) = listener.accept().await else { return };
            let mut buf = [0u8; 4096];
            let _ = socket.read(&mut buf).await;
            let answer = if n == 0 {
                format!("HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\n\r\n{}", whole.len(), &whole[..10])
            } else {
                format!("HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\n\r\n{whole}", whole.len())
            };
            let _ = socket.write_all(answer.as_bytes()).await;
            let _ = socket.shutdown().await;
        }
    });
    addr
}

#[tokio::test]
async fn a_body_cut_off_is_a_connection_error_and_is_retried() {
    let latest = fixture("getLatestBlock");
    let addr = cut_off_then_whole(&latest).await;
    let client = Client::builder().api_key(KEY).base_url(format!("http://{addr}/api/v1")).max_retries(0).build().unwrap();
    let e = client.blocks().latest().await.expect_err("cut off");
    assert_eq!(e.code(), "connection_error");
    assert_eq!(e.status(), 200, "the status line arrived");

    let addr = cut_off_then_whole(&latest).await;
    let client = Client::builder().api_key(KEY).base_url(format!("http://{addr}/api/v1")).max_retries(1).build().unwrap();
    let res = client.blocks().latest().await.expect("retried");
    assert_eq!(serde_json::to_value(&res.data).unwrap(), latest["data"]);
}

#[tokio::test]
async fn a_slow_answer_becomes_timeout_and_is_not_retried() {
    let s = server(vec![ok(&fixture("getLatestBlock")).set_delay(Duration::from_secs(5))]).await;
    let client = client_for(&s, 2).with_timeout(Duration::from_millis(200));
    let e = client.blocks().latest().await.expect_err("too slow");
    assert_eq!(e.code(), "timeout");
    assert!(e.is_timeout());
    assert_eq!(e.status(), 0);
    assert_eq!(calls(&s).await.len(), 1, "a timeout is not retried");
}

// ---- Pagination -----------------------------------------------------------

fn page(cursor: Option<&str>, rows: usize) -> Value {
    let mut body = fixture("getEntityTransactions");
    let tx = body["data"]["transactions"][0].clone();
    body["data"]["transactions"] = Value::Array(vec![tx; rows]);
    let data = body["data"].as_object_mut().unwrap();
    match cursor {
        Some(c) => data.insert("nextCursor".into(), json!(c)),
        None => data.remove("nextCursor"),
    };
    body
}

#[tokio::test]
async fn pages_follow_next_cursor_with_the_same_parameters_and_stop_when_it_is_absent() {
    let s = server(vec![ok(&page(Some("c1"), 2)), ok(&page(None, 1))]).await;
    let client = client_for(&s, 0);
    let params = AddressesTransactionsParams::new().limit(2).direction("sent");
    let mut pager = client.addresses().transactions_pages(USDC, &params);
    let mut pages = Vec::new();
    while let Some(p) = pager.next_page().await.expect("a page") {
        pages.push(p);
    }
    assert_eq!(pages.len(), 2);
    assert_eq!(pages[0].next_cursor(), Some("c1"));
    assert_eq!(pages[0].items().len(), 2);
    assert_eq!(pages[1].next_cursor(), None);
    assert!(pager.next_page().await.unwrap().is_none(), "a finished pager stays finished");

    let reqs = calls(&s).await;
    assert_eq!(reqs.len(), 2);
    let q = |i: usize| reqs[i].url.query_pairs().map(|(k, v)| (k.into_owned(), v.into_owned())).collect::<Vec<_>>();
    assert_eq!(q(0), vec![("limit".into(), "2".into()), ("direction".into(), "sent".into())]);
    let second = q(1);
    assert_eq!(second.len(), 3);
    for pair in [("cursor", "c1"), ("limit", "2"), ("direction", "sent")] {
        assert!(second.contains(&(pair.0.into(), pair.1.into())), "{pair:?} in {second:?}");
    }
    assert_eq!(reqs[0].url.path(), reqs[1].url.path());
}

#[tokio::test]
async fn a_short_page_with_a_next_cursor_is_not_the_last_one() {
    let s = server(vec![ok(&page(Some("c1"), 1)), ok(&page(None, 0))]).await;
    let client = client_for(&s, 0);
    let mut pager = client.addresses().transactions_pages(USDC, &AddressesTransactionsParams::new().limit(50));
    let mut n = 0;
    while pager.next_page().await.expect("a page").is_some() {
        n += 1;
    }
    assert_eq!(n, 2);
}

#[tokio::test]
async fn a_pager_passes_a_given_cursor_once_then_the_next_one() {
    let s = server(vec![ok(&page(Some("c2"), 1)), ok(&page(None, 1))]).await;
    let client = client_for(&s, 0);
    let mut pager = client.addresses().transactions_pages(USDC, &AddressesTransactionsParams::new().cursor("c1"));
    while pager.next_page().await.unwrap().is_some() {}
    let reqs = calls(&s).await;
    let cursors: Vec<Vec<String>> =
        reqs.iter().map(|r| r.url.query_pairs().filter(|(k, _)| k == "cursor").map(|(_, v)| v.into_owned()).collect()).collect();
    assert_eq!(cursors, vec![vec!["c1".to_string()], vec!["c2".to_string()]]);
}

#[tokio::test]
async fn an_empty_or_repeated_cursor_is_unexpected_response() {
    for answers in [vec![ok(&page(Some(""), 1))], vec![ok(&page(Some("c1"), 1)), ok(&page(Some("c1"), 1))]] {
        let n = answers.len();
        let s = server(answers).await;
        let client = client_for(&s, 0);
        let mut pager = client.addresses().transactions_pages(USDC, &AddressesTransactionsParams::new());
        for _ in 1..n {
            pager.next_page().await.expect("a good page");
        }
        let e = pager.next_page().await.expect_err("a broken cursor");
        assert_eq!(e.code(), "unexpected_response");
        assert!(pager.next_page().await.unwrap().is_none(), "the walk ends");
        assert_eq!(calls(&s).await.len(), n);
    }
}

#[tokio::test]
async fn an_invalid_cursor_surfaces_as_an_error() {
    let s = server(vec![refusal(400, "invalid_cursor", "the block was reorganised")]).await;
    let client = client_for(&s, 2);
    let mut pager = client.addresses().transactions_pages(USDC, &AddressesTransactionsParams::new().cursor("old"));
    let e = pager.next_page().await.expect_err("400");
    assert_eq!(e.code(), "invalid_cursor");
    assert!(e.is_bad_request());
}

#[tokio::test]
async fn max_pages_stops_the_walk() {
    let s = server(vec![ok(&page(Some("c1"), 2)), ok(&page(Some("c2"), 2)), ok(&page(None, 2))]).await;
    let client = client_for(&s, 0);
    let mut pager = client.addresses().transactions_pages(USDC, &AddressesTransactionsParams::new()).max_pages(2);
    let mut n = 0;
    while pager.next_page().await.unwrap().is_some() {
        n += 1;
    }
    assert_eq!(n, 2);
    assert_eq!(calls(&s).await.len(), 2);
}

#[tokio::test]
async fn items_walk_the_rows_of_every_page() {
    let s = server(vec![ok(&page(Some("c1"), 2)), ok(&page(Some("c2"), 0)), ok(&page(None, 3))]).await;
    let client = client_for(&s, 0);
    let mut items = client.addresses().transactions_items(USDC, &AddressesTransactionsParams::new());
    let mut n = 0;
    while let Some(tx) = items.next_item().await.expect("a row") {
        assert!(!tx.hash.is_empty());
        n += 1;
    }
    assert_eq!(n, 5);
    assert_eq!(calls(&s).await.len(), 3);
}

#[tokio::test]
async fn max_items_stops_without_fetching_another_page() {
    let s = server(vec![ok(&page(Some("c1"), 2)), ok(&page(Some("c2"), 2)), ok(&page(None, 2))]).await;
    let client = client_for(&s, 0);
    let mut items = client.addresses().transactions_items(USDC, &AddressesTransactionsParams::new()).max_items(3);
    let mut n = 0;
    while items.next_item().await.unwrap().is_some() {
        n += 1;
    }
    assert_eq!(n, 3);
    assert_eq!(calls(&s).await.len(), 2);

    let s = server(vec![ok(&page(Some("c1"), 2))]).await;
    let client = client_for(&s, 0);
    let mut none = client.addresses().transactions_items(USDC, &AddressesTransactionsParams::new()).max_items(0);
    assert!(none.next_item().await.unwrap().is_none());
    assert!(calls(&s).await.is_empty());
}

#[cfg(feature = "stream")]
#[tokio::test]
async fn walkers_are_streams() {
    use futures::{StreamExt, TryStreamExt};

    let s = server(vec![ok(&page(Some("c1"), 2)), ok(&page(None, 1)), ok(&page(Some("c1"), 2)), ok(&page(None, 1))]).await;
    let client = client_for(&s, 0);
    let pages: Vec<EntityTransactionsResponse> =
        client.addresses().transactions_pages(USDC, &AddressesTransactionsParams::new()).try_collect().await.unwrap();
    assert_eq!(pages.len(), 2);
    let rows = client.addresses().transactions_items(USDC, &AddressesTransactionsParams::new()).collect::<Vec<_>>().await;
    assert_eq!(rows.len(), 3);
}

// ---- Responses ------------------------------------------------------------

#[tokio::test]
async fn unknown_fields_are_ignored_and_enums_are_open() {
    let mut body = fixture("getLatestBlock");
    body["data"]["somethingNew"] = json!({ "nested": [1, 2] });
    body["meta"]["alsoNew"] = json!(true);
    let s = server(vec![ok(&body)]).await;
    let res: BlockResponse = client_for(&s, 0).blocks().latest().await.expect("unknown fields are fine");
    assert_eq!(serde_json::to_value(&res).unwrap(), serde_json::to_value(fixture_typed::<BlockResponse>("getLatestBlock")).unwrap());

    // An enum value the catalogue does not list still deserializes.
    let mut proxy = fixture("getContractProxy");
    proxy["data"]["implementations"][0]["source"] = json!("a_source_from_the_future");
    let typed: tracefast::types::ProxyImplementationsResponse = serde_json::from_value(proxy).expect("open enum");
    assert_eq!(typed.data.implementations[0].source, "a_source_from_the_future");
}

fn fixture_typed<T: serde::de::DeserializeOwned>(id: &str) -> T {
    serde_json::from_value(fixture(id)).expect("fixture deserializes")
}

#[test]
fn absent_null_and_a_value_are_three_answers() {
    let row = fixture("getContractProxy")["data"]["implementations"][0].clone();
    let with = |v: Option<Value>| {
        let mut r = row.clone();
        let o = r.as_object_mut().unwrap();
        o.remove("supersededBy");
        if let Some(v) = v {
            o.insert("supersededBy".into(), v);
        }
        let typed: ProxyImplementation = serde_json::from_value(r).expect("a row");
        let back = serde_json::to_value(&typed).unwrap();
        (typed.superseded_by, back.get("supersededBy").cloned())
    };
    assert_eq!(with(None), (None, None), "absent stays absent");
    assert_eq!(with(Some(Value::Null)), (Some(None), Some(Value::Null)), "null stays null");
    assert_eq!(with(Some(json!("0xabc"))), (Some(Some("0xabc".to_string())), Some(json!("0xabc"))));
}

#[test]
fn response_types_have_defaults_for_test_values() {
    let mut block = tracefast::types::Block::default();
    block.number = 20_000_000;
    block.hash = "0x01".into();
    assert_eq!(serde_json::to_value(&block).unwrap()["number"], 20_000_000);
}

#[test]
fn the_strict_fixture_check_sees_what_the_types_drop() {
    let body = fixture("getLatestBlock");
    let typed: BlockResponse = serde_json::from_value(body.clone()).unwrap();
    assert!(common::round_trip(&body, &typed).is_empty());

    let mut extra = body.clone();
    extra["data"]["notInTheCatalogue"] = json!(1);
    let typed: BlockResponse = serde_json::from_value(extra.clone()).unwrap();
    assert_eq!(common::round_trip(&extra, &typed), vec!["$.data.notInTheCatalogue: not in the generated type".to_string()]);

    let mut row = fixture("getContractProxy");
    row["data"]["implementations"][0]["supersededBy"] = json!(null);
    row["data"]["implementations"][0].as_object_mut().unwrap().remove("supersededByBlock");
    let typed: tracefast::types::ProxyImplementationsResponse = serde_json::from_value(row.clone()).unwrap();
    assert!(common::round_trip(&row, &typed).is_empty(), "absent and null survive the round trip");
}

// ---- Futures, Send and Sync -----------------------------------------------

#[tokio::test]
async fn method_futures_borrow_only_the_client() {
    let proxy = fixture("getContractProxy");
    let s = server(vec![ok(&fixture("getLatestBlock")), ok(&proxy), ok(&proxy), ok(&proxy)]).await;
    let client = client_for(&s, 0);

    // A future outlives the handle it came from.
    let f = client.blocks().latest();
    f.await.unwrap();

    // Futures built in a loop from borrowed arguments, awaited together.
    let addrs: Vec<String> = vec![USDC.to_string(); 3];
    let futures: Vec<_> = addrs.iter().map(|a| client.contracts().proxy(a)).collect();
    for res in futures::future::join_all(futures).await {
        assert_eq!(res.unwrap().data.implementations.len(), 2);
    }

    // A temporary params struct does not have to outlive the future.
    let s = server(vec![ok(&page(None, 1))]).await;
    let client = client_for(&s, 0);
    let f = client.addresses().transactions(USDC, &AddressesTransactionsParams::new().limit(1));
    f.await.unwrap();
}

#[tokio::test]
async fn a_cloned_client_and_its_walkers_move_into_a_task() {
    let s = server(vec![ok(&fixture("getLatestBlock")), ok(&page(None, 2))]).await;
    let client = client_for(&s, 0);
    let task = {
        let client = client.clone();
        tokio::spawn(async move { client.blocks().latest().await.map(|b| b.data.number) })
    };
    assert!(task.await.unwrap().unwrap() > 0);
    let mut items = client.addresses().transactions_items(USDC, &AddressesTransactionsParams::new());
    let rows = tokio::spawn(async move {
        let mut n = 0;
        while items.next_item().await?.is_some() {
            n += 1;
        }
        Ok::<_, Error>(n)
    });
    assert_eq!(rows.await.unwrap().unwrap(), 2);
}

#[test]
fn public_types_are_send_and_sync() {
    fn send_sync<T: Send + Sync>() {}
    send_sync::<Client>();
    send_sync::<Error>();
    send_sync::<ConfigError>();
    send_sync::<Pager<EntityTransactionsResponse>>();
    send_sync::<Items<EntityTransactionsResponse>>();
    send_sync::<ResponseFuture<'static, BlockResponse>>();
    send_sync::<tracefast::BlocksApi<'static>>();
}
