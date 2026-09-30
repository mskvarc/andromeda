//! The clause 6.29 and 6.30 `/jsonldContexts` resources.

use crate::{
    broker_endpoints::broker_url,
    context_origin::ContextOrigin,
    ngsi_requests::{create_entity, create_subscription},
    polling::eventually,
    unique_ids::unique_id,
};
use reqwest::{Client, Method, Response, StatusCode, header::HeaderMap};
use serde_json::{Value, json};

/// The path segment every stored `@context` is served under (clause 6.30).
pub const CONTEXTS_PATH: &str = "/ngsi-ld/v1/jsonldContexts/";

/// The prefix of every identifier the broker mints for a stored `@context`.
pub const JSON_LD_CONTEXT_PREFIX: &str = "urn:ngsi-ld:JsonLdContext:";

/// Add @context (clause 5.13.2): the status, the `Location` header and the body.
pub async fn create_context(client: &Client, body: &Value) -> (StatusCode, Option<String>, Value) {
    let response = client
        .post(broker_url("jsonldContexts"))
        .header("Content-Type", "application/json")
        .json(body)
        .send()
        .await
        .expect("create_context request failed");
    let status = response.status();
    let location = response
        .headers()
        .get("location")
        .map(|value| value.to_str().expect("non-ascii location").to_owned());
    let body: Value = response.json().await.unwrap_or_default();
    (status, location, body)
}

/// Add a Hosted @context holding `body` and return its identifier.
pub async fn add_hosted(client: &Client, body: &Value) -> String {
    let (status, location, problem) = create_context(client, body).await;
    assert_eq!(status, StatusCode::CREATED, "the context is added: {problem}");
    extract_id(location.as_deref().expect("a Location header"))
}

/// Add a Hosted @context with three terms and return its identifier.
pub async fn create_hosted_context(client: &Client) -> String {
    add_hosted(
        client,
        &json!({
            "@context": {
                "A1": "urn:ngsi-ld:attributes:A1",
                "A2": "urn:ngsi-ld:attributes:A2",
                "A3": "urn:ngsi-ld:attributes:A3"
            }
        }),
    )
    .await
}

/// Serve @context (clause 5.13.4): `GET /jsonldContexts/{id}` with `query`.
pub async fn get_context(client: &Client, id: &str, query: &str) -> (StatusCode, Value) {
    let separator = if query.is_empty() { "" } else { "?" };
    let response = client
        .get(broker_url(&format!("jsonldContexts/{}{separator}{query}", urlencoding::encode(id))))
        .send()
        .await
        .expect("get_context request failed");
    let status = response.status();
    let body: Value = response.json().await.unwrap_or_default();
    (status, body)
}

/// One Serve @context exchange with every part a test may inspect.
pub struct Served {
    /// The response status.
    pub status: StatusCode,
    /// The response headers.
    pub headers: HeaderMap,
    /// The raw body bytes.
    pub body: Vec<u8>,
}

impl Served {
    /// One header's value, as text.
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers.get(name).and_then(|value| value.to_str().ok())
    }

    /// The body as JSON.
    pub fn json(&self) -> Value {
        serde_json::from_slice(&self.body).expect("a JSON body")
    }

    /// The `max-age` the `Cache-Control` header states, in seconds.
    pub fn max_age(&self) -> Option<u64> {
        self.header("cache-control")?
            .split(',')
            .find_map(|directive| directive.trim().strip_prefix("max-age="))
            .and_then(|seconds| seconds.parse().ok())
    }
}

/// Serve `id` with `method`, adding the request headers `extra`.
pub async fn serve(client: &Client, method: Method, id: &str, extra: &[(&str, &str)]) -> Served {
    served(send_serve(client, method, &broker_url(&format!("jsonldContexts/{}", urlencoding::encode(id))), extra).await).await
}

/// Serve the clause 5.13.3.5 metadata of `id` (`details=true`).
pub async fn serve_details(client: &Client, id: &str) -> Served {
    served(
        send_serve(
            client,
            Method::GET,
            &broker_url(&format!("jsonldContexts/{}?details=true", urlencoding::encode(id))),
            &[],
        )
        .await,
    )
    .await
}

/// Dereference an absolute context URL the broker minted, with `client`.
pub async fn dereference(client: &Client, url: &str) -> Served {
    served(send_serve(client, Method::GET, url, &[]).await).await
}

async fn send_serve(client: &Client, method: Method, url: &str, extra: &[(&str, &str)]) -> Response {
    let mut request = client.request(method, url);
    for (name, value) in extra {
        request = request.header(*name, *value);
    }
    request.send().await.expect("serve request failed")
}

async fn served(response: Response) -> Served {
    let status = response.status();
    let headers = response.headers().clone(); // clone: the headers outlive the consumed response
    let body = response.bytes().await.expect("a readable body").to_vec();
    Served { status, headers, body }
}

/// Read `numberOfHits` for a context right now (clause 5.13.3.5). The served
/// metadata folds this process's unflushed accesses into the stored row
/// before answering, so it is current the instant a request that used the
/// context returns.
pub async fn number_of_hits(client: &Client, id: &str) -> u64 {
    let (status, body) = get_context(client, id, "details=true").await;
    assert_eq!(status, StatusCode::OK, "metadata read failed: {body:?}");
    body["numberOfHits"].as_u64().unwrap_or(0)
}

