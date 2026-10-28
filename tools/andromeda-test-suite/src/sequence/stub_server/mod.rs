//! The test's own stub server: it answers what the brokers ask of it (notifications, forwarded requests, `@context`
//! documents) with the stubs the test sets, and records every request for the test to query.
//!
//! Stubs, patterns and journal entries have the JSON shapes of the WireMock admin API, so a test states them as data.
//! The server runs in the test process: it records only the test's own requests, and a stub set by one test never
//! answers another's.

pub mod mapping;
pub mod pattern;
pub mod response_template;

use crate::{
    callback_server::{CallbackError, CallbackServer},
    sequence::stub_server::{
        mapping::{STARTED, StubError, StubMapping},
        pattern::{PatternError, RequestPattern},
    },
};
use serde::Deserialize;
use serde_json::{Map, Value as Json, json};
use serde_json_path::JsonPath;
use std::{
    collections::HashMap,
    sync::{Arc, PoisonError, RwLock},
};
use uuid::Uuid;
use wiremock::{Mock, Request, Respond, ResponseTemplate, matchers::any};

/// The stubs every server starts with: a notification or a relayed MQTT message is accepted with 200 unless a stub of
/// the test, at a lower priority value, answers otherwise.
const DEFAULT_STUBS: [&str; 2] = [
    r#"{"priority": 10, "request": {"method": "POST", "urlPathPattern": "/notify/.*"}, "response": {"status": 200}}"#,
    r#"{"priority": 10, "request": {"method": "POST", "urlPathPattern": "/mqtt/.*"}, "response": {"status": 200}}"#,
];

/// Why the stub server cannot do what the test asks.
#[derive(Debug, thiserror::Error)]
pub enum StubServerError {
    /// The server cannot start.
    #[error(transparent)]
    Callback(#[from] CallbackError),
    /// A stub or pattern is not the JSON of one.
    #[error("{0} is not a stub mapping or request pattern: {1}")]
    Shape(String, String),
    /// A stub cannot answer.
    #[error(transparent)]
    Stub(#[from] StubError),
    /// A pattern is not valid.
    #[error(transparent)]
    Pattern(#[from] PatternError),
}

/// One stub as the server keeps it.
#[derive(Debug, Clone)]
struct Stub {
    /// The identifier the server gave it.
    id: Uuid,
    /// The mapping.
    mapping: StubMapping,
}

/// The stubs and scenario states, shared with the responder.
#[derive(Debug, Default)]
struct Stubs {
    /// Every stub, in the order set.
    stubs: Vec<Stub>,
    /// The state of each scenario a stub moved.
    scenarios: HashMap<String, String>,
}

/// Answers a request with the best matching stub, or 404 when none matches.
#[derive(Debug, Clone)]
struct Responder(Arc<RwLock<Stubs>>);

impl Respond for Responder {
    fn respond(&self, request: &Request) -> ResponseTemplate {
        let mut state = self.0.write().unwrap_or_else(PoisonError::into_inner);
        let chosen = state
            .stubs
            .iter()
            .enumerate()
            .filter(|(_, stub)| {
                let scenario = stub
                    .mapping
                    .scenario_name
                    .as_ref()
                    .map(|name| state.scenarios.get(name).map_or(STARTED, String::as_str));
                let in_state = match (&stub.mapping.required_scenario_state, scenario) {
                    (Some(required), Some(current)) => required == current,
                    (Some(_), None) | (None, _) => true,
                };
                in_state && stub.mapping.request.matches(request).unwrap_or(false)
            })
            // The stub set last wins among equal priorities.
            .min_by_key(|(index, stub)| (stub.mapping.rank(), usize::MAX - index))
            .map(|(_, stub)| stub.clone());
        match chosen {
            Some(stub) => {
                if let (Some(name), Some(next)) = (&stub.mapping.scenario_name, &stub.mapping.new_scenario_state) {
                    state.scenarios.insert(name.clone(), next.clone());
                }
                stub.mapping
                    .response
                    .answer()
                    .unwrap_or_else(|error| ResponseTemplate::new(500).set_body_string(error.to_string()))
            }
            None => ResponseTemplate::new(404).set_body_string("Request was not matched"),
        }
    }
}

/// A removal of the stubs whose metadata a JSON path selects with a value.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct MetadataPattern {
    /// The selection.
    matches_json_path: MetadataSelection,
}

/// A JSON path and the value it must select.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct MetadataSelection {
    /// The JSON path.
    expression: String,
    /// The value.
    equal_to: String,
}

/// The stub server of one test.
#[derive(Debug)]
pub struct StubServer {
    /// The server the brokers call.
    server: CallbackServer,
    /// The stubs it answers with.
    stubs: Arc<RwLock<Stubs>>,
}

impl StubServer {
    /// Starts a server with no stub: every request is answered 404 and recorded.
    ///
    /// # Errors
    ///
    /// Returns [`StubServerError`] when the server cannot listen.
    pub async fn start() -> Result<StubServer, StubServerError> {
        let server = CallbackServer::start().await?;
        let stubs = Arc::new(RwLock::new(Stubs::default()));
        Mock::given(any()).respond_with(Responder(Arc::clone(&stubs))).mount(server.server()?).await;
        let started = StubServer { server, stubs };
        for default in DEFAULT_STUBS {
            started.set(default)?;
        }
        Ok(started)
    }

