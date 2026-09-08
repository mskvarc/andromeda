//! Context Source Identity data model (clause 5.2.40). The broker's own
//! `GET /info/sourceIdentity` is validated positively and its configured
//! `contextSourceExtras` checked verbatim (`T-LOCAL`); adversarial
//! validation-on-consume of a peer's identity runs against a mock source
//! (`T-MOCK`) whose endpoint is the mock origin, so the broker's canonical
//! `/ngsi-ld/v1/info/sourceIdentity` path lands on the stub.

use crate::{
    context_source_adapter::ContextSourceAdapter,
    oracle::{assert_status, json_body},
    registration::CsrBuilder,
    topology::{self, BrokerRole},
};
use reqwest::Method;
use serde_json::{Value, json};

/// The broker's own `GET /info/sourceIdentity` returns a conformant
/// `ContextSourceIdentity` with all mandatory members present and well-formed
/// (clause 5.2.40). Confirms REQ-REG-43, REQ-REG-44, REQ-REG-45, REQ-REG-46,
/// REQ-REG-47.
#[tokio::test]
async fn source_identity_returns_conformant_identity() {
    let topology = topology::single().await.expect("single-broker topology");
    let response = topology
        .broker(BrokerRole::A)
        .expect("topology broker A")
        .client()
        .source_identity(Some("application/ld+json"))
        .await
        .expect("source identity request");
    assert_status(&response, 200);
    let body = json_body(&response);

    let identity_id = body["id"].as_str().unwrap_or_else(|| panic!("ContextSourceIdentity id is a string"));
    assert!(identity_id.contains(':'), "the identity id is a URI, got {identity_id}");
    assert_eq!(
        body["type"].as_str(),
        Some("ContextSourceIdentity"),
        "the identity type is ContextSourceIdentity"
    );
    let alias = body["contextSourceAlias"].as_str().unwrap_or_else(|| panic!("contextSourceAlias is a string"));
    assert!(!alias.is_empty(), "contextSourceAlias is non-empty");
    let uptime = body["contextSourceUptime"]
        .as_str()
        .unwrap_or_else(|| panic!("contextSourceUptime is a string"));
    assert!(uptime.starts_with('P'), "contextSourceUptime is an ISO 8601 duration, got {uptime}");
    let time_at = body["contextSourceTimeAt"]
        .as_str()
        .unwrap_or_else(|| panic!("contextSourceTimeAt is a string"));
    assert!(time_at.contains('T'), "contextSourceTimeAt is a DateTime, got {time_at}");

    topology.finish().await.expect("topology cleanup");
}

/// The deployment's configured `contextSourceExtras` is preserved byte-identically
/// with no JSON-LD expansion (Table 5.2.40-1: "raw un-expandable JSON which
/// shall not be interpreted as JSON-LD using the supplied @context"). The
/// compose stack configures `{"deployment":"distop"}` on broker A. Confirms
/// REQ-REG-48.
#[tokio::test]
async fn source_identity_preserves_extras_verbatim() {
    let topology = topology::single().await.expect("single-broker topology");
    let response = topology
        .broker(BrokerRole::A)
        .expect("topology broker A")
        .client()
        .source_identity(Some("application/ld+json"))
        .await
        .expect("source identity request");
    assert_status(&response, 200);
    let body = json_body(&response);
    assert_eq!(
        body["contextSourceExtras"],
        json!({ "deployment": "distop" }),
        "the configured extras are present verbatim in the identity document: {body}"
    );

    topology.finish().await.expect("topology cleanup");
}

/// Generate one `#[tokio::test]` per malformed-identity row. Each stands up a mock
/// context source that serves a `ContextSourceIdentity` violating one mandatory
/// field, registers it, and asserts the broker does not adopt the malformed
/// identity in place of its own conformant one (clause 5.2.40, negative side).
macro_rules! source_identity_rejection_cases {
    ($($name:ident => $identity:expr;)+) => {
        $(
            #[tokio::test]
            async fn $name() {
                let topology = topology::single().await.expect("single-broker topology");
                let adapter = ContextSourceAdapter::start().await;
                let identity: Value = $identity;
                adapter.set_stub_with_body(Method::GET, "/ngsi-ld/v1/info/sourceIdentity", 200, &identity).await;

                let registration_id = topology.id(concat!("registration:", stringify!($name)));
                let registration = CsrBuilder::new(&registration_id, &adapter.url(""))
                    .mode("auxiliary")
                    .operations(&["retrieveEntity", "queryEntity"])
                    .build();
                let registered = topology.broker(BrokerRole::A).expect("topology broker A").client().create_registration(&registration).await.expect("register mock source");
                assert_status(&registered, 201);

                let own = topology.broker(BrokerRole::A).expect("topology broker A").client().source_identity(Some("application/ld+json")).await.expect("broker source identity");
                assert_status(&own, 200);
                assert_eq!(
                    json_body(&own)["type"].as_str(),
                    Some("ContextSourceIdentity"),
                    "the broker keeps its own conformant identity and does not adopt the malformed peer identity"
                );

                adapter.shutdown().await;
                topology.finish().await.expect("topology cleanup");
            }
        )+
    };
}

source_identity_rejection_cases! {
    source_identity_missing_id_rejected => json!({
        "type": "ContextSourceIdentity",
        "contextSourceAlias": "peer",
        "contextSourceUptime": "PT1H",
        "contextSourceTimeAt": "2026-01-01T00:00:00Z"
    });
    source_identity_wrong_type_rejected => json!({
        "id": "urn:ngsi-ld:ContextSourceIdentity:peer",
        "type": "Vehicle",
        "contextSourceAlias": "peer",
        "contextSourceUptime": "PT1H",
        "contextSourceTimeAt": "2026-01-01T00:00:00Z"
    });
    source_identity_empty_alias_rejected => json!({
        "id": "urn:ngsi-ld:ContextSourceIdentity:peer",
        "type": "ContextSourceIdentity",
        "contextSourceAlias": "",
        "contextSourceUptime": "PT1H",
        "contextSourceTimeAt": "2026-01-01T00:00:00Z"
    });
    source_identity_invalid_uptime_rejected => json!({
        "id": "urn:ngsi-ld:ContextSourceIdentity:peer",
        "type": "ContextSourceIdentity",
        "contextSourceAlias": "peer",
        "contextSourceUptime": "1 hour",
        "contextSourceTimeAt": "2026-01-01T00:00:00Z"
    });
    source_identity_invalid_timeat_rejected => json!({
        "id": "urn:ngsi-ld:ContextSourceIdentity:peer",
        "type": "ContextSourceIdentity",
        "contextSourceAlias": "peer",
        "contextSourceUptime": "PT1H",
        "contextSourceTimeAt": "not-a-date"
    });
}