/// List @contexts (clause 5.13.3) with `query`.
pub async fn list_contexts(client: &Client, query: &str) -> (StatusCode, Vec<Value>) {
    let separator = if query.is_empty() { "" } else { "?" };
    let response = client
        .get(broker_url(&format!("jsonldContexts{separator}{query}")))
        .send()
        .await
        .expect("list_contexts request failed");
    let status = response.status();
    let body: Vec<Value> = response.json().await.unwrap_or_default();
    (status, body)
}

/// Delete @context (clause 5.13.5) with `query`.
pub async fn delete_context(client: &Client, id: &str, query: &str) -> (StatusCode, Value) {
    let (status, bytes) = delete_context_raw(client, id, query).await;
    (status, serde_json::from_slice(&bytes).unwrap_or_default())
}

/// Delete @context, returning the raw body for the empty-body checks.
pub async fn delete_context_raw(client: &Client, id: &str, query: &str) -> (StatusCode, Vec<u8>) {
    let separator = if query.is_empty() { "" } else { "?" };
    let response = client
        .delete(broker_url(&format!("jsonldContexts/{}{separator}{query}", urlencoding::encode(id))))
        .send()
        .await
        .expect("delete_context request failed");
    let status = response.status();
    let bytes = response.bytes().await.unwrap_or_default().to_vec();
    (status, bytes)
}

/// The last path segment of a `Location` header or a URL, query dropped.
pub fn extract_id(uri: &str) -> String {
    let path = uri.split('?').next().unwrap_or(uri);
    path.rsplit('/').next().expect("a last path segment").to_owned()
}

/// The identifiers of the broker-minted `ImplicitlyCreated` entries the tenant
/// lists; the embedded core `@context`, listed under its URL, is left out.
pub async fn implicit_context_ids(client: &Client) -> Vec<String> {
    let (status, contexts) = list_contexts(client, "kind=ImplicitlyCreated").await;
    assert_eq!(status, StatusCode::OK);
    contexts
        .iter()
        .filter_map(Value::as_str)
        .map(extract_id)
        .filter(|identifier| identifier.starts_with(JSON_LD_CONTEXT_PREFIX))
        .collect()
}

/// The URLs of the Cached entries the tenant lists.
pub async fn cached_urls(client: &Client) -> Vec<String> {
    let (status, cached) = list_contexts(client, "kind=Cached&details=true").await;
    assert_eq!(status, StatusCode::OK);
    cached.iter().filter_map(|entry| entry["URL"].as_str()).map(str::to_owned).collect()
}

/// A subscription whose `@context` is an array of two inline definitions, so
/// the broker hosts it as an `ImplicitlyCreated` wrapper to name it by one URL
/// (clause 5.8.1.4). Returns the subscription's and the wrapper's identifiers.
pub async fn create_implicit_context(client: &Client) -> (String, String) {
    let subscription = json!({
        "type": "Subscription",
        "entities": [{ "type": "T1" }],
        "notification": { "endpoint": { "uri": "http://127.0.0.1:1/notify" } },
        "@context": [{ "T1": "https://example.org/implicit/T1" }, { "speed": "https://example.org/implicit/speed" }]
    });
    let (status, location) = create_subscription(client, &subscription).await;
    assert_eq!(status, StatusCode::CREATED);
    let subscription_id = extract_id(location.as_deref().expect("a Location header"));
    let wrappers = implicit_context_ids(client).await;
    assert_eq!(wrappers.len(), 1, "one wrapper hosted for the subscription: {wrappers:?}");
    (subscription_id, wrappers[0].clone()) // clone: the list is dropped after this
}

/// Serve a one-term context from a fresh origin, create an entity naming it so
/// the broker downloads and records it as Cached, and wait until the record is
/// listed. Returns the origin and the context URL.
pub async fn setup_cached_context(client: &Client) -> (ContextOrigin, String) {
    let origin = ContextOrigin::start().await;
    let context_url = origin
        .serve_document("/context.jsonld", &json!({ "@context": { "X1": "urn:ngsi-ld:attributes:X1" } }))
        .await;
    let entity = json!({
        "id": unique_id("urn:ngsi-ld:Testing:cached"),
        "type": "Testing",
        "X1": { "type": "Property", "value": "test" },
        "@context": [context_url]
    });
    let (status, _) = create_entity(client, &entity).await;
    assert_eq!(status, StatusCode::CREATED);
    let recorded = &context_url;
    eventually("the downloaded context is recorded as Cached", move || async move {
        cached_urls(client).await.contains(recorded).then_some(())
    })
    .await;
    (origin, context_url)
}

/// The local identifier of the Cached entry downloaded from `context_url`.
pub async fn find_cached_context_id(client: &Client, context_url: &str) -> String {
    let (status, contexts) = list_contexts(client, "kind=Cached&details=true").await;
    assert_eq!(status, StatusCode::OK);
    let cached = contexts
        .iter()
        .find(|entry| entry.get("URL").and_then(Value::as_str) == Some(context_url))
        .expect("the context is cached");
    cached["localId"].as_str().expect("a localId").to_owned()
}
