//! The behaviour every TraceFast SDK shares (SPEC.md): errors, retries, pages,
//! the client's configuration and the key's hygiene.

mod common;

use std::net::{Ipv4Addr, TcpListener};
use std::time::Duration;

use serde_json::{Value, json};
use tracefast::types::{BlockResponse, ProxyImplementation};
use tracefast::{AddressesPortfolioParams, AddressesTransactionsParams, Client, ConfigError, Error, Paged};
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

#[tokio::test]
async fn a_refusal_becomes_error_with_code_reason_details_and_request_id() {
    let body = json!({ "error": {
        "code": "block_not_available",
        "message": "past the covered history",
        "details": { "reason": "block_not_available", "maxAvailableBlock": 25901228 },
        "requestId": "req-1"
    } });
    let s = server(vec![ResponseTemplate::new(422).set_body_json(body)]).await;
    let e = client_for(&s, 2).addresses().portfolio(USDC, &AddressesPortfolioParams::new().block(99_999_999)).await.expect_err("a refusal");
    assert_eq!(e.status(), 422);
    assert_eq!(e.code(), "block_not_available");
    assert_eq!(e.reason(), Some("block_not_available"));
    assert_eq!(e.details().and_then(|d| d.get("maxAvailableBlock")), Some(&json!(25901228)));
    assert_eq!(e.request_id(), Some("req-1"));
    assert_eq!(e.message(), "past the covered history");
    assert_eq!(calls(&s).await.len(), 1, "422 is not retried");
}

