//! A request body written in the shape compaction gives it under the request's
//! `@context` is read as the same body written without the containers of the
//! terms (clause 5.5.4, clause 5.5.7, JSON-LD 1.1 section 4.6).

#![expect(
    clippy::expect_used,
    reason = "clippy reaches only `#[test]` functions in a member crate's integration test, and these helpers fail the test by panicking"
)]
use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
use andromeda_ngsi_ld_model::{
    attribute::{instance::AttributeValue, written_attribute::WrittenAttribute},
    entity::{Entity, EntityPatch},
    request_body::RequestBody,
    subscription_request::SubscriptionRequest,
    temporal::written_temporal_entity::WrittenTemporalEntity,
};
use andromeda_ngsi_ld_representation::{
    container_terms::{ContainerTerms, TermContainer},
    context_terms::ContextTerms,
    core_term_aliases::CoreTermAliases,
};
use serde::Serialize;
use serde_json::{Value, json};

/// The terms of a context that aliases `type` as `tps`, `value` as `val` and
/// defines `tps` with `@container: @set`, `speed` and `brand` with an `@index`
/// container, `tags` with `@set` while it reaches no core term, and `n` as an
/// alias of `@nest`.
fn terms() -> ContextTerms {
    let mut aliases = CoreTermAliases::keyword_spellings().clone(); // clone: the fixture extends the shared table
    aliases.alias("tps", "type");
    aliases.alias("val", "value");
    let mut containers = ContainerTerms::none();
    containers.contain("tps", TermContainer::Set);
    containers.contain("speed", TermContainer::Map);
    containers.contain("brand", TermContainer::Map);
    containers.contain("tags", TermContainer::Set);
    containers.contain("n", TermContainer::Nest);
    ContextTerms::new(aliases, containers)
}

/// `T` read from `text` under the terms of [`terms`].
fn read<T: RequestBody>(text: &Value) -> T {
    T::from_request_text(text.to_string().as_bytes(), terms().reading()).expect("the body reads under the context")
}

/// `T` read from `text` under the keyword spellings alone.
fn read_plain<T: RequestBody>(text: &Value) -> T {
    T::from_request_text(text.to_string().as_bytes(), ContextTerms::keyword_spellings().reading()).expect("the plain body reads")
}

/// `value` as the JSON it is written as.
fn written<T: Serialize>(value: &T) -> Value {
    serde_json::to_value(value).expect("the resource is written")
}

#[test]
fn an_entity_with_a_set_alias_of_type_reads_as_the_entity_without_the_container() {
    let contained = json!({"id": "urn:ngsi-ld:Vehicle:1", "type": "Vehicle", "height": {"tps": ["Property"], "value": 5}});
    let plain = json!({"id": "urn:ngsi-ld:Vehicle:1", "type": "Vehicle", "height": {"type": "Property", "value": 5}});
    pretty_assertions::assert_eq!(written(&read::<Entity>(&contained)), written(&read_plain::<Entity>(&plain)));
}

#[test]
fn an_entity_with_an_index_map_attribute_reads_as_the_entity_without_the_container() {
    let contained = json!({
        "id": "urn:ngsi-ld:Vehicle:1",
        "type": "Vehicle",
        "speed": {"@none": {"type": "Property", "val": 5}},
    });
    let plain = json!({"id": "urn:ngsi-ld:Vehicle:1", "type": "Vehicle", "speed": {"type": "Property", "value": 5}});
    pretty_assertions::assert_eq!(written(&read::<Entity>(&contained)), written(&read_plain::<Entity>(&plain)));
}

#[test]
fn an_entity_with_members_under_a_nest_alias_reads_as_the_entity_with_them_beside_it() {
    let contained = json!({
        "id": "urn:ngsi-ld:Vehicle:1",
        "tps": ["Vehicle"],
        "n": {"speed": {"@none": {"type": "Property", "value": 5}}, "n": {"brand": {"a": {"type": "Property", "value": "x"}}}},
    });
    let plain = json!({
        "id": "urn:ngsi-ld:Vehicle:1",
        "type": "Vehicle",
        "speed": {"type": "Property", "value": 5},
        "brand": {"type": "Property", "value": "x"},
    });
    pretty_assertions::assert_eq!(written(&read::<Entity>(&contained)), written(&read_plain::<Entity>(&plain)));
}

