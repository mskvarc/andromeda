//! The `instance` of a problem detail names the request as the client sent it.
//!
//! ETSI GS CIM 009 v1.9.1 clause 5.5.3 makes every error body an IETF RFC 7807
//! section 3.1 object, whose `instance` is "a URI reference that identifies the
//! specific occurrence of the problem". Every refusal raised while a request's
//! parameters, headers or body were still being read named the URI the nested
//! API router had stripped of its `/ngsi-ld/v1` mount: `/?type=GtfsStop&q=bad((`
//! for a query, `/query` for `POST /entityOperations/query`, `/` for `GET
//! /types` on an unknown tenant. Resolved against the broker, each names a
//! resource that does not exist, while a refusal raised later in the same
//! operation named the full path.
//!
//! Naming convention: `regression_<finding>_<scenario>`.

#![allow(clippy::expect_used, clippy::unwrap_used, reason = "integration test setup uses explicit assertions")]

use andromeda_test_client::{response::TestResponse, tenant_name::TenantName, test_client::TestClient};
use andromeda_test_harness::{configuration::TestRunConfiguration, environment::TestEnvironment};
use reqwest::Method;
use serde_json::Value;
use url::Position;

/// The request target `path` resolves to under the client's endpoint: the
/// path and query the client puts on the wire, which the `instance` of a
/// refusal must repeat.
fn addressed_target(client: &TestClient, path: &str) -> String {
    let url = client.endpoint().join(path).expect("a path under the endpoint");
    url[Position::BeforePath..].to_owned()
}

/// Require `response` to refuse the request with a problem detail, and report
/// the `instance` it named when that is not `expected`.
///
/// The mismatch is returned rather than asserted so that a regression names
/// every request it reaches, not only the first.
fn misnamed_instance(response: &TestResponse, expected: &str, request: &str) -> Option<String> {
    assert!(
        response.status.is_client_error(),
        "{request} was not refused: {} {:?}",
        response.status,
        response.body
    );
    let problem: Value = response.json_value().expect("a refusal carries a problem details body");
    (problem["instance"] != expected).then(|| format!("{request} named {}, expected {expected:?}", problem["instance"]))
}

/// One request the broker must refuse while it is still reading it.
struct RefusedRequest {
    /// The request method.
    method: Method,
    /// The request target, relative to the NGSI-LD endpoint.
    path: &'static str,
    /// The one header the request adds, if any.
    header: Option<(&'static str, &'static str)>,
    /// The payload body, empty for a request that sends none.
    body: &'static [u8],
}

/// Each refusal below is raised while the request is still being read -- its
/// query string, a path segment, a header, its payload body -- and each names
/// the full request target, query string included.
#[tokio::test]
async fn regression_problem_instance_request_reading_refusals_name_the_addressed_uri() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();

    let json = Some(("content-type", "application/json"));
    let requests = [
        RefusedRequest {
            method: Method::GET,
            path: "entities?type=Building&q=bad((",
            header: None,
            body: b"",
        },
        RefusedRequest {
            method: Method::GET,
            path: "entities/bare-name",
            header: None,
            body: b"",
        },
        RefusedRequest {
            method: Method::GET,
            path: "entities?type=Building",
            header: Some(("link", "this is not a link header")),
            body: b"",
        },
        RefusedRequest {
            method: Method::GET,
            path: "entities?type=Building&unknownParameter=1",
            header: None,
            body: b"",
        },
        RefusedRequest {
            method: Method::POST,
            path: "entityOperations/query",
            header: json,
            body: b"{",
        },
        RefusedRequest {
            method: Method::POST,
            path: "entityOperations/upsert?unknownParameter=1",
            header: json,
            body: b"[]",
        },
        RefusedRequest {
            method: Method::DELETE,
            path: "snapshots",
            header: None,
            body: b"",
        },
    ];

    let mut misnamed = Vec::new();
    for RefusedRequest { method, path, header, body } in requests {
        let request = format!("{method} {path}");
        let mut raw = client.raw(method, path);
        if let Some((name, value)) = header {
            raw = raw.header(name, value);
        }
        if !body.is_empty() {
            raw = raw.body(body.to_vec());
        }
        let response = raw.send().await.expect("the broker answers");

        misnamed.extend(misnamed_instance(&response, &addressed_target(client, path), &request));
    }
    assert!(misnamed.is_empty(), "refusals not naming the addressed URI:\n{}", misnamed.join("\n"));

    scope.finish().await.expect("cleanup failed");
}

/// Clause 5.5.10: an operation "that target[s] a non-existing Tenant should
/// raise an error of type `NonexistentTenant`". The refusal is raised while the
/// tenant header is being resolved, before the operation runs, and named `/`
/// for `GET /types`.
#[tokio::test]
async fn regression_problem_instance_an_unknown_tenant_names_the_addressed_uri() {
    let configuration = TestRunConfiguration::from_environment().expect("test run configuration");
    let tenant = format!("problem-instance-absent-{}", uuid::Uuid::now_v7().simple());
    let client = TestClient::new(
        configuration.broker().clone(),
        Some(TenantName::parse(&tenant).expect("a valid tenant header value")),
    );

    let mut misnamed = Vec::new();
    for path in ["types", "attributes?details=true", "entities?type=Building"] {
        let response = client.raw(Method::GET, path).send().await.expect("the broker answers");

        misnamed.extend(misnamed_instance(
            &response,
            &addressed_target(&client, path),
            &format!("GET {path} on {tenant}"),
        ));
    }
    assert!(misnamed.is_empty(), "refusals not naming the addressed URI:\n{}", misnamed.join("\n"));
}
