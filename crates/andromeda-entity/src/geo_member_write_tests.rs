//! The geospatial members a write holds only as a `GeoProperty` (Table
//! 5.2.4-1), as a request reaches the rule through the entity service:
//! however the member is spelled, and in a simplified merge only once the
//! fragment takes the types the target holds.

use crate::{
    EntityService,
    admission_fixture::{admitted_context, admitted_written},
    change_notifier::{ChangeNotifier, SubscriptionMatching},
    entity_validation_reason::EntityValidationReason,
    error::EntityError,
    in_memory_service::in_memory_service,
    in_memory_store::InMemoryStore,
    merge_options::MergeOptions,
    write_kind::{CreateEntity, MergeEntity},
    write_target::WriteTarget,
};
use andromeda_context_commit::committer::ContextCommitter;
use andromeda_context_space::context_target::ContextTarget;
use andromeda_json_literal::JsonLiteral;
use andromeda_jsonld::context_spec::ContextSpec;
use andromeda_ngsi_ld_entity::{admission::written_families::WrittenFamilies, refusal::EntityRefusal};
use andromeda_ngsi_ld_lexicon::{attribute_type::AttributeType, identifier::Identifier};
use andromeda_ngsi_ld_model::{
    attribute::{instance::AttributeValue, merge_member::MergeMember},
    entity::{Entity, EntityPatch},
};
use andromeda_ngsi_ld_representation::merge_representation::MergeRepresentation;
use andromeda_ngsi_ld_vocabulary::expanded::properties;
use serde_json::{Value, json};
use std::sync::Arc;

/// The Vehicle every case writes.
const VEHICLE_ID: &str = "urn:ngsi-ld:Vehicle:geo-1";

/// The Vehicle's identifier.
fn vehicle_id() -> Identifier {
    VEHICLE_ID.parse().expect("an identifier")
}

/// An entity service over an empty in-memory store, and the store.
fn service_and_store() -> (EntityService, Arc<InMemoryStore>) {
    let store = Arc::new(InMemoryStore::default());
    let service = in_memory_service(
        &store,
        Arc::clone(&store) as Arc<dyn ContextCommitter>,
        ChangeNotifier::new(None, SubscriptionMatching::Absent),
    );
    (service, store)
}

/// The refusal a creation of the Vehicle carrying `member` as `node` under
/// `context` gets.
async fn refused_creation(member: &str, node: &Value, context: &ContextSpec) -> EntityError {
    let (service, _store) = service_and_store();
    let entity: Entity<AttributeValue> = serde_json::from_value(json!({"id": VEHICLE_ID, "type": "Vehicle", member: node})).expect("the payload decodes");
    service
        .admit_entity_write::<CreateEntity>(&entity, &admitted_context(&service, context).await, WrittenFamilies::Final)
        .await
        .expect_err("the creation is refused")
}

/// Assert `error` refuses `location` as a Property.
fn assert_location_as_a_property(error: &EntityError) {
    let EntityError::Validation(reason) = error else {
        panic!("not a validation refusal: {error:?}");
    };
    let refusal = reason.to_string();
    assert!(refusal.contains("'location' must be a GeoProperty, not a Property"), "{refusal}");
}

