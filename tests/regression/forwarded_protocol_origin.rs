//! The scheme the broker mints its own absolute URLs with.
//!
//! ETSI GS CIM 009 v1.9.1 clause 6.2: "the `apiRoot` includes the scheme
//! (`http` or `https`), host and optional port. The API shall support HTTP over
//! TLS (also known as HTTPS - see IETF RFC 2818 [18]). TLS version 1.2 as
//! defined by IETF RFC 5246 [19] shall be supported. HTTP without TLS is not
//! recommended."
//!
//! Andromeda meets that clause by terminating TLS at a reverse proxy
//! (`docs/tls-deployment.md`), which leaves the broker serving plaintext HTTP.
//! It previously hardcoded `http://` when reconstructing its own origin, so a
//! broker behind a terminator advertised `http://` URLs for an `https://`
//! deployment -- including the `@context` URL clause 5.13 has it store, which
//! outlives the request that created it.
//!
//! The fix reads `Forwarded` (IETF RFC 7239) and `X-Forwarded-Proto`, but only
//! when an operator has declared the edge trusted through
//! `ANDROMEDA_HTTP_FORWARDED_HEADERS`. Both fields are ordinary request fields,
//! so believing them by default would let any client choose the scheme of a
//! persisted `@context` URL.
//!
//! Asserting that needs two brokers, because the two halves of the behaviour
//! are two configurations of the same binary. This module drives both: the
//! broker under test, whose edge is untrusted, and the broker named by
//! [`TRUSTED_PROXY_BROKER_ENV`], started with
//! `ANDROMEDA_HTTP_FORWARDED_HEADERS=trusted` against the same database.
//! Neither half is optional. A trusted broker that is not running fails these
//! tests rather than skipping them, because the assertions that discriminate a
//! working implementation from the hardcoded `http://` it replaced are exactly
//! the ones only that half can make. `docs/tls-deployment.md` states how the
//! tier is started.

#![allow(clippy::expect_used, clippy::unwrap_used, reason = "integration test setup uses explicit assertions")]

use andromeda_http_headers::{forwarded_header_trust::ForwardedHeaderTrust, forwarded_protocol::ForwardedProtocol};
use andromeda_test_client::{broker_endpoint::BrokerEndpoint, test_client::TestClient};
use andromeda_test_fixtures::context::CORE_CONTEXT;
use andromeda_test_harness::{
    environment::TestEnvironment,
    readiness::{ReadinessPolicy, wait_until_ready},
};
use pretty_assertions::assert_eq;
use reqwest::{Method, StatusCode};
use serde_json::{Value, json};
use std::{env, time::Duration};
use url::Url;

/// Names the tier's trusted-edge broker.
const TRUSTED_PROXY_BROKER_ENV: &str = "ANDROMEDA_TEST_TRUSTED_PROXY_BROKER_URL";

/// The endpoint the tier's trusted-edge broker listens on when
/// [`TRUSTED_PROXY_BROKER_ENV`] names no other.
const DEFAULT_TRUSTED_PROXY_BROKER: &str = "http://127.0.0.1:1057/ngsi-ld/v1/";

/// What a tier whose trusted half is missing needs, quoted into the failure.
const TIER_SETUP: &str = "start a second broker beside the one under test with ANDROMEDA_HTTP_FORWARDED_HEADERS=trusted against the same database, as docs/tls-deployment.md describes";

/// How long the trusted-edge broker is given to answer its health endpoint.
const READINESS: ReadinessPolicy = ReadinessPolicy::new(Duration::from_secs(20), Duration::from_millis(250), Duration::from_secs(2));

/// One row of the forwarded-protocol matrix: the fields a request carries and
/// the scheme the edge under assertion must mint from them.
struct ForwardedCase {
    /// The forwarded fields the request carries, in the order it sends them.
    fields: &'static [(&'static str, &'static str)],
    /// The scheme the URL the broker mints and stores must carry.
    minted: ForwardedProtocol,
}

impl ForwardedCase {
    /// Pair a set of forwarded fields with the scheme they must produce.
    const fn new(fields: &'static [(&'static str, &'static str)], minted: ForwardedProtocol) -> ForwardedCase {
        ForwardedCase { fields, minted }
    }
}