    /// The base URL the brokers reach the server by, without its final `/`.
    ///
    /// # Errors
    ///
    /// Returns [`StubServerError`] when the URL cannot be built.
    pub fn base(&self) -> Result<String, StubServerError> {
        Ok(self.server.url("/")?.as_str().trim_end_matches('/').to_owned())
    }

    /// The base URL the test process reaches the server by, without its final `/`.
    ///
    /// # Errors
    ///
    /// Returns [`StubServerError`] when the server does not serve.
    pub fn local(&self) -> Result<String, StubServerError> {
        Ok(self.server.server()?.uri())
    }

    /// Sets the stub `mapping`, the JSON of a stub mapping, and answers as WireMock does: 201 and the stub with its
    /// identifier.
    ///
    /// # Errors
    ///
    /// Returns [`StubServerError`] when `mapping` is not a stub the server can answer with.
    pub fn set(&self, mapping: &str) -> Result<(u16, String), StubServerError> {
        let parsed = serde_json::from_str::<StubMapping>(mapping).map_err(|error| StubServerError::Shape(mapping.to_owned(), error.to_string()))?;
        parsed.validate()?;
        let id = Uuid::new_v4();
        let mut answer = serde_json::from_str::<Map<String, Json>>(mapping).map_err(|error| StubServerError::Shape(mapping.to_owned(), error.to_string()))?;
        answer.insert("id".to_owned(), Json::String(id.to_string()));
        answer.insert("uuid".to_owned(), Json::String(id.to_string()));
        self.stubs
            .write()
            .unwrap_or_else(PoisonError::into_inner)
            .stubs
            .push(Stub { id, mapping: parsed });
        Ok((201, Json::Object(answer).to_string()))
    }

    /// Removes the stub with identifier `id`; 200 when it existed, 404 otherwise.
    #[must_use]
    pub fn remove(&self, id: &str) -> u16 {
        let mut state = self.stubs.write().unwrap_or_else(PoisonError::into_inner);
        let before = state.stubs.len();
        state.stubs.retain(|stub| stub.id.to_string() != id);
        if state.stubs.len() < before { 200 } else { 404 }
    }

