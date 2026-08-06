//! Expanding an NGSI-LD entity without the JSON-LD processor.
//!
//! Walks the entity's DOM once and emits the expanded form directly, fusing
//! the concise-to-normalized step of clause 5.2.4 into the same descent. The
//! result must be what
//! `andromeda_ngsi_ld_representation::normalization::normalize_entity_attributes`
//! followed by a full expansion would produce, so every shape this module is
//! not certain of answers `None` and the caller falls back.

use crate::{
    core_context::{ANDROMEDA_CORE_CONTEXT_URL, same_core_context_document},
    engine::{expanded_key, fast_attribute::expand_attribute, term_map::BatchTermMap},
};
use serde_json::{Map, Value};

/// Expand a document holding one entity or an array of them.
///
/// Answers `None` as soon as any part of it falls outside the supported
/// subset.
pub(crate) fn expand_document(document: &Value, term_map: &BatchTermMap) -> Option<Value> {
    match document {
        Value::Object(entity) => Some(Value::Array(vec![expand_entity(entity, term_map)?])),
        Value::Array(items) => {
            let mut expanded = Vec::with_capacity(items.len());
            for item in items {
                expanded.push(expand_entity(item.as_object()?, term_map)?);
            }
            Some(Value::Array(expanded))
        }
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => None,
    }
}

/// Expand one entity object.
fn expand_entity(entity: &Map<String, Value>, term_map: &BatchTermMap) -> Option<Value> {
    // A per-entity `@context` overriding the batch's is clause 5.5.7 working
    // as specified, and it is exactly what this walker cannot honour.
    if let Some(context) = entity.get("@context")
        && !names_core_contexts_only(context, term_map.effective_core_url())
    {
        return None;
    }

    let mut expanded = Map::with_capacity(entity.len());

    for (key, value) in entity {
        match key.as_str() {
            "@context" => {}
            "id" | "@id" => {
                expanded.insert("@id".to_string(), Value::String(value.as_str()?.to_owned()));
            }
            "type" | "@type" => {
                expanded.insert("@type".to_string(), expand_entity_types(value, term_map)?);
            }
            "scope" => {
                expanded.insert(expanded_key::SCOPE.clone(), expand_scope(value)?);
            }
            // Clause 5.2.4 entity-level temporal metadata: the core context
            // coerces these to a dateTime. They are not attribute slots, so
            // the implicit-Property wrap must not apply to them.
            "expiresAt" | "createdAt" | "modifiedAt" | "deletedAt" => {
                let iri = term_map.expand_iri(key).into_owned();
                expanded.insert(iri, typed_date_time(value.as_str()?));
            }
            other if other.starts_with('@') => return None,
            attribute => {
                // A term with a scoped `@context` changes what its
                // sub-attributes mean, which this walker does not unfold.
                if term_map.term_has_scoped_context(attribute) {
                    return None;
                }
                let iri = term_map.expand_iri(attribute).into_owned();
                expanded.insert(iri, expand_attribute(value, term_map)?);
            }
        }
    }

    Some(Value::Object(expanded))
}

/// Expand an entity's `type`, which clause 5.2.4 allows to be one name or
/// several.
fn expand_entity_types(value: &Value, term_map: &BatchTermMap) -> Option<Value> {
    let names: &[Value] = match value {
        Value::String(_) => std::slice::from_ref(value),
        Value::Array(items) => items.as_slice(),
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::Object(_) => return None,
    };
    let mut iris = Vec::with_capacity(names.len());
    for name in names {
        iris.push(Value::String(term_map.expand_iri(name.as_str()?).into_owned()));
    }
    Some(Value::Array(iris))
}

/// Whether a per-entity `@context` names nothing but the core context the
/// batch already resolved against.
///
/// Only a core reference that resolves to the *same released version* the
/// deployment pinned may pass: the batch term table is that one version's, so
/// an inline reference to a different release (v1.6 under a v1.9 deployment)
/// would be expanded against the wrong table. Anything else redefines terms
/// this walker resolved from the batch table, so it falls to the processor.
fn names_core_contexts_only(context: &Value, effective_core_url: &str) -> bool {
    match context {
        Value::String(url) => is_effective_core_context(url, effective_core_url),
        Value::Array(items) => items
            .iter()
            .all(|item| item.as_str().is_some_and(|url| is_effective_core_context(url, effective_core_url))),
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::Object(_) => false,
    }
}

/// Whether a URL names Andromeda's own context, or a core context that resolves
/// to the same document the deployment pinned.
fn is_effective_core_context(url: &str, effective_core_url: &str) -> bool {
    url == ANDROMEDA_CORE_CONTEXT_URL || same_core_context_document(url, effective_core_url)
}