/// Every readable forwarded-field shape a proxy in front of the broker sends,
/// paired with the scheme a **trusted** edge must mint from it.
///
/// The untrusted half of the tier mints `http` for every one of these, which is
/// the security property rather than a per-row expectation, so it is asserted
/// as an invariant over the whole table.
const READABLE_FIELDS: &[ForwardedCase] = &[
    // The de-facto field alone.
    ForwardedCase::new(&[("X-Forwarded-Proto", "https")], ForwardedProtocol::Https),
    // The RFC 7239 field alone.
    ForwardedCase::new(&[("Forwarded", "proto=https")], ForwardedProtocol::Https),
    // Both, agreeing, as the reference terminator configuration sends them.
    ForwardedCase::new(&[("Forwarded", "proto=https"), ("X-Forwarded-Proto", "https")], ForwardedProtocol::Https),
    // RFC 7239 clause 4 permits a quoted string wherever it permits a token.
    ForwardedCase::new(&[("Forwarded", "proto=\"https\"")], ForwardedProtocol::Https),
    // A complete RFC 7239 element, whose other parameters change nothing.
    ForwardedCase::new(&[("Forwarded", "for=192.0.2.60;proto=https;host=terminator.example")], ForwardedProtocol::Https),
    // A `for=`-only element names no protocol, so the de-facto field decides.
    ForwardedCase::new(&[("Forwarded", "for=192.0.2.60"), ("X-Forwarded-Proto", "https")], ForwardedProtocol::Https),
    // A `for=`-only element with nothing beside it leaves the listener scheme.
    ForwardedCase::new(&[("Forwarded", "for=192.0.2.60")], ForwardedProtocol::Http),
    // Trusting the edge is not a blanket upgrade: a terminator that forwarded
    // plaintext gets plaintext back.
    ForwardedCase::new(&[("X-Forwarded-Proto", "http")], ForwardedProtocol::Http),
    ForwardedCase::new(&[("Forwarded", "proto=http")], ForwardedProtocol::Http),
];

/// The field shapes the broker cannot read a protocol from.
///
/// Both halves of the tier mint the listener scheme for these: a forwarded
/// scheme decides only how a minted URL is spelled and is never a precondition
/// of the operation the request asked for.
const UNREADABLE_FIELDS: &[ForwardedCase] = &[
    ForwardedCase::new(&[("Forwarded", "definitely not rfc 7239")], ForwardedProtocol::Http),
    ForwardedCase::new(&[("Forwarded", ";;;")], ForwardedProtocol::Http),
    // Clause 6.2 admits `http` and `https` only, so a third scheme is refused
    // rather than minted into an `apiRoot`.
    ForwardedCase::new(&[("Forwarded", "proto=gopher")], ForwardedProtocol::Http),
    ForwardedCase::new(&[("X-Forwarded-Proto", "not-a-scheme")], ForwardedProtocol::Http),
    ForwardedCase::new(&[("X-Forwarded-Proto", "ws")], ForwardedProtocol::Http),
];

/// One half of the tier: a broker and the trust its operator declared for the
/// forwarded fields it is sent.
struct TierEdge<'client> {
    /// The broker the cases are driven against.
    client: &'client TestClient,
    /// The trust that broker was started with, named in every failure.
    trust: ForwardedHeaderTrust,
}

impl TierEdge<'_> {
    /// Assert that every case mints a stored `@context` URL carrying the scheme
    /// it names, at the authority the request itself named.
    async fn assert_mints(&self, cases: &[ForwardedCase]) {
        for case in cases {
            let minted = minted_context_url(self.client, case.fields).await;
            assert_eq!(
                minted.scheme(),
                case.minted.as_scheme(),
                "{} edge, fields {:?}: minted {minted}",
                self.trust,
                case.fields
            );
            assert_eq!(
                minted.authority(),
                self.client.endpoint().url().authority(),
                "the authority half of a minted URL comes from the request's own Host field: {minted}"
            );
        }
    }
}

/// A `@context` payload the broker will host under an identifier of its own.
fn hosted_context() -> Value {
    json!({"@context": {"regressionForwardedProto": "https://example.org/regression/forwardedProto"}})
}

/// Create a hosted `@context` through `client` with the given forwarded fields,
/// read back the absolute URL the broker stored for it, and remove it again.
///
/// The stored URL is the durable consequence of the origin reconstruction:
/// clause 5.13 has the broker serve the `@context` at it, and every later
/// reader is handed that value.
async fn minted_context_url(client: &TestClient, forwarded: &[(&str, &str)]) -> Url {
    let mut request = client
        .raw(Method::POST, "jsonldContexts")
        .header("Content-Type", "application/json")
        .json(&hosted_context());
    for (name, value) in forwarded {
        request = request.header(name, *value);
    }
    let created = request.send().await.expect("create hosted @context failed");
    assert_eq!(created.status, StatusCode::CREATED, "create @context with {forwarded:?}: {:?}", created.body);
    let location = created
        .location()
        .expect("a Location header must be valid text")
        .expect("a create answers with a Location")
        .to_owned();

    let details = client
        .raw(Method::GET, &format!("{location}?details=true"))
        .send()
        .await
        .expect("read hosted @context metadata failed");
    assert_eq!(details.status, StatusCode::OK, "read @context: {:?}", details.body);
    let body = details.json_value().expect("@context metadata must be valid JSON");
    assert!(body["URL"].is_string(), "a hosted @context created with a Host header carries a URL: {body}");
    let minted = Url::parse(body["URL"].as_str().expect("asserted to be a string above")).expect("a stored @context URL is absolute");

    let removed = client.raw(Method::DELETE, &location).send().await.expect("delete hosted @context failed");
    assert_eq!(removed.status, StatusCode::NO_CONTENT, "delete @context: {:?}", removed.body);
    minted
}

