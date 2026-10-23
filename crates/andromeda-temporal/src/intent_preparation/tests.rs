//! The intent the history consequences of one write prepare.

use crate::{
    append_intent::TemporalAppendEntry,
    error::TemporalError,
    intent_preparation::prepare,
    scope_entry_kind::ScopeEntryKind,
    scope_observation::ScopeObservation,
    temporal_validation_reason::TemporalValidationReason,
    write_purpose::TemporalWritePurpose,
};
use andromeda_context_commit::{
    history_record::{HistoryRecord, tombstone},
    write_id::WriteId,
    write_ordinal::WriteOrdinal,
    write_record::WriteRecord,
};
use andromeda_ngsi_ld_entity::{
    document::{EntityDocument, attribute_member::AttributeMember, instance_document::InstanceDocument},
    entity_state::EntityState,
    member_shape::MemberShape,
    refusal::EntityRefusal,
    scope_refusal::ScopeRefusal,
};
use andromeda_ngsi_ld_lexicon::{
    attribute_name::AttributeName,
    attribute_type::AttributeType,
    identifier::Identifier,
    instance_identity::InstanceIdentity,
    scope::path::ScopePath,
};
use andromeda_ngsi_ld_model::temporal::{TemporalHistoryRow, TemporalValue, evolution_instance::EvolutionInstance};
use andromeda_ngsi_ld_number::JsonNumber;
use andromeda_ngsi_ld_vocabulary::expanded::properties as expanded_properties;
use chrono::{DateTime, TimeZone, Utc};
use indexmap::IndexMap;
use serde_json::{Value, json};
use std::sync::Arc;

const PROPERTY: &str = "https://uri.etsi.org/ngsi-ld/Property";
const VEHICLE: &str = "https://example.com/Vehicle";

/// Build a restored record so tests can hand `prepare` a fixed identity.
fn record(recorded_at: DateTime<Utc>) -> WriteRecord {
    WriteRecord::restored(WriteId::mint(), recorded_at)
}

fn recorded_at() -> DateTime<Utc> {
    Utc.timestamp_opt(1_700_000_000, 0).single().expect("valid instant")
}

fn entity_id() -> Identifier {
    "urn:ngsi-ld:Vehicle:1".parse::<Identifier>().expect("valid entity identifier")
}

/// The stored entity `members` make, as the state a Core write observed.
fn observed(members: &Value) -> HistoryRecord {
    let mut node = json!({"@id": entity_id(), "@type": [VEHICLE]});
    if let (Some(node), Some(members)) = (node.as_object_mut(), members.as_object()) {
        node.extend(members.iter().map(|(key, value)| (key.clone(), value.clone()))); // clone: the fixture owns its members
    }
    let state = EntityState::from_slice(&serde_json::to_vec(&json!([node])).expect("JSON")).expect("a stored entity decodes");
    HistoryRecord::Observations(Arc::new(state))
}

/// The Temporal Evolution document `document` decodes as.
fn written(document: &Value) -> HistoryRecord {
    HistoryRecord::EvolutionInstances(Arc::new(
        EntityDocument::from_slice(&serde_json::to_vec(document).expect("JSON")).expect("a written document decodes"),
    ))
}

/// The entries of the intent `history` prepares.
fn prepared(history: &[HistoryRecord]) -> Vec<TemporalAppendEntry> {
    prepare(&entity_id(), &[], history, record(recorded_at()), WriteOrdinal::at(0))
        .expect("the history prepares")
        .expect("the history produces an intent")
        .entries
}

/// The observation rows among `entries`.
fn rows(entries: &[TemporalAppendEntry]) -> Vec<&TemporalHistoryRow> {
    entries
        .iter()
        .filter_map(|entry| match entry {
            TemporalAppendEntry::Observation { row } => Some(row),
            TemporalAppendEntry::Tombstone { .. } | TemporalAppendEntry::Scope { .. } => None,
        })
        .collect()
}

/// One reified scope instance: a Property whose `value` names `paths` and
/// whose `observedAt` dates them.
fn reified_scope(value: &Value, observed_at: &str) -> InstanceDocument {
    let scope: AttributeName = expanded_properties::SCOPE.as_str().parse().expect("valid attribute name");
    let instance = json!({
        "@type": [PROPERTY],
        expanded_properties::VALUE.as_str(): value,
        expanded_properties::OBSERVED_AT.as_str(): [{"@type": "https://uri.etsi.org/ngsi-ld/DateTime", "@value": observed_at}]
    });
    InstanceDocument::from_slice(&scope, &serde_json::to_vec(&instance).expect("JSON")).expect("a reified scope instance decodes")
}