#[tokio::test]
async fn a_body_that_is_not_the_error_envelope_is_unexpected_response() {
    let s =
        server(vec![ResponseTemplate::new(502).set_body_string("<html>bad gateway</html>").insert_header("x-request-id", "req-9")]).await;
    let e = client_for(&s, 2).blocks().latest().await.expect_err("a proxy page");
    assert_eq!(e.code(), "unexpected_response");
    assert_eq!(e.status(), 502);
    assert_eq!(e.request_id(), Some("req-9"));
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
async fn the_key_never_appears_in_an_error_or_debug_output() {
    let s = server(vec![refusal(404, "not_found", "no such block")]).await;
    let client = client_for(&s, 2);
    let e = client.blocks().get("1").await.expect_err("404");
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

#[tokio::test]
async fn retry_429_only_with_retry_after() {
    let latest = fixture("getLatestBlock");
    let limited = refusal(429, "rate_limited", "slow down").insert_header("retry-after", "0");
    let s = server(vec![limited, ok(&latest)]).await;
    let res = client_for(&s, 2).blocks().latest().await.expect("retried");
    assert_eq!(serde_json::to_value(&res.data).unwrap(), latest["data"]);
    assert_eq!(calls(&s).await.len(), 2);

    let s = server(vec![refusal(429, "quota_exceeded", "daily quota"), ok(&latest)]).await;
    let e = client_for(&s, 2).blocks().latest().await.expect_err("returned at once");
    assert_eq!(e.code(), "quota_exceeded");
    assert_eq!(e.retry_after(), None);
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
    assert_eq!(e.retry_after(), Some(0));
    assert_eq!(calls(&s).await.len(), 3);
}

/// A local address nothing listens on.
fn closed_port_url() -> String {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).expect("a free port");
    let port = listener.local_addr().unwrap().port();
    drop(listener);
    format!("http://{}:{port}/api/v1", Ipv4Addr::LOCALHOST)
}

#[tokio::test]
async fn a_network_failure_becomes_connection_error_after_the_retries() {
    let client = Client::builder().api_key(KEY).base_url(closed_port_url()).max_retries(1).build().unwrap();
    let e: Error = client.blocks().latest().await.expect_err("nothing listens");
    assert_eq!(e.code(), "connection_error");
    assert_eq!(e.status(), 0);
    assert!(std::error::Error::source(&e).is_some(), "the cause is attached");
}

#[tokio::test]
async fn a_slow_answer_becomes_timeout() {
    let s = server(vec![ok(&fixture("getLatestBlock")).set_delay(Duration::from_secs(5))]).await;
    let client =
        Client::builder().api_key(KEY).base_url(format!("{}/api/v1", s.uri())).timeout(Duration::from_millis(200)).build().unwrap();
    let e = client.blocks().latest().await.expect_err("too slow");
    assert_eq!(e.code(), "timeout");
    assert_eq!(e.status(), 0);
    assert_eq!(calls(&s).await.len(), 1, "a timeout is not retried");
}

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
    let params = AddressesTransactionsParams::new().limit(2).direction("in");
    let mut pager = client.addresses().transactions_pages(USDC, &params);
    let mut pages = Vec::new();
    while let Some(p) = pager.next_page().await.expect("a page") {
        pages.push(p);
    }
    assert_eq!(pages.len(), 2);
    assert_eq!(pages[0].next_cursor(), Some("c1"));
    assert_eq!(pages[1].next_cursor(), None);
    assert!(pager.next_page().await.unwrap().is_none(), "a finished pager stays finished");

    let reqs = calls(&s).await;
    assert_eq!(reqs.len(), 2);
    let q = |i: usize| reqs[i].url.query_pairs().map(|(k, v)| (k.into_owned(), v.into_owned())).collect::<Vec<_>>();
    assert_eq!(q(0), vec![("limit".into(), "2".into()), ("direction".into(), "in".into())]);
    let second = q(1);
    assert_eq!(second.len(), 3);
    for pair in [("cursor", "c1"), ("limit", "2"), ("direction", "in")] {
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
async fn chain_id_comes_from_the_client() {
    let latest = fixture("getLatestBlock");
    let s = server(vec![ok(&latest), ok(&latest)]).await;
    client_for(&s, 0).blocks().latest().await.unwrap();
    let other = Client::builder().api_key(KEY).base_url(format!("{}/api/v1/", s.uri())).chain_id(17000).build().unwrap();
    assert_eq!(other.chain_id(), 17000);
    other.blocks().latest().await.unwrap();
    let reqs = calls(&s).await;
    assert_eq!(reqs[0].url.path(), "/api/v1/chains/1/blocks/latest");
    assert_eq!(reqs[1].url.path(), "/api/v1/chains/17000/blocks/latest");
}

#[tokio::test]
async fn path_values_are_percent_encoded_and_integers_accept_strings() {
    let latest = fixture("getLatestBlock");
    let s = server(vec![ok(&latest), ok(&latest)]).await;
    let client = client_for(&s, 0);
    client.blocks().get(25_920_000).await.unwrap();
    client.blocks().get("25920000").await.unwrap();
    let reqs = calls(&s).await;
    assert_eq!(reqs[0].url.path(), reqs[1].url.path());
    assert_eq!(reqs[0].url.path(), "/api/v1/chains/1/blocks/25920000");
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
fn an_api_key_is_required() {
    assert_eq!(Client::new("").unwrap_err(), ConfigError::MissingApiKey);
    assert_eq!(Client::builder().build().unwrap_err(), ConfigError::MissingApiKey);
    assert_eq!(Client::new("bad\nkey").unwrap_err(), ConfigError::InvalidApiKey);
    assert_eq!(Client::builder().api_key("k").base_url("not a url").build().unwrap_err(), ConfigError::InvalidBaseUrl);
}

#[test]
fn defaults() {
    let client = Client::new("k").unwrap();
    assert_eq!(client.base_url(), "https://tracefast.xyz/api/v1");
    assert_eq!(client.chain_id(), 1);
    let debug = format!("{client:?}");
    assert!(debug.contains("max_retries: 2") && debug.contains("timeout: 30s"), "{debug}");
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

#[tokio::test]
async fn a_redirect_is_not_followed_it_is_an_unexpected_response() {
    let target = server(vec![ok(&fixture("getLatestBlock"))]).await;
    let redirect = ResponseTemplate::new(302).insert_header("location", format!("{}/api/v1/chains/1/blocks/latest", target.uri()));
    let s = server(vec![redirect]).await;
    let e = client_for(&s, 2).blocks().latest().await.expect_err("a redirect");
    assert_eq!(e.code(), "unexpected_response");
    assert_eq!(e.status(), 302);
    assert_eq!(calls(&s).await.len(), 1);
    assert!(calls(&target).await.is_empty(), "the key never goes to the redirect's target");
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