/// Expand `scope`, which clause 4.18 allows to be one path or several.
fn expand_scope(value: &Value) -> Option<Value> {
    let paths: &[Value] = match value {
        Value::String(_) => std::slice::from_ref(value),
        Value::Array(items) => items.as_slice(),
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::Object(_) => return None,
    };
    let mut expanded = Vec::with_capacity(paths.len());
    for path in paths {
        let mut node = Map::with_capacity(1);
        node.insert("@value".to_owned(), Value::String(path.as_str()?.to_owned()));
        expanded.push(Value::Object(node));
    }
    Some(Value::Array(expanded))
}

/// The expanded form of a value the core context coerces to a dateTime.
fn typed_date_time(timestamp: &str) -> Value {
    let mut node = Map::with_capacity(2);
    node.insert("@type".to_owned(), Value::String(expanded_key::DATE_TIME.clone()));
    node.insert("@value".to_owned(), Value::String(timestamp.to_owned()));
    Value::Array(vec![Value::Object(node)])
}

#[cfg(test)]
mod tests {
    use crate::{
        context_spec::ContextSpec,
        engine::{batch_context::BatchContextHandle, processor::JsonLdEngine},
    };
    use andromeda_ngsi_ld_representation::normalization::normalize_entity_attributes;
    use serde_json::{Value, json};

    /// An engine that resolves only what is embedded, never the network.
    fn offline_engine() -> JsonLdEngine {
        JsonLdEngine::builder().allow_remote(false).build().expect("engine builds")
    }

    /// A handle over the core context alone, which is fastpath-eligible.
    async fn core_handle(engine: &JsonLdEngine) -> BatchContextHandle {
        let handle = engine.batch_context_handle(&ContextSpec::core_only()).await;
        assert!(handle.fast_eligible(), "a core-only spec must admit the fastpath");
        handle
    }

    /// The claim this whole module makes: the fastpath answers exactly what
    /// the processor answers.
    ///
    /// Both sides are driven from the same document. When the fastpath
    /// refuses, `expand_with_handle` falls through to the processor, so this
    /// still asserts something — that the refusal produced the right answer.
    async fn assert_both_paths_agree(document: Value) {
        let engine = offline_engine();
        let handle = core_handle(&engine).await;

        let fast = engine
            .expand_with_handle(document.clone(), &handle) // clone: both paths consume a document
            .await
            .expect("the fastpath or its fallback expands");
        let slow = engine.expand(document, handle.spec()).await.expect("the processor expands");

        pretty_assertions::assert_eq!(fast, slow);
    }

    /// The fastpath alone, with no fallback: `None` means it refused.
    async fn fastpath_only(document: Value) -> Option<Value> {
        let engine = offline_engine();
        let handle = core_handle(&engine).await;
        engine.try_fast_expand(&document, &handle)
    }

    /// The fastpath must expand `document` itself — not refuse and fall back —
    /// and the result must equal the processor's. Used where the point is that
    /// the fastpath *models* a shape, so a fallback would hide the defect.
    async fn assert_fastpath_expands_and_agrees(document: Value) {
        let engine = offline_engine();
        let handle = core_handle(&engine).await;
        let fast = engine
            .try_fast_expand(&document, &handle)
            .expect("the fastpath must expand this document, not refuse it");
        let slow = engine.expand(document, handle.spec()).await.expect("the processor expands");
        pretty_assertions::assert_eq!(fast, slow);
    }

    #[tokio::test]
    async fn a_property_expands_identically_on_both_paths() {
        assert_both_paths_agree(json!({
            "id": "urn:ngsi-ld:Device:1",
            "type": "Device",
            "temperature": { "type": "Property", "value": 21 }
        }))
        .await;
    }

    #[tokio::test]
    async fn a_property_carrying_observed_at_and_unit_code_expands_identically() {
        assert_both_paths_agree(json!({
            "id": "urn:ngsi-ld:Device:2",
            "type": "Device",
            "temperature": { "type": "Property", "value": 21.5, "observedAt": "2024-01-01T00:00:00Z", "unitCode": "CEL" }
        }))
        .await;
    }

    #[tokio::test]
    async fn a_property_carrying_a_dataset_id_expands_identically() {
        assert_both_paths_agree(json!({
            "id": "urn:ngsi-ld:Device:3",
            "type": "Device",
            "temperature": { "type": "Property", "value": 21, "datasetId": "urn:ngsi-ld:Dataset:sensor-a" }
        }))
        .await;
    }