/// The tier's trusted-edge broker, once it answers its health endpoint.
///
/// A tier whose trusted half is not running is a broken tier rather than a
/// reason to assert less, so this fails the calling test and says what to start.
///
/// The caller still opens a [`TestEnvironment`] around the work: the trusted
/// broker shares the database of the broker under test, so driving it takes the
/// same shared lease that keeps a destructive global reset out of the run.
async fn trusted_proxy_client() -> TestClient {
    let configured = env::var(TRUSTED_PROXY_BROKER_ENV)
        .ok()
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| DEFAULT_TRUSTED_PROXY_BROKER.to_owned());
    let endpoint = BrokerEndpoint::parse(&configured).expect("a valid trusted-proxy broker URL");
    let client = TestClient::new(endpoint, None);
    let ready = wait_until_ready(&client, READINESS).await;
    assert!(
        ready.is_ok(),
        "the tier's trusted-edge broker must answer at {configured}, overridden with {TRUSTED_PROXY_BROKER_ENV}: {TIER_SETUP}. {ready:?}"
    );
    client
}

/// The security property, and the one that matters most: with the default
/// configuration no forwarded field a client supplies may decide the scheme of
/// a URL the broker mints and stores.
#[tokio::test]
async fn regression_forwarded_protocol_is_ignored_without_a_trusted_proxy() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let untrusted = TierEdge {
        client: scope.client(),
        trust: ForwardedHeaderTrust::Untrusted,
    };

    let ignored: Vec<ForwardedCase> = READABLE_FIELDS
        .iter()
        .map(|case| ForwardedCase::new(case.fields, ForwardedProtocol::Http))
        .collect();
    untrusted.assert_mints(&ignored).await;

    scope.finish().await.expect("cleanup failed");
}

/// A broker whose operator declared its edge trusted mints the scheme the
/// terminator forwarded, so clause 6.2's `apiRoot` names the origin the client
/// actually reached rather than the broker's plaintext listener.
#[tokio::test]
async fn regression_forwarded_protocol_is_honoured_behind_a_trusted_proxy() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = trusted_proxy_client().await;
    let trusted = TierEdge {
        client: &client,
        trust: ForwardedHeaderTrust::Trusted,
    };

    trusted.assert_mints(READABLE_FIELDS).await;

    scope.finish().await.expect("cleanup failed");
}

/// RFC 7239 clause 1 makes `Forwarded` the standards-track replacement for the
/// `X-Forwarded-*` family, so at a trusted edge it decides whenever it names a
/// protocol -- whichever protocol that is, and whichever order the two fields
/// arrive in.
#[tokio::test]
async fn regression_the_rfc_7239_field_outranks_the_de_facto_one() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = trusted_proxy_client().await;
    let trusted = TierEdge {
        client: &client,
        trust: ForwardedHeaderTrust::Trusted,
    };

    trusted
        .assert_mints(&[
            ForwardedCase::new(&[("Forwarded", "proto=https"), ("X-Forwarded-Proto", "http")], ForwardedProtocol::Https),
            ForwardedCase::new(&[("Forwarded", "proto=http"), ("X-Forwarded-Proto", "https")], ForwardedProtocol::Http),
            // The same two disagreements with the fields sent the other way
            // round: the broker reads by field name, never by arrival order.
            ForwardedCase::new(&[("X-Forwarded-Proto", "http"), ("Forwarded", "proto=https")], ForwardedProtocol::Https),
            ForwardedCase::new(&[("X-Forwarded-Proto", "https"), ("Forwarded", "proto=http")], ForwardedProtocol::Http),
        ])
        .await;

    scope.finish().await.expect("cleanup failed");
}