/// Clause 4.5.6: the temporal scope is the reified Property whose `observedAt`
/// dates it; a non-reified scope written through the Core API is dated at the
/// write ("a copy of the modifiedAt"), and the NGSI-LD Null records the end of
/// the Scope (clause 4.18).
#[test]
fn scopes_are_prepared_with_their_observed_at_or_dated_at_the_write() {
    let scope: AttributeName = expanded_properties::SCOPE.as_str().parse().expect("valid attribute name");
    let reified = EntityDocument::new(
        None,
        Vec::new(),
        None,
        None,
        IndexMap::from([(
            scope,
            AttributeMember::Instances(vec![
                reified_scope(&json!([{"@value": "/Barcelona"}]), "2020-06-01T00:00:00Z"),
                reified_scope(&json!([{"@value": "urn:ngsi-ld:null"}]), "2020-06-01T00:00:00Z"),
            ]),
        )]),
    );
    let history = [
        observed(&json!({"https://uri.etsi.org/ngsi-ld/scope": [{"@value": "/Madrid"}]})),
        HistoryRecord::EvolutionInstances(Arc::new(reified)),
    ];
    let observed_at = Utc.with_ymd_and_hms(2020, 6, 1, 0, 0, 0).single().expect("valid instant");

    pretty_assertions::assert_eq!(
        scope_entries(&history),
        vec![
            (path("/Madrid"), ScopeEntryKind::Event, Some(recorded_at())),
            (path("/Barcelona"), ScopeEntryKind::Event, Some(observed_at)),
            (ScopeObservation::Deleted, ScopeEntryKind::Event, Some(observed_at)),
        ]
    );
}

/// What each scope entry of the intent `history` prepares records, of which
/// kind, and when it was observed.
fn scope_entries(history: &[HistoryRecord]) -> Vec<(ScopeObservation, ScopeEntryKind, Option<DateTime<Utc>>)> {
    prepared(history)
        .into_iter()
        .filter_map(|entry| match entry {
            TemporalAppendEntry::Scope {
                observation,
                kind,
                observed_at,
            } => Some((observation, kind, observed_at)),
            TemporalAppendEntry::Observation { .. } | TemporalAppendEntry::Tombstone { .. } => None,
        })
        .collect()
}

fn path(raw: &str) -> ScopeObservation {
    ScopeObservation::Path(raw.parse::<ScopePath>().expect("a valid scope path"))
}

/// The Temporal Evolution document holding the reified scope `instance`.
fn reified_document(instance: InstanceDocument) -> HistoryRecord {
    let scope: AttributeName = expanded_properties::SCOPE.as_str().parse().expect("valid attribute name");
    HistoryRecord::EvolutionInstances(Arc::new(EntityDocument::new(
        None,
        Vec::new(),
        None,
        None,
        IndexMap::from([(scope, AttributeMember::Instances(vec![instance]))]),
    )))
}

/// A non-reified `scope` has no instant of its own: it states the Entity is
/// in that Scope at the write, which the history judges.
#[test]
fn a_non_reified_temporal_scope_is_an_assertion_at_the_write() {
    let history = [written(&json!([{
        "@id": entity_id(),
        "@type": [VEHICLE],
        "https://uri.etsi.org/ngsi-ld/scope": [{"@value": "/Madrid"}, {"@value": "Madrid/Centro"}]
    }]))];

    pretty_assertions::assert_eq!(
        scope_entries(&history),
        vec![
            (path("/Madrid"), ScopeEntryKind::Assertion, Some(recorded_at())),
            (path("Madrid/Centro"), ScopeEntryKind::Assertion, Some(recorded_at())),
        ]
    );
}

/// Clauses 4.5.6 and 4.5.7: a reified scope instance carries its own
/// `observedAt`, so it is an event dated there.
#[test]
fn a_reified_temporal_scope_is_an_instance_at_its_observed_at() {
    let history = [reified_document(reified_scope(&json!([{"@value": "/Madrid"}]), "2026-03-01T00:00:00Z"))];
    let observed_at = Utc.with_ymd_and_hms(2026, 3, 1, 0, 0, 0).single().expect("valid instant");

    pretty_assertions::assert_eq!(scope_entries(&history), vec![(path("/Madrid"), ScopeEntryKind::Event, Some(observed_at))]);
}

