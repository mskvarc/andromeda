//! The request a single-entity fallback write sends for one narrowed entity
//! (clauses 6.4 and 6.5).

use crate::{
    error::FederationError,
    forwarding_url::forwarding_url,
    provision_batch::{fallback_plan::SingleWrite, narrowing::NarrowedEntity},
};
use serde_json::Value;
use url::Url;

/// The request one single-entity fallback write sends for `entity`.
pub(super) fn single_write_request(
    write: SingleWrite,
    endpoint: &Url,
    entity: &NarrowedEntity,
) -> (Result<Url, FederationError>, reqwest::Method, Option<Vec<u8>>) {
    let id = entity.id.as_str();
    let fragment = || entity.body.as_ref().map(without_identifier).and_then(|body| serde_json::to_vec(&body).ok());
    let whole = || entity.body.as_ref().and_then(|body| serde_json::to_vec(body).ok());
    match write {
        SingleWrite::Create | SingleWrite::CreateThenOnConflict(_) => (forwarding_url(endpoint, &["entities"], None), reqwest::Method::POST, whole()),
        SingleWrite::Replace => (forwarding_url(endpoint, &["entities", id], None), reqwest::Method::PUT, whole()),
        SingleWrite::UpdateAttributes => (forwarding_url(endpoint, &["entities", id, "attrs"], None), reqwest::Method::PATCH, fragment()),
        SingleWrite::AppendAttributesNoOverwrite => (
            forwarding_url(endpoint, &["entities", id, "attrs"], Some("options=noOverwrite")),
            reqwest::Method::POST,
            fragment(),
        ),
        SingleWrite::Delete => (forwarding_url(endpoint, &["entities", id], None), reqwest::Method::DELETE, None),
        SingleWrite::Merge => (forwarding_url(endpoint, &["entities", id], None), reqwest::Method::PATCH, fragment()),
    }
}

/// An entity fragment for the attribute-level operations: the entity without
/// its `id`, which the request path already names.
fn without_identifier(body: &Value) -> Value {
    match body.as_object() {
        Some(object) => Value::Object(
            object
                .iter()
                .filter(|(name, _)| name.as_str() != "id")
                .map(|(name, value)| (name.clone(), value.clone()))
                .collect(),
        ), // clone: the fragment is a new value
        None => body.clone(), // clone: same
    }
}

#[cfg(test)]
mod tests {
    use crate::provision_batch::{
        fallback_plan::{SingleWrite, UpsertFollowUp},
        narrowing::NarrowedEntity,
        registration_fixture::uri,
        single_write::single_write_request,
    };
    use andromeda_jsonld::{batch_contexts::BatchContexts, context_spec::ContextSpec};
    use serde_json::{Value, json};
    use url::Url;

    #[test]
    fn every_single_entity_fallback_addresses_its_clause_6_4_and_6_5_resource() {
        let endpoint = Url::parse("http://peer").expect("valid URL");
        let context = BatchContexts::shared(ContextSpec::none()).context_of(None);
        let entity = NarrowedEntity {
            id: uri("urn:ngsi-ld:Vehicle:1"),
            body: Some(json!({"id": "urn:ngsi-ld:Vehicle:1", "type": "Vehicle", "speed": {"type": "Property", "value": 1}})),
            proxied_attrs: Vec::new(),
            context: &context,
        };
        let cases = [
            (SingleWrite::Create, reqwest::Method::POST, "http://peer/ngsi-ld/v1/entities", true),
            (
                SingleWrite::CreateThenOnConflict(UpsertFollowUp::Replace),
                reqwest::Method::POST,
                "http://peer/ngsi-ld/v1/entities",
                true,
            ),
            (
                SingleWrite::Replace,
                reqwest::Method::PUT,
                "http://peer/ngsi-ld/v1/entities/urn:ngsi-ld:Vehicle:1",
                true,
            ),
            (
                SingleWrite::UpdateAttributes,
                reqwest::Method::PATCH,
                "http://peer/ngsi-ld/v1/entities/urn:ngsi-ld:Vehicle:1/attrs",
                false,
            ),
            (
                SingleWrite::AppendAttributesNoOverwrite,
                reqwest::Method::POST,
                "http://peer/ngsi-ld/v1/entities/urn:ngsi-ld:Vehicle:1/attrs?options=noOverwrite",
                false,
            ),
            (
                SingleWrite::Delete,
                reqwest::Method::DELETE,
                "http://peer/ngsi-ld/v1/entities/urn:ngsi-ld:Vehicle:1",
                false,
            ),
            (
                SingleWrite::Merge,
                reqwest::Method::PATCH,
                "http://peer/ngsi-ld/v1/entities/urn:ngsi-ld:Vehicle:1",
                false,
            ),
        ];
        for (write, expected_method, expected_url, keeps_id) in cases {
            let (url, method, body) = single_write_request(write, &endpoint, &entity);
            pretty_assertions::assert_eq!(url.expect("joins").as_str(), expected_url, "{write:?}");
            pretty_assertions::assert_eq!(method, expected_method, "{write:?}");
            if write == SingleWrite::Delete {
                assert!(body.is_none(), "a deletion carries no body");
                continue;
            }
            let body: Value = serde_json::from_slice(&body.expect("a body")).expect("JSON body");
            pretty_assertions::assert_eq!(body.get("id").is_some(), keeps_id, "{write:?}");
            assert!(body.get("speed").is_some(), "{write:?}");
        }
    }
}
