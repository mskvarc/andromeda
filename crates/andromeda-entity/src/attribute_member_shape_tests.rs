//! What a client's request makes of every attribute member shape: the body is
//! decoded under the aliases of its admitted context and admitted as the write
//! names, a creation (clause 5.6.1) and a merge (clause 5.6.17) alike.

use crate::{
    admission_fixture::admitted_context,
    notification_recording_harness::{ENTITY_ID, recording_service},
    write_kind::{CreateEntity, MergeEntity},
};
use andromeda_json_literal::JsonLiteral;
use andromeda_jsonld::context_spec::ContextSpec;
use andromeda_ngsi_ld_model::{
    attribute::merge_member::MergeMember,
    entity::{Entity, EntityPatch},
    request_body::RequestBody,
};

/// What a route makes of a member written in one shape.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Verdict {
    /// The member is read into the typed model.
    Typed,
    /// The member is refused with `BadRequestData` (clause 5.5.4).
    Refused,
    /// The member is read, the member it names left unread: clause 5.2.2 has
    /// an implementation ignore a system member a Context Producer supplies.
    Ignored(&'static str),
}

/// The user `@context` aliasing `@type`, `@id` and `ngsi-ld:hasValue`.
const ALIASING_CONTEXT: &str = r#"{"kind": "@type", "ident": "@id", "v": "https://uri.etsi.org/ngsi-ld/hasValue"}"#;

/// The context a member is written under: the user `@context` `terms`, or the
/// Core `@context` alone.
fn context_of(terms: Option<&str>) -> ContextSpec {
    terms.map_or_else(ContextSpec::core_only, |terms| {
        ContextSpec::from_body(JsonLiteral::admit(terms).expect("the aliasing context is JSON"))
    })
}

