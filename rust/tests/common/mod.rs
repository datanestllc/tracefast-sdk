//! Shared by the test files: fixtures, samples, a mock server per call, and the
//! strict comparison of a fixture with what the generated types make of it.
#![allow(dead_code)]

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::str::FromStr;

use serde::Serialize;
use serde_json::Value;
use tracefast::Client;
use wiremock::matchers::any;
use wiremock::{Mock, MockServer, ResponseTemplate};

pub const KEY: &str = "tf_test_key";

fn repo() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..")
}

fn read_json(rel: &str) -> Value {
    let path = repo().join(rel);
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    serde_json::from_str(&text).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

/// The recorded answer of an operation.
pub fn fixture(operation_id: &str) -> Value {
    read_json(&format!("fixtures/{operation_id}.json"))
}

pub fn samples() -> Value {
    read_json("fixtures/samples.json")
}

/// A sample string parsed into the parameter's type.
pub fn parse<T: FromStr>(s: &str) -> T
where
    T::Err: std::fmt::Debug,
{
    s.parse().unwrap_or_else(|e| panic!("sample {s:?}: {e:?}"))
}

/// A client for a mock server.
pub fn client_for(server: &MockServer, max_retries: u32) -> Client {
    Client::builder().api_key(KEY).base_url(format!("{}/api/v1", server.uri())).max_retries(max_retries).build().expect("a client")
}

/// `encodeURIComponent`, written again here so the test does not trust the SDK's own.
pub fn encode_uri_component(s: &str) -> String {
    const KEEP: &str = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_.!~*'()";
    s.bytes().map(|b| if KEEP.as_bytes().contains(&b) { (b as char).to_string() } else { format!("%{b:02X}") }).collect()
}

/// One operation's test: its fixture served by a mock server, its sample arguments.
pub struct Case {
    pub operation_id: String,
    pub server: MockServer,
    pub body: Value,
    path_values: BTreeMap<String, String>,
    query: BTreeMap<String, String>,
}

impl Case {
    /// `query_names`: the operation's query parameters.
    pub async fn start(operation_id: &str, namespace: Option<&str>, query_names: &[&str]) -> Self {
        let samples = samples();
        let own = &samples["operations"][operation_id];
        let mut path_values = BTreeMap::new();
        let mut layers = vec![&samples["defaults"]];
        if let Some(ns) = namespace {
            layers.push(&samples["byNamespace"][ns]);
        }
        layers.push(&own["path"]);
        for layer in layers {
            if let Some(m) = layer.as_object() {
                for (k, v) in m {
                    path_values.insert(k.clone(), v.as_str().expect("string sample").to_string());
                }
            }
        }
        let mut query = BTreeMap::new();
        if query_names.contains(&"limit") {
            query.insert("limit".to_string(), samples["limit"].as_str().expect("samples.limit").to_string());
        }
        if let Some(m) = own["query"].as_object() {
            for (k, v) in m {
                assert!(query_names.contains(&k.as_str()), "{operation_id}: sample query {k} is not a parameter");
                query.insert(k.clone(), v.as_str().expect("string sample").to_string());
            }
        }
        let body = fixture(operation_id);
        let server = MockServer::start().await;
        Mock::given(any()).respond_with(ResponseTemplate::new(200).set_body_json(&body)).mount(&server).await;
        Self { operation_id: operation_id.to_string(), server, body, path_values, query }
    }

    pub fn client(&self) -> Client {
        client_for(&self.server, 0)
    }

    pub fn path(&self, name: &str) -> String {
        self.path_values.get(name).unwrap_or_else(|| panic!("{}: no sample for path parameter {name}", self.operation_id)).clone()
    }

    pub fn query(&self, name: &str) -> Option<String> {
        self.query.get(name).cloned()
    }

    /// How many rows the fixture holds in `data.<field>`.
    pub fn rows(&self, field: &str) -> usize {
        self.body["data"][field].as_array().map_or(0, Vec::len)
    }

    /// The request was the right one, and the typed response is the fixture —
    /// every field known, nothing dropped, `null` and absent kept apart.
    pub async fn check<T: Serialize>(&self, path_template: &str, response: &T) {
        let requests = self.server.received_requests().await.expect("recording");
        assert_eq!(requests.len(), 1, "{}: one request", self.operation_id);
        let req = &requests[0];
        let mut expected = String::from("/api/v1");
        let mut rest = path_template;
        while let Some(open) = rest.find('{') {
            let close = rest.find('}').expect("closing brace");
            expected.push_str(&rest[..open]);
            let name = &rest[open + 1..close];
            expected.push_str(&if name == "chainId" { "1".to_string() } else { encode_uri_component(&self.path(name)) });
            rest = &rest[close + 1..];
        }
        expected.push_str(rest);
        assert_eq!(req.url.path(), expected, "{}: path", self.operation_id);
        let sent: Vec<(String, String)> = req.url.query_pairs().map(|(k, v)| (k.into_owned(), v.into_owned())).collect();
        let sent_map: BTreeMap<String, String> = sent.iter().cloned().collect();
        assert_eq!(sent.len(), sent_map.len(), "{}: no parameter twice", self.operation_id);
        assert_eq!(sent_map, self.query, "{}: only the given parameters are sent", self.operation_id);
        assert_eq!(req.method.as_str(), "GET");
        assert_eq!(header(req, "x-api-key").as_deref(), Some(KEY));
        assert_eq!(header(req, "accept").as_deref(), Some("application/json"));
        let agent = header(req, "user-agent").expect("user-agent");
        assert!(agent.starts_with(&format!("tracefast-rust/{}", tracefast::VERSION)), "{agent}");

        let problems = round_trip(&self.body, response);
        assert!(problems.is_empty(), "{}: the fixture does not match the generated types:\n{}", self.operation_id, problems.join("\n"));
    }
}

pub fn header(req: &wiremock::Request, name: &str) -> Option<String> {
    req.headers.get(name).and_then(|v| v.to_str().ok()).map(str::to_string)
}

/// Differences between a fixture and the re-serialized typed response. A field
/// the types do not know is dropped, a `null` in a field that does not admit one
/// is dropped, a required-nullable field that was absent comes back `null`.
pub fn round_trip<T: Serialize>(input: &Value, typed: &T) -> Vec<String> {
    let output = serde_json::to_value(typed).expect("serializable");
    let mut out = Vec::new();
    diff(input, &output, "$", &mut out);
    out
}

fn diff(a: &Value, b: &Value, path: &str, out: &mut Vec<String>) {
    match (a, b) {
        (Value::Object(x), Value::Object(y)) => {
            for (k, v) in x {
                match y.get(k) {
                    Some(w) => diff(v, w, &format!("{path}.{k}"), out),
                    None if v.is_null() => out.push(format!("{path}.{k}: null in a field that does not admit null")),
                    None => out.push(format!("{path}.{k}: not in the generated type")),
                }
            }
            for k in y.keys().filter(|k| !x.contains_key(*k)) {
                out.push(format!("{path}.{k}: absent in the fixture, {} after the round trip", y[k]));
            }
        }
        (Value::Array(x), Value::Array(y)) => {
            if x.len() != y.len() {
                out.push(format!("{path}: {} items became {}", x.len(), y.len()));
            }
            for (i, (v, w)) in x.iter().zip(y).enumerate() {
                diff(v, w, &format!("{path}[{i}]"), out);
            }
        }
        (Value::Number(x), Value::Number(y)) => {
            let same = match (x.as_i64(), y.as_i64(), x.as_u64(), y.as_u64()) {
                (Some(p), Some(q), _, _) => p == q,
                (_, _, Some(p), Some(q)) => p == q,
                _ => x.as_f64() == y.as_f64(),
            };
            if !same {
                out.push(format!("{path}: {x} became {y}"));
            }
        }
        _ if a == b => {}
        _ => out.push(format!("{path}: {a} became {b}")),
    }
}
