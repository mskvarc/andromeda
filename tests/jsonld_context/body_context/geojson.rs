//! The `@context` of a `GeoJSON` representation.
//!
//! Clause 4.5.16.3 has a `FeatureCollection` carry one `@context` at its top
//! level: "separate @context elements for each Feature will not be present in
//! the payload body"; a single Feature carries its own (clause 4.5.16.2). The
//! value follows the same rule as any `application/ld+json` body.

use crate::{
    core_contexts::core_value,
    ngsi_requests::{create_device, device_query, get, ok_body},
    private_tenant::PrivateTenant,
    unique_ids::unique_id,
    user_contexts::UserContexts,
};
use serde_json::{Value, json};

/// A `FeatureCollection` names the rule's value once, on the collection, and no
/// Feature names one; a single Feature names it itself.
#[tokio::test]
async fn a_feature_collection_names_the_context_once_and_a_feature_names_its_own() {
    let tenant = PrivateTenant::provisioned().await;
    let contexts = UserContexts::start().await;
    let device = unique_id("urn:ngsi-ld:Device:geojson");
    create_device(tenant.client(), &device, &contexts.terms).await;
    let cases: [(Option<&str>, Value); 3] = [
        (None, core_value()),
        (Some(&contexts.terms), json!([contexts.terms, core_value()])),
        (Some(&contexts.wrapper), json!(contexts.wrapper)),
    ];

    for (link, expected) in cases {
        let collection = ok_body(get(tenant.client(), &device_query(&device), "application/geo+json", link).await).await;
        assert_eq!(collection["type"], "FeatureCollection", "{collection}");
        assert_eq!(collection["@context"], expected, "{link:?}: {collection}");
        let features = collection["features"].as_array().expect("features");
        assert_eq!(features.len(), 1, "{collection}");
        assert!(
            features[0].get("@context").is_none(),
            "no Feature of a collection names a @context: {collection}"
        );

        let feature = ok_body(get(tenant.client(), &format!("entities/{device}"), "application/geo+json", link).await).await;
        assert_eq!(feature["type"], "Feature", "{feature}");
        assert_eq!(feature["@context"], expected, "{link:?}: {feature}");
    }
    contexts.shutdown().await;
    tenant.finish().await;
}