/// The written payload of a creation of the Building carrying `member`,
/// decoded and admitted, or the refusal of it.
async fn created(member: &str, terms: Option<&str>) -> Result<String, String> {
    let (service, _recorded) = recording_service().await;
    let context = admitted_context(&service, &context_of(terms)).await;
    let text = format!(r#"{{"id": "{ENTITY_ID}", "type": "Building", "attr": {member}}}"#);
    let entity = <Entity>::from_request_text(text.as_bytes(), context.terms().reading()).map_err(|error| error.to_string())?;
    service
        .admit_entity_write::<CreateEntity>(&entity, &context)
        .await
        .map_err(|error| error.to_string())?;
    serde_json::to_string(&entity).map_err(|error| error.to_string())
}

/// The written payload of a merge of the fragment carrying `member`, decoded
/// and admitted, or the refusal of it.
async fn merged(member: &str, terms: Option<&str>) -> Result<String, String> {
    let (service, _recorded) = recording_service().await;
    let context = admitted_context(&service, &context_of(terms)).await;
    let text = format!(r#"{{"attr": {member}}}"#);
    let patch = <EntityPatch<MergeMember>>::from_request_text(text.as_bytes(), context.terms().reading()).map_err(|error| error.to_string())?;
    service
        .admit_entity_write::<MergeEntity>(&patch, &context)
        .await
        .map_err(|error| error.to_string())?;
    serde_json::to_string(&patch).map_err(|error| error.to_string())
}

/// Assert that `outcome`, the reading of a member by `route`, is `verdict`.
fn assert_outcome(route: &str, verdict: Verdict, outcome: &Result<String, String>) {
    match verdict {
        Verdict::Typed => assert!(outcome.is_ok(), "{route}: {outcome:?}"),
        Verdict::Refused => assert!(outcome.is_err(), "{route}: {outcome:?}"),
        Verdict::Ignored(member) => match outcome {
            Ok(written) => assert!(!written.contains(member), "{route} keeps {member}: {written}"),
            Err(refusal) => panic!("{route}: {refusal}"),
        },
    }
}

/// One test per shape, named after it: `member` written under the optional
/// user `@context`, with what a creation and then a merge make of it.
macro_rules! shape_tests {
    ($($(#[$doc:meta])* $test:ident: $member:literal $(under $context:ident)? => $request:ident $(($request_member:literal))?, $merge:ident $(($merge_member:literal))?),+ $(,)?) => {
        $(
            $(#[$doc])*
            #[tokio::test]
            async fn $test() {
                let terms: Option<&str> = [$($context)?].into_iter().next();
                assert_outcome("a creation", Verdict::$request $(($request_member))?, &created($member, terms).await);
                assert_outcome("a merge", Verdict::$merge $(($merge_member))?, &merged($member, terms).await);
            }
        )+
    };
}

shape_tests! {
    /// Clause 4.5.2.3: a bare scalar is a concise Property.
    a_bare_scalar: "5" => Typed, Typed,
    /// Clause 4.5.2.3: an array of values is a concise Property.
    an_array_of_values: "[1, 2, 3]" => Typed, Typed,
    /// Clause 4.5.2.3: the empty array is a concise Property too.
    an_empty_array: "[]" => Typed, Typed,
    /// Clause 4.5.2.3: an object naming no type is a concise Property.
    a_typeless_object: r#"{"street": "Main", "no": 1}"# => Typed, Typed,
    /// Clause 4.5.2.3: a typeless instance carrying `value`.
    a_typeless_instance_with_a_value: r#"{"value": 21, "unitCode": "CEL"}"# => Typed, Typed,
    /// Clause 4.7.3: a typeless instance whose value is a geometry.
    a_typeless_instance_with_a_geometry_value: r#"{"value": {"type": "Point", "coordinates": [13.4, 52.5]}}"# => Typed, Typed,
    /// Clause 4.5.3.3: a typeless instance carrying `object`.
    a_typeless_instance_with_an_object: r#"{"object": "urn:ngsi-ld:Owner:1"}"# => Typed, Typed,
    /// Clause 4.5.18.3: a typeless instance carrying `languageMap`.
    a_typeless_instance_with_a_language_map: r#"{"languageMap": {"en": "Tower", "de": "Turm"}}"# => Typed, Typed,
    /// Clause 4.5.20.3: a typeless instance carrying `vocab`.
    a_typeless_instance_with_a_vocab: r#"{"vocab": "Tower"}"# => Typed, Typed,
    /// Clause 4.5.21.3: a typeless instance carrying `valueList`.
    a_typeless_instance_with_a_value_list: r#"{"valueList": [1, 2]}"# => Typed, Typed,
    /// Clause 4.5.22.3: a typeless instance carrying `objectList`.
    a_typeless_instance_with_an_object_list: r#"{"objectList": ["urn:ngsi-ld:Owner:1"]}"# => Typed, Typed,
    /// Clause 4.5.24.3: a typeless instance carrying `json`.
    a_typeless_instance_with_a_json_value: r#"{"json": {"a": [1, {"b": null}]}}"# => Typed, Typed,
    /// Clause 4.7.3: a bare geometry is a concise `GeoProperty`.
    a_bare_geometry: r#"{"type": "Point", "coordinates": [13.4, 52.5]}"# => Typed, Typed,
    /// Clause 4.5.5: a multi-attribute one of whose instances is concise.
    an_array_with_a_concise_instance: r#"[{"value": 1, "datasetId": "urn:ngsi-ld:Dataset:a"}, {"type": "Property", "value": 2}]"# => Typed, Typed,
    /// Clause 4.7.2: a geometry may be written as the JSON string encoding it.
    a_geometry_written_as_a_string: r#"{"type": "GeoProperty", "value": "{\"type\":\"Point\",\"coordinates\":[1,2]}"}"# => Typed, Typed,
    /// Clause 4.7.2: a string that encodes no geometry is no `GeoProperty` value.
    a_string_encoding_no_geometry: r#"{"type": "GeoProperty", "value": "not a geometry"}"# => Refused, Refused,
    /// Clause 5.5.4 refuses the NGSI-LD Null outside a merge, which removes the
    /// member (clause 5.5.12), and a notification reports a deleted attribute so
    /// (clause 5.8.6).
    a_bare_ngsi_ld_null: r#""urn:ngsi-ld:null""# => Refused, Typed,
    /// Clause 4.5.0 uses the NGSI-LD Null as the value of a deleted `GeoProperty`
    /// in a notification and a temporal representation; clause 5.5.4 refuses it
    /// elsewhere outside a merge.
    a_geoproperty_whose_value_is_the_null: r#"{"type": "GeoProperty", "value": "urn:ngsi-ld:null"}"# => Refused, Typed,
    /// Clause 5.5.12: a merge removes the `unitCode` written as the NGSI-LD Null.
    a_null_unit_code: r#"{"type": "Property", "value": 1, "unitCode": "urn:ngsi-ld:null"}"# => Refused, Typed,
    /// Clause 5.5.12: a merge removes the `observedAt` written as the NGSI-LD Null.
    a_null_observed_at: r#"{"type": "Property", "value": 1, "observedAt": "urn:ngsi-ld:null"}"# => Refused, Typed,
    /// Clause 5.5.12, last bullet: a `datasetId` is never removed by a merge.
    a_null_dataset_id: r#"{"type": "Property", "value": 1, "datasetId": "urn:ngsi-ld:null"}"# => Refused, Refused,
    /// Clauses 4.5.0 and 5.2.1: no member admits JSON `null`.
    a_json_null: "null" => Refused, Refused,
    /// Clause 4.5.2.2: a type naming no attribute type.
    an_unknown_type: r#"{"type": "Propertyy", "value": 1}"# => Refused, Refused,
    /// Clause 4.7.3: a geometry type without its coordinates.
    a_geometry_without_coordinates: r#"{"type": "Point"}"# => Refused, Refused,
    /// Clause 4.5.2.2 requires the value member; clause 5.5.12 lets a merge keep
    /// the value held.
    an_instance_without_its_value: r#"{"type": "Property"}"# => Refused, Typed,
    /// Clause 4.5.3.2: an `object` is a URI.
    an_undecodable_family_value: r#"{"type": "Relationship", "object": "not a uri"}"# => Refused, Refused,
    /// Clause 4.8: an `observedAt` is a `DateTime`.
    an_undecodable_envelope_member: r#"{"type": "Property", "value": 1, "observedAt": "yesterday"}"# => Refused, Refused,
    /// Clause 5.2.2: a producer's system member is ignored; a peer's own system
    /// members are `DateTime`s (clause 4.8).
    a_malformed_system_member: r#"{"type": "Property", "value": 1, "createdAt": "not a time"}"# => Ignored("createdAt"), Ignored("createdAt"),
    /// Clause 4.6.2: a sub-attribute is named by an attribute name.
    a_bad_sub_attribute_name: r#"{"type": "Property", "value": 1, "bad name": {"type": "Property", "value": 2}}"# => Refused, Refused,
    /// Clause 4.5.5: a multi-attribute is an array of instances.
    a_mixed_array: r#"[{"type": "Property", "value": 1}, 1]"# => Refused, Refused,
    /// Clause 4.5.2.3: a concise instance infers one family.
    several_family_value_members: r#"{"object": "urn:ngsi-ld:Owner:1", "vocab": "Tower"}"# => Refused, Refused,
    /// Clause 5.5.7: no JSON-LD keyword inside a geometry.
    a_geometry_with_a_json_ld_keyword: r#"{"type": "GeoProperty", "value": {"@id": "urn:ngsi-ld:Geometry:1", "type": "Point", "coordinates": [13.4, 52.5]}}"# => Refused, Refused,
    /// Clause 4.4: `@type` stands for `type`.
    an_instance_spelled_with_keywords: r#"{"@type": "Property", "value": 1}"# => Typed, Typed,
    /// Clause 5.5.7: a user `@context` may alias `@type` and `ngsi-ld:hasValue`.
    an_instance_spelled_with_user_aliases: r#"{"kind": "Property", "v": 1}"# under ALIASING_CONTEXT => Typed, Typed,
}