/// The user `@context` aliasing `loc` to the core `location`.
fn aliasing_context() -> ContextSpec {
    ContextSpec::from_body(JsonLiteral::admit(r#"{"loc": "https://uri.etsi.org/ngsi-ld/location"}"#).expect("the aliasing context is JSON"))
}

#[tokio::test]
async fn location_written_as_a_property_under_its_core_term_is_refused() {
    let error = refused_creation("location", &json!({"type": "Property", "value": "somewhere"}), &ContextSpec::core_only()).await;
    assert_location_as_a_property(&error);
}

#[tokio::test]
async fn location_written_as_a_concise_property_is_refused() {
    let error = refused_creation("location", &json!("somewhere"), &ContextSpec::core_only()).await;
    assert_location_as_a_property(&error);
}

#[tokio::test]
async fn location_written_as_a_property_under_the_core_prefix_is_refused() {
    let error = refused_creation(
        "ngsi-ld:location",
        &json!({"type": "Property", "value": "somewhere"}),
        &ContextSpec::core_only(),
    )
    .await;
    assert_location_as_a_property(&error);
}

#[tokio::test]
async fn location_written_as_a_property_under_its_iri_is_refused() {
    let error = refused_creation(
        properties::LOCATION.as_str(),
        &json!({"type": "Property", "value": "somewhere"}),
        &ContextSpec::core_only(),
    )
    .await;
    assert_location_as_a_property(&error);
}

#[tokio::test]
async fn location_written_as_a_property_under_a_user_term_aliasing_it_is_refused() {
    let error = refused_creation("loc", &json!({"type": "Property", "value": "somewhere"}), &aliasing_context()).await;
    assert_location_as_a_property(&error);
}

#[tokio::test]
async fn location_written_as_a_geo_property_under_a_user_term_aliasing_it_is_admitted() {
    let (service, _store) = service_and_store();
    let entity: Entity<AttributeValue> = serde_json::from_value(json!({
        "id": VEHICLE_ID,
        "type": "Vehicle",
        "loc": {"type": "GeoProperty", "value": {"type": "Point", "coordinates": [13.4, 52.5]}}
    }))
    .expect("the payload decodes");
    service
        .admit_entity_write::<CreateEntity>(&entity, &admitted_context(&service, &aliasing_context()).await, WrittenFamilies::Final)
        .await
        .expect("a GeoProperty");
}

/// Merge `fragment` into the stored Vehicle located at (13.4, 52.5), in the
/// simplified representation, admitting it first as the transport does before
/// it forwards anything; the stored Vehicle node or the refusal.
async fn simplified_merge_into_located_vehicle(fragment: &Value, located: bool) -> Result<Value, EntityError> {
    let (service, store) = service_and_store();
    let mut vehicle = json!({"id": VEHICLE_ID, "type": "Vehicle", "speed": {"type": "Property", "value": 42}});
    if located {
        vehicle["location"] = json!({"type": "GeoProperty", "value": {"type": "Point", "coordinates": [13.4, 52.5]}});
    }
    let created = admitted_written::<CreateEntity>(&service, vehicle, &ContextSpec::core_only()).await;
    service.create(&ContextTarget::Live, &vehicle_id(), created).await.expect("the create lands");
    let patch: EntityPatch<MergeMember> = serde_json::from_value(fragment.clone()).expect("a merge fragment");
    let context = admitted_context(&service, &ContextSpec::core_only()).await;
    let admitted = service
        .admit_entity_write::<MergeEntity>(&patch, &context, WrittenFamilies::from(MergeRepresentation::Simplified))
        .await?;
    let options = MergeOptions {
        representation: MergeRepresentation::Simplified,
        ..MergeOptions::default()
    };
    service
        .merge(&ContextTarget::Live, WriteTarget::new(&vehicle_id(), None), &patch, admitted, &context, options)
        .await?;
    let (state, _version) = store.stored(&vehicle_id()).expect("the Vehicle is stored");
    let encoded = serde_json::to_value(state.as_ref()).expect("the state encodes");
    Ok(encoded.get(0).cloned().unwrap_or(encoded))
}

/// Table 6.5.3.4-1 gives the bare value the `GeoProperty` type the target
/// holds, and clause 4.7.2 admits a geometry written as a JSON string as its
/// value.
#[tokio::test]
async fn a_simplified_merge_of_a_string_geometry_into_a_held_location_keeps_it_a_geo_property() {
    let encoded = json!({"type": "Point", "coordinates": [2.35, 48.85]}).to_string();
    let node = simplified_merge_into_located_vehicle(&json!({"location": encoded}), true)
        .await
        .expect("the merge lands");
    let location = &node[properties::LOCATION.as_str()][0];
    pretty_assertions::assert_eq!(location["@type"], json!([AttributeType::GeoProperty.iri().as_str()]), "{node}");
}

/// A target holding no `location` leaves the bare value the Property the
/// concise reading of clause 4.5.2.3 gives it, which Table 5.2.4-1 refuses.
#[tokio::test]
async fn a_simplified_merge_of_a_bare_text_into_an_unlocated_vehicle_is_refused() {
    let error = simplified_merge_into_located_vehicle(&json!({"location": "nowhere"}), false)
        .await
        .expect_err("a Property location");
    assert_location_as_a_property(&error);
}

#[tokio::test]
async fn a_normalized_merge_of_location_as_a_relationship_is_refused_before_anything_is_merged() {
    let (service, _store) = service_and_store();
    let patch: EntityPatch<MergeMember> =
        serde_json::from_value(json!({"location": {"type": "Relationship", "object": "urn:ngsi-ld:Place:1"}})).expect("a merge fragment");
    let error = service
        .admit_entity_write::<MergeEntity>(&patch, &admitted_context(&service, &ContextSpec::core_only()).await, WrittenFamilies::Final)
        .await
        .expect_err("a Relationship location");
    let EntityError::Validation(reason) = error else {
        panic!("not a validation refusal: {error:?}");
    };
    assert!(reason.to_string().contains("not a Relationship"), "{reason}");
}

#[tokio::test]
async fn the_refusal_is_the_models_typed_refusal_naming_the_family_written() {
    let error = refused_creation(
        "observationSpace",
        &json!({"type": "Relationship", "object": "urn:ngsi-ld:Place:1"}),
        &ContextSpec::core_only(),
    )
    .await;
    let EntityError::Validation(EntityValidationReason::Refused(refusal)) = error else {
        panic!("not the model's refusal: {error:?}");
    };
    let EntityRefusal::NotAGeoProperty { attribute, family } = *refusal else {
        panic!("not the geospatial member refusal: {refusal:?}");
    };
    pretty_assertions::assert_eq!(attribute.as_str(), properties::OBSERVATION_SPACE.as_str());
    pretty_assertions::assert_eq!(family, AttributeType::Relationship);
}
