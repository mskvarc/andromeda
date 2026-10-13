//! The intent the history consequences of one write prepare.

use crate::{append_intent::TemporalAppendEntry, error::TemporalError, intent_preparation::prepare, scope_observation::ScopeObservation};
use andromeda_context_commit::{
    history_record::{HistoryRecord, tombstone},
    write_id::WriteId,
    write_ordinal::WriteOrdinal,
    write_record::WriteRecord,
};
use andromeda_ngsi_ld_entity::{
    document::{EntityDocument, attribute_member::AttributeMember, instance_document::InstanceDocument},
    entity_state::EntityState,
};
use andromeda_ngsi_ld_lexicon::{
    attribute_name::AttributeName,
    attribute_type::AttributeType,
    identifier::Identifier,
    instance_family::InstanceFamily,
    scope::path::ScopePath,
};
use andromeda_ngsi_ld_model::temporal::{TemporalHistoryRow, TemporalValue};
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

    let scopes: Vec<(ScopeObservation, Option<DateTime<Utc>>)> = prepared(&history)
        .into_iter()
        .filter_map(|entry| match entry {
            TemporalAppendEntry::Scope { observation, observed_at, .. } => Some((observation, observed_at)),
            TemporalAppendEntry::Observation { .. } | TemporalAppendEntry::Tombstone { .. } => None,
        })
        .collect();
    let path = |raw: &str| ScopeObservation::Path(raw.parse::<ScopePath>().expect("a valid scope path"));
    pretty_assertions::assert_eq!(
        scopes,
        vec![
            (path("/Madrid"), Some(recorded_at())),
            (path("/Barcelona"), Some(observed_at)),
            (ScopeObservation::Deleted, Some(observed_at)),
        ]
    );
}

/// An instance whose value is the NGSI-LD Null records a deletion, which the
/// tombstone beside it carries; it is never decoded as an observation.
#[test]
fn a_deletion_is_recorded_by_its_tombstone_alone() {
    let attribute_name = "https://example.com/location".parse::<AttributeName>().expect("valid attribute name");
    let dataset_id = "urn:ngsi-ld:Dataset:a".parse::<Identifier>().expect("valid dataset identifier");
    let history = [
        written(&json!([{
            "@id": entity_id(),
            "https://example.com/location": [{
                "@type": ["https://uri.etsi.org/ngsi-ld/GeoProperty"],
                "https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": "urn:ngsi-ld:null"}],
                "https://uri.etsi.org/ngsi-ld/datasetId": [{"@id": dataset_id}]
            }]
        }])),
        tombstone(&attribute_name, Some(&dataset_id), AttributeType::GeoProperty),
    ];

    let entries = prepared(&history);

    pretty_assertions::assert_eq!(entries.len(), 1);
    assert!(matches!(
        &entries[0],
        TemporalAppendEntry::Tombstone {
            attribute_name: actual_attribute,
            dataset_id: Some(actual_dataset),
            ..
        } if actual_attribute == &attribute_name && actual_dataset == &dataset_id
    ));
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
        vec![TemporalValue::Number(JsonNumber::from(55)), TemporalValue::Number(JsonNumber::from(61))]
    );
    assert!(
        rows.iter()
            .all(|row| row.dataset_id.as_ref().is_some_and(|dataset| dataset.as_str() == "urn:ngsi-ld:Dataset:sensor"))
    );
    assert!(rows.iter().all(|row| row.observed_at.is_some() && row.created_at == Some(recorded_at())));
    pretty_assertions::assert_eq!(
        rows.iter()
            .map(|row| row.instance.rendered(InstanceFamily::Attribute).to_string())
            .collect::<Vec<_>>(),
        vec!["urn:ngsi-ld:instance:1", "urn:ngsi-ld:instance:2"]
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
