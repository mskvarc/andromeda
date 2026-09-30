//! Randomly minted identifiers (study G1 and G5, H21 and H22).

use crate::{
    context_api::{create_context, extract_id, find_cached_context_id, get_context, setup_cached_context},
    identity::uuid_version,
    private_tenant::PrivateTenant,
};
use reqwest::StatusCode;
use serde_json::json;
use uuid::Version;

/// Clause 5.13.2.4: every Add @context is "a new entry", so the same document
/// posted twice is two entries, each under its own random identifier.
#[tokio::test]
async fn adding_the_same_context_twice_creates_two_randomly_identified_entries() {
    let tenant = PrivateTenant::provisioned().await;
    let body = json!({ "@context": { "Spot": "https://example.org/Spot" } });

    let (first_status, first_location, _) = create_context(tenant.client(), &body).await;
    let (second_status, second_location, _) = create_context(tenant.client(), &body).await;

    assert_eq!(first_status, StatusCode::CREATED);
    assert_eq!(second_status, StatusCode::CREATED);
    let first = extract_id(first_location.as_deref().expect("a Location header"));
    let second = extract_id(second_location.as_deref().expect("a Location header"));
    assert_ne!(first, second);
    for (identifier, location) in [(&first, &first_location), (&second, &second_location)] {
        assert_eq!(uuid_version(identifier), Some(Version::Random), "{identifier}");
        assert_eq!(
            location.as_deref(),
            Some(format!("/ngsi-ld/v1/jsonldContexts/{identifier}").as_str()),
            "clause 6.29.3.1: the Location is the local relative path"
        );
        let (status, detail) = get_context(tenant.client(), identifier, "details=true").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(detail["localId"], json!(identifier));
    }
    tenant.finish().await;
}

/// A Cached entry is named by a random identifier, and Table 6.30.2-1 lets a
/// client address it by that identifier or by the URL it was downloaded from:
/// both return the same metadata.
#[tokio::test]
async fn a_cached_context_is_randomly_identified_and_addressable_by_either_identity() {
    let tenant = PrivateTenant::provisioned().await;
    let (origin, context_url) = setup_cached_context(tenant.client()).await;

    let local_id = find_cached_context_id(tenant.client(), &context_url).await;

    assert_eq!(uuid_version(&local_id), Some(Version::Random), "{local_id}");
    let (by_id_status, by_id) = get_context(tenant.client(), &local_id, "details=true").await;
    let (by_url_status, by_url) = get_context(tenant.client(), &context_url, "details=true").await;
    assert_eq!(by_id_status, StatusCode::OK);
    assert_eq!(by_url_status, StatusCode::OK);
    assert_eq!(by_id["localId"], by_url["localId"]);
    assert_eq!(by_id["URL"], by_url["URL"]);
    assert_eq!(by_id["kind"], "Cached");
    origin.shutdown().await;
    tenant.finish().await;
}