    #[tokio::test]
    async fn a_multi_dataset_attribute_expands_identically() {
        assert_both_paths_agree(json!({
            "id": "urn:ngsi-ld:Device:4",
            "type": "Device",
            "temperature": [
                { "type": "Property", "value": 21, "datasetId": "urn:ngsi-ld:Dataset:a" },
                { "type": "Property", "value": 22, "datasetId": "urn:ngsi-ld:Dataset:b" }
            ]
        }))
        .await;
    }

    #[tokio::test]
    async fn a_relationship_expands_identically() {
        assert_both_paths_agree(json!({
            "id": "urn:ngsi-ld:Device:5",
            "type": "Device",
            "belongsTo": { "type": "Relationship", "object": "urn:ngsi-ld:Site:1", "observedAt": "2024-01-01T00:00:00Z" }
        }))
        .await;
    }

    #[tokio::test]
    async fn an_entity_with_several_types_expands_identically() {
        assert_both_paths_agree(json!({
            "id": "urn:ngsi-ld:Device:6",
            "type": ["Device", "Sensor"],
            "temperature": { "type": "Property", "value": 21 }
        }))
        .await;
    }

    #[tokio::test]
    async fn a_geoproperty_point_expands_identically() {
        assert_both_paths_agree(json!({
            "id": "urn:ngsi-ld:Device:g1",
            "type": "Device",
            "location": { "type": "GeoProperty", "value": { "type": "Point", "coordinates": [13.4, 52.5] } }
        }))
        .await;
    }

    #[tokio::test]
    async fn a_geoproperty_polygon_expands_identically() {
        assert_both_paths_agree(json!({
            "id": "urn:ngsi-ld:Device:g2",
            "type": "Device",
            "footprint": {
                "type": "GeoProperty",
                "value": { "type": "Polygon", "coordinates": [[[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 0.0]]] }
            }
        }))
        .await;
    }

    #[tokio::test]
    async fn a_language_property_expands_identically() {
        assert_both_paths_agree(json!({
            "id": "urn:ngsi-ld:Device:lp1",
            "type": "Device",
            "description": { "type": "LanguageProperty", "languageMap": { "en": "hello", "de": "hallo" } }
        }))
        .await;
    }

    #[tokio::test]
    async fn a_vocab_property_expands_identically() {
        assert_both_paths_agree(json!({
            "id": "urn:ngsi-ld:Device:vp1",
            "type": "Device",
            "category": { "type": "VocabProperty", "vocab": "sensor" }
        }))
        .await;
    }

    #[tokio::test]
    async fn a_json_property_expands_identically() {
        assert_both_paths_agree(json!({
            "id": "urn:ngsi-ld:Device:jp1",
            "type": "Device",
            "config": { "type": "JsonProperty", "json": { "k1": 1, "k2": [true, false] } }
        }))
        .await;
    }

    #[tokio::test]
    async fn a_scope_expands_identically_as_one_path_and_as_several() {
        assert_both_paths_agree(json!({ "id": "urn:ngsi-ld:Device:sc1", "type": "Device", "scope": "root/a" })).await;
        assert_both_paths_agree(json!({ "id": "urn:ngsi-ld:Device:sc2", "type": "Device", "scope": ["root/a", "root/b"] })).await;
    }

    #[tokio::test]
    async fn a_nested_sub_property_expands_identically() {
        assert_both_paths_agree(json!({
            "id": "urn:ngsi-ld:Device:nested1",
            "type": "Device",
            "temperature": { "type": "Property", "value": 21, "accuracy": { "type": "Property", "value": 0.5 } }
        }))
        .await;
    }

    #[tokio::test]
    async fn a_nested_sub_relationship_expands_identically() {
        assert_both_paths_agree(json!({
            "id": "urn:ngsi-ld:Device:nested2",
            "type": "Device",
            "belongsTo": {
                "type": "Relationship",
                "object": "urn:ngsi-ld:Site:1",
                "providedBy": { "type": "Relationship", "object": "urn:ngsi-ld:Org:1" }
            }
        }))
        .await;
    }

    #[tokio::test]
    async fn an_entity_whose_inline_context_diverges_falls_back_and_still_agrees() {
        assert_both_paths_agree(json!({
            "@context": { "@vocab": "https://example.com/vocab#" },
            "id": "urn:ngsi-ld:Device:8",
            "type": "Thing"
        }))
        .await;
    }