#[test]
fn an_entity_with_a_map_of_several_keys_reads_as_the_entity_with_the_instances_the_map_holds() {
    let contained = json!({
        "id": "urn:ngsi-ld:Vehicle:1",
        "type": "Vehicle",
        "speed": {
            "a": {"type": "Property", "value": 5, "datasetId": "urn:ngsi-ld:Dataset:a"},
            "b": [{"type": "Property", "value": 6, "datasetId": "urn:ngsi-ld:Dataset:b"}, {"type": "Property", "value": 7, "datasetId": "urn:ngsi-ld:Dataset:c"}],
        },
    });
    let plain = json!({
        "id": "urn:ngsi-ld:Vehicle:1",
        "type": "Vehicle",
        "speed": [
            {"type": "Property", "value": 5, "datasetId": "urn:ngsi-ld:Dataset:a"},
            {"type": "Property", "value": 6, "datasetId": "urn:ngsi-ld:Dataset:b"},
            {"type": "Property", "value": 7, "datasetId": "urn:ngsi-ld:Dataset:c"},
        ],
    });
    pretty_assertions::assert_eq!(written(&read::<Entity>(&contained)), written(&read_plain::<Entity>(&plain)));
}

#[test]
fn a_set_term_that_reaches_no_core_term_is_read_as_its_one_element() {
    let contained = json!({"id": "urn:ngsi-ld:Vehicle:1", "type": "Vehicle", "tags": [{"type": "Property", "value": "fast"}]});
    let plain = json!({"id": "urn:ngsi-ld:Vehicle:1", "type": "Vehicle", "tags": {"type": "Property", "value": "fast"}});
    pretty_assertions::assert_eq!(written(&read::<Entity>(&contained)), written(&read_plain::<Entity>(&plain)));
}

#[test]
fn an_entity_patch_reads_through_the_containers_of_its_context() {
    let contained = json!({"tps": ["Vehicle"], "n": {"speed": {"@none": {"type": "Property", "value": 5}}}});
    let plain = json!({"type": "Vehicle", "speed": {"type": "Property", "value": 5}});
    pretty_assertions::assert_eq!(written(&read::<EntityPatch>(&contained)), written(&read_plain::<EntityPatch>(&plain)));
}

#[test]
fn an_attribute_fragment_reads_through_the_containers_of_its_context() {
    let name: AttributeName = "speed".parse().expect("an attribute name");
    let contained = json!({"tps": ["Property"], "n": {"val": 5, "unitCode": "KMH"}});
    let plain = json!({"type": "Property", "value": 5, "unitCode": "KMH"});
    let from_contained = read::<WrittenAttribute>(&contained)
        .read::<AttributeValue>(&name, terms().reading())
        .expect("the fragment reads under the context");
    let from_plain = read_plain::<WrittenAttribute>(&plain)
        .read::<AttributeValue>(&name, ContextTerms::keyword_spellings().reading())
        .expect("the plain fragment reads");
    pretty_assertions::assert_eq!(format!("{from_contained:?}"), format!("{from_plain:?}"));
}

#[test]
fn a_subscription_reads_through_the_containers_of_its_context() {
    let contained = json!({
        "tps": ["Subscription"],
        "n": {"entities": [{"type": "Vehicle"}], "notification": {"endpoint": {"uri": "http://receiver.example.org/notify"}}},
    });
    let plain = json!({
        "type": "Subscription",
        "entities": [{"type": "Vehicle"}],
        "notification": {"endpoint": {"uri": "http://receiver.example.org/notify"}},
    });
    pretty_assertions::assert_eq!(
        written(&read::<SubscriptionRequest>(&contained)),
        written(&read_plain::<SubscriptionRequest>(&plain))
    );
}

#[test]
fn a_temporal_evolution_reads_through_the_containers_of_its_context() {
    let contained = json!({
        "id": "urn:ngsi-ld:Vehicle:1",
        "tps": ["Vehicle"],
        "n": {"speed": [{"type": "Property", "val": 5, "observedAt": "2026-01-01T00:00:00Z"}]},
    });
    let plain = json!({
        "id": "urn:ngsi-ld:Vehicle:1",
        "type": "Vehicle",
        "speed": [{"type": "Property", "value": 5, "observedAt": "2026-01-01T00:00:00Z"}],
    });
    pretty_assertions::assert_eq!(
        written(&read::<WrittenTemporalEntity>(&contained)),
        written(&read_plain::<WrittenTemporalEntity>(&plain))
    );
}

#[test]
fn a_body_without_the_containers_of_its_context_reads_as_before() {
    let body = json!({"id": "urn:ngsi-ld:Vehicle:1", "type": "Vehicle", "height": {"type": "Property", "value": 5}});
    pretty_assertions::assert_eq!(written(&read::<Entity>(&body)), written(&read_plain::<Entity>(&body)));
}