#[test]
fn an_undated_reified_temporal_scope_is_an_instance_at_the_write() {
    let scope: AttributeName = expanded_properties::SCOPE.as_str().parse().expect("valid attribute name");
    let undated = json!({"@type": [PROPERTY], expanded_properties::VALUE.as_str(): [{"@value": "/Madrid"}]});
    let instance = InstanceDocument::from_slice(&scope, &serde_json::to_vec(&undated).expect("JSON")).expect("a reified scope instance decodes");

    pretty_assertions::assert_eq!(
        scope_entries(&[reified_document(instance)]),
        vec![(path("/Madrid"), ScopeEntryKind::Event, Some(recorded_at()))]
    );
}

/// Clause 4.18 reserves the NGSI-LD Null for a deleted Scope, so a Temporal
/// API write setting the non-reified `scope` to it removes the Scope, an event
/// whatever the history holds.
#[test]
fn a_non_reified_temporal_scope_set_to_the_ngsi_ld_null_is_a_removal() {
    let history = [written(&json!([{
        "@id": entity_id(),
        "@type": [VEHICLE],
        "https://uri.etsi.org/ngsi-ld/scope": [{"@value": "urn:ngsi-ld:null"}]
    }]))];

    pretty_assertions::assert_eq!(
        scope_entries(&history),
        vec![(ScopeObservation::Deleted, ScopeEntryKind::Event, Some(recorded_at()))]
    );
}

/// Clause 4.18: a Core write removing the Scope ends it with an event dated at
/// the write.
#[test]
fn a_core_scope_removal_is_an_event() {
    pretty_assertions::assert_eq!(
        scope_entries(&[HistoryRecord::ScopeRemoval]),
        vec![(ScopeObservation::Deleted, ScopeEntryKind::Event, Some(recorded_at()))]
    );
}

/// Clauses 4.5.7 and 5.6.11.4: a deletion a written Temporal Evolution states
/// is an instance of the evolution beside its observations, placed at its
/// `observedAt` and keeping its `instanceId`. Clause 4.8 has the broker
/// generate `deletedAt`, so it is the write's instant, whatever the client
/// wrote.
#[test]
fn a_written_deletion_is_an_instance_of_the_evolution_at_its_observed_at() {
    let entries = prepared(&[written(&json!([{
        "@id": entity_id(),
        "@type": [VEHICLE],
        "https://example.com/speed": [
            {
                "@type": [PROPERTY],
                "https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": 1}],
                "https://uri.etsi.org/ngsi-ld/observedAt": [{"@value": "2024-01-01T00:00:00Z"}]
            },
            {
                "@type": [PROPERTY],
                "https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": "urn:ngsi-ld:null"}],
                "https://uri.etsi.org/ngsi-ld/observedAt": [{"@value": "2024-01-02T00:00:00Z"}],
                "https://uri.etsi.org/ngsi-ld/deletedAt": [{"@value": "2024-01-02T00:00:00Z"}],
                "https://uri.etsi.org/ngsi-ld/instanceId": [{"@id": "urn:ngsi-ld:instance:deleted"}]
            }
        ]
    }]))]);

    let rows = rows(&entries);
    pretty_assertions::assert_eq!(rows.len(), 2, "{entries:?}");
    let deletion = rows[1];
    pretty_assertions::assert_eq!(deletion.value, TemporalValue::Tombstone { deleted_at: recorded_at() });
    pretty_assertions::assert_eq!(deletion.attribute_type, AttributeType::Property);
    pretty_assertions::assert_eq!(
        deletion.observed_at,
        Some(Utc.with_ymd_and_hms(2024, 1, 2, 0, 0, 0).single().expect("valid instant"))
    );
    pretty_assertions::assert_eq!((deletion.created_at, deletion.modified_at), (None, None));
    pretty_assertions::assert_eq!(deletion.metadata, None);
    pretty_assertions::assert_eq!(
        deletion.instance,
        EvolutionInstance::Attribute(InstanceIdentity::Verbatim("urn:ngsi-ld:instance:deleted".parse().expect("valid URI")))
    );
}