    #[tokio::test]
    async fn the_fastpath_accepts_every_attribute_type_it_claims_to_expand() {
        let documents = [
            json!({ "id": "urn:x:1", "type": "X", "a": { "type": "Property", "value": 1 } }),
            json!({ "id": "urn:x:2", "type": "X", "loc": { "type": "GeoProperty", "value": { "type": "Point", "coordinates": [1.0, 2.0] } } }),
            json!({ "id": "urn:x:3", "type": "X", "desc": { "type": "LanguageProperty", "languageMap": { "en": "hi" } } }),
            json!({ "id": "urn:x:4", "type": "X", "cat": { "type": "VocabProperty", "vocab": "sensor" } }),
            json!({ "id": "urn:x:5", "type": "X", "cfg": { "type": "JsonProperty", "json": { "k": 1 } } }),
            json!({ "id": "urn:x:6", "type": "X", "rel": { "type": "Relationship", "object": "urn:ngsi-ld:Site:1" } }),
        ];
        for document in documents {
            assert!(fastpath_only(document.clone()).await.is_some(), "the fastpath must accept {document}");
        }
    }

    #[tokio::test]
    async fn the_fastpath_refuses_the_two_list_attribute_types_it_does_not_model() {
        let documents = [
            json!({ "id": "urn:x:l1", "type": "X", "readings": { "type": "ListProperty", "valueList": [1, 2] } }),
            json!({ "id": "urn:x:l2", "type": "X", "parts": { "type": "ListRelationship", "objectList": ["urn:ngsi-ld:Part:1"] } }),
        ];
        for document in documents {
            assert!(fastpath_only(document.clone()).await.is_none(), "the fastpath must refuse {document}");
        }
    }

    #[tokio::test]
    async fn the_fastpath_refuses_what_it_cannot_prove_equivalent() {
        let refused = [
            json!({ "@context": { "@vocab": "https://example.com/" }, "id": "urn:x:6", "type": "X" }),
            json!({ "id": "urn:x:7", "type": "X", "weird": { "type": "SomethingElse", "value": 1 } }),
            json!({ "id": "urn:x:8", "type": "X", "complex": { "type": "Property", "value": { "nested": true } } }),
        ];
        for document in refused {
            assert!(fastpath_only(document.clone()).await.is_none(), "the fastpath must refuse {document}");
        }
    }

    #[tokio::test]
    async fn the_fused_walk_wraps_a_concise_value_exactly_as_the_normalizer_would() {
        let engine = offline_engine();
        let handle = core_handle(&engine).await;
        let concise = json!({ "id": "urn:ngsi-ld:Device:fused1", "type": "Device", "speed": 42 });

        let mut normalized = concise.clone(); // clone: the concise form drives the fastpath below
        normalize_entity_attributes(&mut normalized);
        let slow = engine.expand(normalized, handle.spec()).await.expect("the processor expands");
        let fast = engine.expand_with_handle(concise, &handle).await.expect("the fastpath expands");

        pretty_assertions::assert_eq!(fast, slow);
    }

    #[tokio::test]
    async fn the_fused_walk_leaves_the_entity_members_the_normalizer_leaves_alone() {
        let engine = offline_engine();
        let handle = core_handle(&engine).await;
        let concise = json!({
            "id": "urn:ngsi-ld:Device:fused4",
            "type": "Device",
            "expiresAt": "2030-01-01T00:00:00Z",
            "scope": "root/a",
            "speed": 42
        });

        let mut normalized = concise.clone(); // clone: the concise form drives the fastpath below
        normalize_entity_attributes(&mut normalized);
        let entity = normalized.as_object().expect("an entity object");
        for member in ["id", "type", "scope", "expiresAt"] {
            assert!(
                entity.get(member).expect("member present").is_string(),
                "{member} is an entity member, not an attribute, and must not be wrapped"
            );
        }

        let slow = engine.expand(normalized, handle.spec()).await.expect("the processor expands");
        let fast = engine.expand_with_handle(concise, &handle).await.expect("the fastpath expands");

        pretty_assertions::assert_eq!(fast, slow);
    }

    #[tokio::test]
    async fn a_concise_array_attribute_is_normalized_per_instance_on_both_paths() {
        assert_both_paths_agree(json!({
            "id": "urn:ngsi-ld:Device:fused3",
            "type": "Device",
            "temperature": [
                { "type": "Property", "value": 21, "datasetId": "urn:ngsi-ld:Dataset:a" },
                { "type": "Property", "value": 22, "datasetId": "urn:ngsi-ld:Dataset:b" }
            ]
        }))
        .await;
    }

    #[tokio::test]
    async fn a_property_with_a_sub_relationship_expands_identically() {
        assert_both_paths_agree(json!({
            "id": "urn:ngsi-ld:Device:fused2",
            "type": "Device",
            "temperature": {
                "type": "Property",
                "value": 21,
                "providedBy": { "type": "Relationship", "object": "urn:ngsi-ld:Sensor:s1" }
            }
        }))
        .await;
    }

