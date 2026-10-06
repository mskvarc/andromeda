//! The @context registry round trips a hosted document through its typed
//! model (clauses 5.13.2, 5.13.3 and 5.13.4; bindings 6.29 and 6.30).
//!
//! Needs the host broker: the case drives the HTTP API and is not run without
//! one.

use crate::{
    context_api::{create_context, extract_id, get_context, list_contexts},
    private_tenant::PrivateTenant,
};
use reqwest::StatusCode;
use serde_json::json;

/// Clauses 5.13.2 and 5.13.4: the document added with Add @context is the
/// document Serve @context answers, the clause 5.13.3.5 details name the entry
/// the registry holds for it (`kind`, `localId`), and the clause 5.13.3 list
/// reports the same entry.
#[tokio::test]
async fn jsonld_context_registry_round_trips_through_its_typed_model() {
    let tenant = PrivateTenant::provisioned().await;
    let document = json!({
        "@context": {
            "A1": "urn:ngsi-ld:attributes:A1",
            "A2": {"@id": "urn:ngsi-ld:attributes:A2", "@type": "@vocab"},
            "A3": "urn:ngsi-ld:attributes:A3"
        }
    });

    let (status, location, problem) = create_context(tenant.client(), &document).await;
    assert_eq!(status, StatusCode::CREATED, "add: {problem}");
    let context_id = extract_id(location.as_deref().expect("a Location header"));

    let (status, served) = get_context(tenant.client(), &context_id, "").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(served, document, "serve answers the document that was added");

    let (status, detail) = get_context(tenant.client(), &context_id, "details=true").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(detail["kind"], "Hosted", "{detail}");
    assert_eq!(detail["localId"], json!(context_id), "{detail}");

    let (status, listed) = list_contexts(tenant.client(), "details=true&kind=Hosted").await;
    assert_eq!(status, StatusCode::OK);
    let entry = listed
        .iter()
        .find(|entry| entry["localId"] == json!(context_id))
        .expect("the added context is listed");
    assert_eq!(entry["kind"], "Hosted", "{entry}");
    tenant.finish().await;
}