/// Clause 4.5.8 and clause 4.5.5: a written deletion keeps the family it
/// deletes and the `datasetId` of the instance it names.
#[test]
fn a_written_deletion_keeps_its_family_and_dataset() {
    let dataset_id = "urn:ngsi-ld:Dataset:a".parse::<Identifier>().expect("valid dataset identifier");
    let entries = prepared(&[written(&json!([{
        "@id": entity_id(),
        "https://example.com/location": [{
            "@type": ["https://uri.etsi.org/ngsi-ld/GeoProperty"],
            "https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": "urn:ngsi-ld:null"}],
            "https://uri.etsi.org/ngsi-ld/datasetId": [{"@id": dataset_id}]
        }]
    }]))]);

    let rows = rows(&entries);
    pretty_assertions::assert_eq!(rows.len(), 1, "{entries:?}");
    pretty_assertions::assert_eq!(rows[0].attribute_type, AttributeType::GeoProperty);
    pretty_assertions::assert_eq!(rows[0].dataset_id.as_ref(), Some(&dataset_id));
    pretty_assertions::assert_eq!(rows[0].observed_at, None);
}

/// Clause 5.6.11.4 creates the Temporal Evolution a client provides, so an
/// evolution stating nothing but a deletion extends the evolution rather than
/// recording nothing where none exists, which only the end of an instance a
/// Core write deleted does.
#[test]
fn an_evolution_stating_only_a_deletion_is_an_observation() {
    let history = [written(&json!([{
        "@id": entity_id(),
        "@type": [VEHICLE],
        "https://example.com/speed": [{
            "@type": [PROPERTY],
            "https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": "urn:ngsi-ld:null"}],
            "https://uri.etsi.org/ngsi-ld/deletedAt": [{"@value": "2024-01-02T00:00:00Z"}]
        }]
    }]))];
    let intent = prepare(&entity_id(), &[], &history, record(recorded_at()), WriteOrdinal::at(0))
        .expect("the history prepares")
        .expect("the history produces an intent");
    pretty_assertions::assert_eq!(intent.write_purpose(), TemporalWritePurpose::Observation);
}

/// The end of an instance a Core write deleted is recorded by its tombstone.
#[test]
fn a_core_deletion_is_recorded_by_its_tombstone() {
    let attribute_name = "https://example.com/location".parse::<AttributeName>().expect("valid attribute name");
    let entries = prepared(&[tombstone(&attribute_name, None, AttributeType::GeoProperty)]);

    assert!(
        matches!(
            entries.as_slice(),
            [TemporalAppendEntry::Tombstone {
                attribute_name: actual,
                dataset_id: None,
                attribute_type: AttributeType::GeoProperty,
                ..
            }] if actual == &attribute_name
        ),
        "{entries:?}"
    );
}

/// Clause 5.6.12: every instance a Temporal Evolution document adds is its own
/// observation, several per `datasetId` included, each keeping its instant and
/// the `instanceId` the client gave it.
#[test]
fn every_written_instance_is_its_own_observation() {
    let instance = |value: i64, observed_at: &str, instance_id: &str| {
        json!({
            "@type": [PROPERTY],
            "https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": value}],
            "https://uri.etsi.org/ngsi-ld/observedAt": [{"@value": observed_at}],
            "https://uri.etsi.org/ngsi-ld/datasetId": [{"@id": "urn:ngsi-ld:Dataset:sensor"}],
            "https://uri.etsi.org/ngsi-ld/instanceId": [{"@id": instance_id}]
        })
    };
    let entries = prepared(&[written(&json!([{
        "@id": entity_id(),
        "@type": [VEHICLE],
        "https://example.com/speed": [
            instance(55, "2020-08-01T12:00:00Z", "urn:ngsi-ld:instance:1"),
            instance(61, "2020-08-01T13:00:00Z", "urn:ngsi-ld:instance:2")
        ]
    }]))]);

    let rows = rows(&entries);
    pretty_assertions::assert_eq!(
        rows.iter().map(|row| row.value.clone()).collect::<Vec<_>>(), // clone: compared after the borrow
        vec![
            TemporalValue::Number {
                number: JsonNumber::from(55),
                datatype: None
            },
            TemporalValue::Number {
                number: JsonNumber::from(61),
                datatype: None
            }
        ]
    );
    assert!(
        rows.iter()
            .all(|row| row.dataset_id.as_ref().is_some_and(|dataset| dataset.as_str() == "urn:ngsi-ld:Dataset:sensor"))
    );
    assert!(rows.iter().all(|row| row.observed_at.is_some() && row.created_at == Some(recorded_at())));
    pretty_assertions::assert_eq!(
        rows.iter().map(|row| &row.instance).collect::<Vec<_>>(),
        ["urn:ngsi-ld:instance:1", "urn:ngsi-ld:instance:2"]
            .map(|id| EvolutionInstance::Attribute(InstanceIdentity::Verbatim(id.parse().expect("valid URI"))))
            .iter()
            .collect::<Vec<_>>()
    );
}

