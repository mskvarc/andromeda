//! The POST body of a forwarded batch entity query (clause 6.23: an NGSI-LD
//! `Query` sent to `/entityOperations/query`).
//!
//! A registration advertising `queryBatch` but not `queryEntity` (or a client
//! that queried via POST) is contacted through the batch query endpoint. Clause
//! 4.3.6.6 carries the JSON-LD `@context` in a Link header for the
//! `application/json` forwarded request, so the body holds only the compact
//! terms the narrowing produced.

use crate::{forwarded_parameter::ForwardedParameter, forwarded_query::ForwardedQuery};
use serde_json::{Map, Value};

/// One entity selector in a forwarded `Query` (clause 5.2.23): a compact type
/// and/or id, as the registration's narrowed coverage carries them.
pub(crate) struct QuerySelector {
    /// The compact entity type, when the query or registration constrains one.
    pub(crate) entity_type: Option<String>,
    /// The entity id, when the read names one.
    pub(crate) id: Option<String>,
}

impl QuerySelector {
    /// The clause 5.2.23 selector object holding whichever of `type`/`id` is set.
    fn to_object(&self) -> Value {
        let mut object = Map::new();
        if let Some(entity_type) = &self.entity_type {
            object.insert("type".to_string(), Value::String(entity_type.clone())); // clone: the body owns its serialized terms
        }
        if let Some(id) = &self.id {
            object.insert("id".to_string(), Value::String(id.clone())); // clone: same
        }
        Value::Object(object)
    }
}

/// The narrowed entity selectors of a query: one selector per covered id (each
/// carrying the covered type), or a single type-only selector when the query
/// names no id.
pub(crate) fn selectors_from_query(query: &ForwardedQuery) -> Vec<QuerySelector> {
    let entity_type = query.value_of(ForwardedParameter::Type).map(str::to_string);
    match query.value_of(ForwardedParameter::Id) {
        Some(ids) => ids
            .split(',')
            .map(|id| QuerySelector {
                entity_type: entity_type.clone(), // clone: one selector per id names the same type
                id: Some(id.to_string()),
            })
            .collect(),
        None => vec![QuerySelector { entity_type, id: None }],
    }
}

/// Build the `entityOperations/query` POST body: an NGSI-LD `Query` (clause
/// 5.2.23) with the narrowed entity selectors and, when present, the narrowed
/// attribute projection.
pub(crate) fn build_batch_query_body(selectors: &[QuerySelector], attrs: Option<&str>) -> Vec<u8> {
    let entities = selectors.iter().map(QuerySelector::to_object).collect::<Vec<_>>();
    let mut query = Map::new();
    query.insert("type".to_string(), Value::String("Query".to_string()));
    query.insert("entities".to_string(), Value::Array(entities));
    if let Some(attrs) = attrs {
        let list = attrs.split(',').map(|attr| Value::String(attr.to_string())).collect::<Vec<_>>();
        query.insert("attrs".to_string(), Value::Array(list));
    }
    // Serializing an in-memory object of strings cannot fail; the default is an
    // unreachable fallback that keeps the builder panic-free.
    serde_json::to_vec(&Value::Object(query)).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use crate::{
        batch_query_body::{QuerySelector, build_batch_query_body, selectors_from_query},
        config::SplitEntities,
        forwarded_names::ForwardedNames,
        forwarded_parameter::ForwardedParameter,
        forwarded_query::{ForwardedQuery, client_query::ClientQuery},
        forwarding_context::Rewording,
        term_resolver::TermResolver,
    };
    use andromeda_jsonld::{context_spec::ContextSpec, engine::processor::JsonLdEngine};
    use andromeda_ngsi_ld_model::registration::ContextSourceRegistration;
    use andromeda_ngsi_ld_request::query::entity::parameters::EntityQueryParameters;
    use serde_json::{Value, json};

    fn registration(information: &Value) -> ContextSourceRegistration {
        serde_json::from_value(json!({
            "id": "urn:ngsi-ld:csr:1",
            "type": "ContextSourceRegistration",
            "endpoint": "https://peer.example.org",
            "information": information,
            "status": "ok",
            "createdAt": "2024-01-01T00:00:00Z",
            "modifiedAt": "2024-01-01T00:00:00Z"
        }))
        .expect("valid registration")
    }

    /// The fork mock (and clause 6.23) require the first entity selector to carry
    /// both the type and the id the stubbed entity has, so a narrowed id+type
    /// query builds exactly that.
    #[tokio::test]
    async fn a_narrowed_id_and_type_query_builds_a_single_selector_with_both() {
        let by_type = registration(&json!([{"entities": [{"type": "https://uri.etsi.org/ngsi-ld/default-context/Vehicle"}]}]));
        let params: EntityQueryParameters = serde_urlencoded::from_str("type=Vehicle&id=urn:ngsi-ld:Vehicle:A1&attrs=speed").expect("valid query");
        let engine = JsonLdEngine::builder().allow_remote(false).build().expect("engine builds");
        let context = ContextSpec::none();
        let resolver = TermResolver::new(&engine, &context);
        let names = ForwardedNames::prepare(&engine, &resolver, &context, Rewording::AsInbound)
            .await
            .expect("the forwarding context processes");
        let forwarded = ForwardedQuery::for_registration(ClientQuery::EntityQuery(&params), &by_type, SplitEntities::Complete, None, &names)
            .await
            .expect("narrowing succeeds");
        let forwarded = forwarded.first().expect("covered");
        let selectors = selectors_from_query(forwarded);
        let body = build_batch_query_body(&selectors, forwarded.value_of(ForwardedParameter::Attrs));
        let value: Value = serde_json::from_slice(&body).expect("valid json");
        pretty_assertions::assert_eq!(value["type"], "Query");
        pretty_assertions::assert_eq!(value["entities"][0]["type"], "Vehicle");
        pretty_assertions::assert_eq!(value["entities"][0]["id"], "urn:ngsi-ld:Vehicle:A1");
        pretty_assertions::assert_eq!(value["attrs"], json!(["speed"]));
    }

    #[test]
    fn a_type_only_query_builds_a_type_only_selector_and_no_attrs() {
        let selectors = vec![QuerySelector {
            entity_type: Some("Vehicle".to_string()),
            id: None,
        }];
        let body = build_batch_query_body(&selectors, None);
        let value: Value = serde_json::from_slice(&body).expect("valid json");
        pretty_assertions::assert_eq!(value["entities"][0]["type"], "Vehicle");
        assert!(value["entities"][0].get("id").is_none());
        assert!(value.get("attrs").is_none());
    }

    #[test]
    fn an_explicit_type_and_id_selector_builds_both_members() {
        let selectors = vec![QuerySelector {
            entity_type: Some("Vehicle".to_string()),
            id: Some("urn:ngsi-ld:Vehicle:9".to_string()),
        }];
        let body = build_batch_query_body(&selectors, Some("speed"));
        let value: Value = serde_json::from_slice(&body).expect("valid json");
        pretty_assertions::assert_eq!(value["entities"][0]["type"], "Vehicle");
        pretty_assertions::assert_eq!(value["entities"][0]["id"], "urn:ngsi-ld:Vehicle:9");
        pretty_assertions::assert_eq!(value["attrs"], json!(["speed"]));
    }
}