    /// Removes every stub whose metadata matches `pattern`, the JSON of a metadata pattern; 200.
    ///
    /// # Errors
    ///
    /// Returns [`StubServerError`] when `pattern` is not a metadata pattern.
    pub fn remove_by_metadata(&self, pattern: &str) -> Result<u16, StubServerError> {
        let parsed = serde_json::from_str::<MetadataPattern>(pattern).map_err(|error| StubServerError::Shape(pattern.to_owned(), error.to_string()))?;
        let selection = parsed.matches_json_path;
        let path = JsonPath::parse(&selection.expression)
            .map_err(|error| StubServerError::Pattern(PatternError::JsonPath(selection.expression.clone(), error.to_string())))?;
        let expected = Json::String(selection.equal_to);
        self.stubs
            .write()
            .unwrap_or_else(PoisonError::into_inner)
            .stubs
            .retain(|stub| !path.query(&Json::Object(stub.mapping.metadata.clone())).all().contains(&&expected));
        Ok(200)
    }

    /// The requests received that match `pattern`, the JSON of a request pattern, oldest first, as the JSON of
    /// the WireMock `requests/find` answer.
    ///
    /// # Errors
    ///
    /// Returns [`StubServerError`] when `pattern` is not a request pattern.
    pub async fn find(&self, pattern: &str) -> Result<String, StubServerError> {
        let requests = self.matching(pattern).await?;
        let entries = requests.iter().map(|request| self.entry(request)).collect::<Vec<_>>();
        Ok(json!({ "requests": entries }).to_string())
    }

    /// The number of requests received that match `pattern`, as the JSON of the WireMock `requests/count` answer.
    ///
    /// # Errors
    ///
    /// Returns [`StubServerError`] when `pattern` is not a request pattern.
    pub async fn count(&self, pattern: &str) -> Result<String, StubServerError> {
        let requests = self.matching(pattern).await?;
        Ok(json!({ "count": requests.len(), "requestJournalDisabled": false }).to_string())
    }

    /// The requests received that match `pattern`, oldest first.
    async fn matching(&self, pattern: &str) -> Result<Vec<Request>, StubServerError> {
        let parsed = serde_json::from_str::<RequestPattern>(pattern).map_err(|error| StubServerError::Shape(pattern.to_owned(), error.to_string()))?;
        let mut matching = Vec::new();
        for request in self.server.received().await? {
            if parsed.matches(&request)? {
                matching.push(request);
            }
        }
        Ok(matching)
    }

    /// The journal entry of `request`: its URL, method, headers (one value as a string, several as a list), body and
    /// query parameters.
    fn entry(&self, request: &Request) -> Json {
        let mut headers = Map::new();
        for name in request.headers.keys() {
            let values = request
                .headers
                .get_all(name)
                .iter()
                .map(|value| Json::String(String::from_utf8_lossy(value.as_bytes()).into_owned()))
                .collect::<Vec<_>>();
            let header = match <[Json; 1]>::try_from(values) {
                Ok([single]) => single,
                Err(several) => Json::Array(several),
            };
            headers.insert(title_case(name.as_str()), header);
        }
        let mut parameters = Map::<String, Json>::new();
        for (key, value) in request.url.query_pairs() {
            let entry = parameters
                .entry(key.clone().into_owned())
                .or_insert_with(|| json!({ "key": key.as_ref(), "values": [] }));
            if let Some(Json::Array(values)) = entry.get_mut("values") {
                values.push(Json::String(value.into_owned()));
            }
        }
        let url = match request.url.query() {
            Some(query) => format!("{}?{query}", request.url.path()),
            None => request.url.path().to_owned(),
        };
        let absolute = self.base().map_or_else(|_| url.clone(), |base| format!("{base}{url}"));
        json!({
            "url": url,
            "absoluteUrl": absolute,
            "method": request.method.as_str(),
            "headers": headers,
            "body": String::from_utf8_lossy(&request.body),
            "queryParams": parameters,
        })
    }
}

/// A header name with each dash-separated word capitalised, as the WireMock journal writes the names HTTP/1.1
/// clients send (`content-type` becomes `Content-Type`).
fn title_case(name: &str) -> String {
    name.split('-')
        .map(|word| {
            let mut letters = word.chars();
            letters.next().map_or_else(String::new, |first| first.to_uppercase().chain(letters).collect())
        })
        .collect::<Vec<_>>()
        .join("-")
}