/// Table 5.2.5-2 makes `instanceId` output-only and clause 5.2.5 has the
/// broker ignore one a producer writes through the Core API, so the instance a
/// Core write adds to the Temporal Evolution is given a minted identity.
#[test]
fn a_core_write_records_a_minted_instance_id_whatever_the_producer_wrote() {
    let entries = prepared(&[observed(&json!({
        "https://example.com/speed": [{
            "@type": [PROPERTY],
            "https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": 55}],
            "https://uri.etsi.org/ngsi-ld/instanceId": [{"@id": "urn:ngsi-ld:Instance:client"}]
        }]
    }))]);

    let rows = rows(&entries);
    pretty_assertions::assert_eq!(rows.len(), 1);
    assert!(
        matches!(rows[0].instance, EvolutionInstance::Attribute(InstanceIdentity::Generated(_))),
        "{:?}",
        rows[0].instance
    );
}

/// The entity state a Property holding a `DateTime`-typed `raw` value
/// observed.
fn datetime_property(raw: &str) -> HistoryRecord {
    observed(&json!({
        "https://example.com/installedAt": [{
            "@type": [PROPERTY],
            "https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": raw, "@type": "https://uri.etsi.org/ngsi-ld/DateTime"}]
        }]
    }))
}

/// Clause 4.6.3 mandates the literal `Z` UTC designator and at most six
/// fractional-second digits; a `DateTime` Property value that breaks either is
/// refused, not reinterpreted into the `timestamptz` column or rounded by it.
#[test]
fn a_datetime_property_value_outside_the_clause_4_6_3_form_is_refused() {
    for raw in ["2026-07-25T12:00:00+02:00", "2026-07-25T12:00:00.1234567Z"] {
        let error = prepare(&entity_id(), &[], &[datetime_property(raw)], record(recorded_at()), WriteOrdinal::at(0))
            .expect_err("the value is not a clause 4.6.3 DateTime");
        assert!(matches!(error, TemporalError::Validation { .. }), "{raw}: {error:?}");
    }
}

/// Clause 4.18: a reified Scope whose value breaks the scope grammar is no
/// Scope, and preparing it is refused rather than recording nothing.
#[test]
fn a_reified_scope_outside_the_scope_grammar_is_refused() {
    let scope: AttributeName = expanded_properties::SCOPE.as_str().parse().expect("valid attribute name");
    let reified = EntityDocument::new(
        None,
        Vec::new(),
        None,
        None,
        IndexMap::from([(
            scope,
            AttributeMember::Instances(vec![reified_scope(&json!([{"@value": "/a-b"}]), "2020-06-01T00:00:00Z")]),
        )]),
    );
    let error = prepare(
        &entity_id(),
        &[],
        &[HistoryRecord::EvolutionInstances(Arc::new(reified))],
        record(recorded_at()),
        WriteOrdinal::at(0),
    )
    .expect_err("`/a-b` is no scope path");
    assert!(matches!(error, TemporalError::Validation { .. }), "{error:?}");
}

/// Clause 4.5.6: a reified Scope whose instance carries a `datasetId` holds a
/// member it cannot have, so recording a history that holds one is refused as
/// a client's write of it is.
#[test]
fn a_reified_scope_carrying_a_dataset_id_is_refused() {
    let scope: AttributeName = expanded_properties::SCOPE.as_str().parse().expect("valid attribute name");
    let instance = json!({
        "@type": [PROPERTY],
        expanded_properties::VALUE.as_str(): [{"@value": "/Madrid"}],
        expanded_properties::DATASET_ID.as_str(): [{"@id": "urn:ngsi-ld:Dataset:a"}]
    });
    let instance = InstanceDocument::from_slice(&scope, &serde_json::to_vec(&instance).expect("JSON")).expect("a reified scope instance decodes");
    let error =
        prepare(&entity_id(), &[], &[reified_document(instance)], record(recorded_at()), WriteOrdinal::at(0)).expect_err("a reified Scope has no datasetId");
    assert!(
        matches!(
            error,
            TemporalError::Validation(TemporalValidationReason::Representation(EntityRefusal::Scope(ScopeRefusal::MemberForbidden {
                member: MemberShape::DatasetId
            })))
        ),
        "{error:?}"
    );
}