    #[tokio::test]
    async fn an_object_type_metadata_member_is_expanded_via_vocab_by_the_fastpath() {
        // ETSI GS CIM 009 v1.9.1: the core context declares `objectType` as
        // `@type: @vocab`, so its string value is a term expanded against the
        // active `@vocab`, not a plain literal. The old hand-written table had
        // no `@vocab` variant and stored `{"@value": "Building"}`, corrupting
        // the row. The fastpath must now emit the IRI the processor does.
        let document = json!({
            "id": "urn:ngsi-ld:Device:ot1",
            "type": "Device",
            "category": { "type": "Property", "value": "x", "objectType": "Building" }
        });
        let engine = offline_engine();
        let handle = core_handle(&engine).await;
        let fast = engine.try_fast_expand(&document, &handle).expect("the fastpath expands objectType via vocab");

        // `category` is not a core term, so it expands against `@vocab`; the
        // `objectType` sub-member lives on that attribute's Property instance.
        let category = &fast[0]["https://uri.etsi.org/ngsi-ld/default-context/category"][0];
        let object_type = category["https://uri.etsi.org/ngsi-ld/hasObjectType"][0].clone();
        pretty_assertions::assert_eq!(
            object_type,
            json!({ "@id": "https://uri.etsi.org/ngsi-ld/default-context/Building" }),
            "a @vocab term expands its value against @vocab as an @id, never a bare @value"
        );

        let slow = engine.expand(document, handle.spec()).await.expect("the processor expands");
        pretty_assertions::assert_eq!(fast, slow);
    }

    #[tokio::test]
    async fn an_end_time_at_metadata_member_is_coerced_to_datetime_by_the_fastpath() {
        // `endTimeAt` is one of the DateTime terms the old hand-written table
        // had forgotten; the derived table now carries it.
        assert_fastpath_expands_and_agrees(json!({
            "id": "urn:ngsi-ld:Device:et1",
            "type": "Device",
            "temperature": { "type": "Property", "value": 21, "endTimeAt": "2030-01-01T00:00:00Z" }
        }))
        .await;
    }

    #[tokio::test]
    async fn a_core_term_carrying_a_scoped_context_is_refused_and_agrees_via_fallback() {
        // `ngsildproof` is the one core term with a scoped `@context`; the
        // fastpath cannot unfold it, so it must refuse rather than expand the
        // sub-members against the batch table.
        let document = json!({
            "id": "urn:ngsi-ld:Device:np1",
            "type": "Device",
            "ngsildproof": { "type": "Property", "value": "z" }
        });
        assert!(
            fastpath_only(document.clone()).await.is_none(),
            "a scoped-context term must refuse the fastpath"
        );
        assert_both_paths_agree(document).await;
    }

    #[tokio::test]
    async fn a_typeless_concise_object_is_refused_so_the_normalizer_infers_its_type() {
        // A concise attribute whose type clause 4.5 infers from a
        // distinguishing member (`object`) carries no `type` for the fastpath's
        // fused walk, which refuses it. The batch path then demotes it to the
        // slow path, where `normalize_entity_attributes` infers Relationship —
        // so the fastpath never mis-expands an inferable shape as a Property.
        let documents = [
            json!({ "id": "urn:x:r1", "type": "X", "rel": { "object": "urn:ngsi-ld:Site:1" } }),
            json!({ "id": "urn:x:l1", "type": "X", "label": { "languageMap": { "en": "hi" } } }),
            json!({ "id": "urn:x:g1", "type": "X", "loc": { "type": "Point", "coordinates": [1.0, 2.0] } }),
        ];
        for document in documents {
            assert!(
                fastpath_only(document.clone()).await.is_none(),
                "the fastpath must refuse the typeless concise shape {document}"
            );
        }
    }

    #[tokio::test]
    async fn an_inline_core_context_of_the_wrong_version_refuses_the_fastpath() {
        // The deployment resolves versionless references to v1.9, so an entity
        // that inlines the v1.6 core context must not be expanded against the
        // v1.9 table; it falls to the processor.
        let document = json!({
            "@context": "https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.6.jsonld",
            "id": "urn:ngsi-ld:Device:v16",
            "type": "Device",
            "temperature": { "type": "Property", "value": 21 }
        });
        assert!(
            fastpath_only(document).await.is_none(),
            "an inline core context of a different release than the deployment pinned must refuse the fastpath"
        );
    }
}