/// RFC 7239 clause 4 has each proxy append its own element, so the leftmost is
/// the element closest to the original client -- the public edge whose scheme a
/// client would use to come back to a minted URL.
#[tokio::test]
async fn regression_the_leftmost_forwarded_element_decides_the_protocol() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = trusted_proxy_client().await;
    let trusted = TierEdge {
        client: &client,
        trust: ForwardedHeaderTrust::Trusted,
    };

    trusted
        .assert_mints(&[
            ForwardedCase::new(&[("Forwarded", "proto=https,proto=http")], ForwardedProtocol::Https),
            ForwardedCase::new(&[("Forwarded", "proto=http,proto=https")], ForwardedProtocol::Http),
            ForwardedCase::new(&[("X-Forwarded-Proto", "https, http")], ForwardedProtocol::Https),
            ForwardedCase::new(&[("X-Forwarded-Proto", "http, https")], ForwardedProtocol::Http),
        ])
        .await;

    scope.finish().await.expect("cleanup failed");
}

/// A forwarded field the broker cannot read must cost the request nothing: the
/// scheme decides only how a minted URL is spelled and is never a precondition
/// of the operation. Both halves of the tier fall back to the listener scheme,
/// so a trusted edge is no more talkable into a bad `apiRoot` than an untrusted
/// one.
#[tokio::test]
async fn regression_a_malformed_forwarded_field_does_not_fail_the_request() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let untrusted = TierEdge {
        client: scope.client(),
        trust: ForwardedHeaderTrust::Untrusted,
    };
    untrusted.assert_mints(UNREADABLE_FIELDS).await;

    let client = trusted_proxy_client().await;
    let trusted = TierEdge {
        client: &client,
        trust: ForwardedHeaderTrust::Trusted,
    };
    trusted.assert_mints(UNREADABLE_FIELDS).await;

    scope.finish().await.expect("cleanup failed");
}

/// Clause 6.12.3.1 and the pagination clauses put no origin in these fields, so
/// they carry no scheme to get wrong and are correct behind any terminator, at
/// any mount point. This pins that they stay relative rather than acquiring an
/// origin along with the forwarded-protocol handling -- at a trusted edge as
/// much as at an untrusted one.
#[tokio::test]
async fn regression_location_and_pagination_links_stay_relative_to_the_api_root() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    assert_relative_fields(scope.client(), &scope.id("ContextSourceRegistration:relative-fields-untrusted")).await;

    let trusted = trusted_proxy_client().await;
    assert_relative_fields(&trusted, &scope.id("ContextSourceRegistration:relative-fields-trusted")).await;

    scope.finish().await.expect("cleanup failed");
}

/// Register a Context Source through `client` while claiming an `https` edge,
/// then assert that neither the create's `Location` nor the pagination `Link`
/// fields of a limited discovery acquired an origin.
async fn assert_relative_fields(client: &TestClient, registration_id: &str) {
    let registration = json!({
        "id": registration_id,
        "type": "ContextSourceRegistration",
        "information": [{"entities": [{"type": "Vehicle"}]}],
        "endpoint": "http://context-source.invalid:1026/",
        "@context": [CORE_CONTEXT]
    });
    let created = client
        .raw(Method::POST, "csourceRegistrations")
        .header("Content-Type", "application/ld+json")
        .header("X-Forwarded-Proto", "https")
        .json(&registration)
        .send()
        .await
        .expect("create registration failed");
    assert_eq!(created.status, StatusCode::CREATED, "create: {:?}", created.body);
    let location = created
        .location()
        .expect("a Location header must be valid text")
        .expect("a create answers with a Location");
    assert!(
        location.starts_with("/ngsi-ld/v1/csourceRegistrations/"),
        "a Location is root-relative and carries no origin: {location}"
    );

    let page = client
        .raw(Method::GET, "csourceRegistrations?type=Vehicle&limit=1")
        .header("X-Forwarded-Proto", "https")
        .send()
        .await
        .expect("registration discovery failed");
    assert_eq!(page.status, StatusCode::OK, "discovery: {:?}", page.body);
    let pagination_targets: Vec<&str> = page
        .headers
        .get_all("link")
        .into_iter()
        .filter_map(|value| value.to_str().ok())
        .flat_map(|value| value.split(','))
        .filter(|link| ["prev", "next", "first", "last"].iter().any(|rel| link.contains(&format!("rel=\"{rel}\""))))
        .filter_map(|link| link.split_once('<').and_then(|(_, rest)| rest.split_once('>')).map(|(target, _)| target))
        .collect();
    assert!(
        !pagination_targets.is_empty(),
        "a limited discovery emits pagination Link fields: {:?}",
        page.headers.get_all("link").into_iter().collect::<Vec<_>>()
    );
    for target in &pagination_targets {
        assert!(
            target.starts_with("/ngsi-ld/v1/csourceRegistrations"),
            "a pagination Link is path-only and carries no origin: {target}"
        );
    }
}
