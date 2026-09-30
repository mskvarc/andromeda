//! How an identifier may be spelled when addressing a stored `@context`
//! (study G3 and G4).
//!
//! IETF RFC 8141 section 3.1 compares the `urn` scheme and the namespace
//! identifier case-insensitively and the namespace-specific string
//! case-sensitively, so `URN:NGSI-LD:JsonLdContext:<id>` names the same entry
//! as the lowercase spelling, while `jsonldcontext` is another kind.

use crate::{
    context_api::{JSON_LD_CONTEXT_PREFIX, add_hosted, get_context},
    private_tenant::PrivateTenant,
    problem_types::RESOURCE_NOT_FOUND_TYPE,
};
use reqwest::StatusCode;
use serde_json::json;

/// Study G3: an uppercase `urn` and namespace identifier address the entry
/// stored under the lowercase spelling.
#[tokio::test]
async fn an_uppercase_urn_prefix_addresses_the_stored_entry() {
    let tenant = PrivateTenant::provisioned().await;
    let document = json!({ "@context": { "Spot": "https://e.org/Spot" } });
    let id = add_hosted(tenant.client(), &document).await;
    let local = id.strip_prefix(JSON_LD_CONTEXT_PREFIX).expect("a minted identifier");

    let (status, body) = get_context(tenant.client(), &format!("URN:NGSI-LD:JsonLdContext:{local}"), "").await;

    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body, document);
    tenant.finish().await;
}

/// Study G4: the kind is part of the case-sensitive namespace-specific string,
/// so a lowercase kind names no entry.
#[tokio::test]
async fn a_lowercase_kind_names_no_stored_entry() {
    let tenant = PrivateTenant::provisioned().await;
    let id = add_hosted(tenant.client(), &json!({ "@context": { "Spot": "https://e.org/Spot" } })).await;
    let local = id.strip_prefix(JSON_LD_CONTEXT_PREFIX).expect("a minted identifier");

    let (status, body) = get_context(tenant.client(), &format!("urn:ngsi-ld:jsonldcontext:{local}"), "").await;

    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
    assert_eq!(body["type"], RESOURCE_NOT_FOUND_TYPE.as_str());
    tenant.finish().await;
}
