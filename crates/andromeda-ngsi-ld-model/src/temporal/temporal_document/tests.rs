use crate::{
    entity_system_times::EntitySystemTimes,
    temporal::temporal_document::{MergeMode, TemporalDocument},
};
use andromeda_ngsi_ld_entity::{
    date_time_text::DateTimeText,
    document::{EntityDocument, attribute_member::AttributeMember, instance_document::InstanceDocument, slot::Slot},
};
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, attribute_type::AttributeType, identifier::Identifier, time_prop::TimeProp};
use chrono::{DateTime, Utc};
use indexmap::IndexMap;

fn at(raw: &str) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(raw).expect("a valid instant").with_timezone(&Utc)
}

fn speed() -> AttributeName {
    "https://example.com/speed".parse().expect("a valid attribute name")
}

fn instance(observed: &str, id: Option<&str>) -> InstanceDocument {
    InstanceDocument::builder()
        .family(Some(AttributeType::Property))
        .observed_at(Some(Slot::Set(observed.parse::<DateTimeText>().expect("an instant"))))
        .instance_id(id.map(|raw| raw.parse::<Identifier>().expect("an identifier")))
        .build()
}

fn document(instances: Vec<InstanceDocument>) -> TemporalDocument {
    let entity = EntityDocument::new(
        Some("urn:ngsi-ld:Vehicle:1".parse().expect("an identifier")),
        vec!["https://example.com/Vehicle".parse().expect("a type")],
        None,
        None,
        IndexMap::from([(speed(), AttributeMember::Instances(instances))]),
    );
    TemporalDocument::new(entity, EntitySystemTimes::default(), IndexMap::new())
}

fn observed(document: &TemporalDocument) -> Vec<DateTime<Utc>> {
    document
        .instances()
        .filter_map(|(_, instance)| instance.instant_on(TimeProp::Observed))
        .collect()
}

/// Inclusive holders share instances by `instanceId`: a shared one counts once
/// and a new one is added.
#[test]
fn inclusive_sources_are_deduplicated_by_instance_id() {
    let mut local = document(vec![instance("2020-01-01T00:00:00Z", Some("urn:ngsi-ld:i:1"))]);
    let remote = document(vec![
        instance("2020-01-01T00:00:00Z", Some("urn:ngsi-ld:i:1")),
        instance("2020-01-02T00:00:00Z", Some("urn:ngsi-ld:i:2")),
    ]);
    local.merge(remote, MergeMode::Inclusive, TimeProp::Observed);
    pretty_assertions::assert_eq!(observed(&local), vec![at("2020-01-01T00:00:00Z"), at("2020-01-02T00:00:00Z")]);
}

/// Clause 5.7.3.4: an auxiliary instance is added only where no instance
/// received from elsewhere stands at the same instant.
#[test]
fn auxiliary_instances_fill_only_missing_instants() {
    let mut local = document(vec![instance("2020-01-01T00:00:00Z", Some("urn:ngsi-ld:i:1"))]);
    let auxiliary = document(vec![
        instance("2020-01-01T00:00:00Z", Some("urn:ngsi-ld:i:9")),
        instance("2020-01-02T00:00:00Z", Some("urn:ngsi-ld:i:2")),
    ]);
    local.merge(auxiliary, MergeMode::Auxiliary, TimeProp::Observed);
    pretty_assertions::assert_eq!(observed(&local), vec![at("2020-01-01T00:00:00Z"), at("2020-01-02T00:00:00Z")]);
}

/// An attribute only the other holder has joins whole.
#[test]
fn an_attribute_only_the_other_holder_has_joins_whole() {
    let mut local = TemporalDocument::default();
    local.merge(document(vec![instance("2020-01-01T00:00:00Z", None)]), MergeMode::Inclusive, TimeProp::Observed);
    pretty_assertions::assert_eq!(local.instance_count(), 1);
}

/// Clause 5.7.3.4 fixes which instants `lastN` keeps, not the order: after a
/// merge the last three are kept, oldest first.
#[test]
fn last_n_after_a_merge_keeps_the_most_recent_in_ascending_order() {
    let mut local = document(vec![
        instance("2020-01-02T00:00:00Z", Some("urn:ngsi-ld:i:2")),
        instance("2020-01-04T00:00:00Z", Some("urn:ngsi-ld:i:4")),
    ]);
    let remote = document(vec![
        instance("2020-01-01T00:00:00Z", Some("urn:ngsi-ld:i:1")),
        instance("2020-01-03T00:00:00Z", Some("urn:ngsi-ld:i:3")),
        instance("2020-01-05T00:00:00Z", Some("urn:ngsi-ld:i:5")),
    ]);
    local.merge(remote, MergeMode::Inclusive, TimeProp::Observed);
    local.keep_last(TimeProp::Observed, 3);
    pretty_assertions::assert_eq!(
        observed(&local),
        vec![at("2020-01-03T00:00:00Z"), at("2020-01-04T00:00:00Z"), at("2020-01-05T00:00:00Z")]
    );
}

/// Trimming to a bound leaves an attribute that lost every instance in place,
/// with the family it recorded, and reports the span the rest carries.
#[test]
fn trimming_keeps_an_emptied_attribute_with_its_family() {
    let mut local = document(vec![instance("2020-01-03T00:00:00Z", None)]);
    local.retain_instants(TimeProp::Observed, |instant| instant <= at("2020-01-02T00:00:00Z"));
    pretty_assertions::assert_eq!(local.instance_count(), 0);
    pretty_assertions::assert_eq!(local.emptied().get(&speed()), Some(&AttributeType::Property));
    pretty_assertions::assert_eq!(local.instant_span(TimeProp::Observed), None);
}

/// The span is the least and most recent instants on the requested axis.
#[test]
fn the_span_is_the_least_and_most_recent_instant() {
    let local = document(vec![instance("2020-01-03T00:00:00Z", None), instance("2020-01-01T00:00:00Z", None)]);
    pretty_assertions::assert_eq!(
        local.instant_span(TimeProp::Observed),
        Some((at("2020-01-01T00:00:00Z"), at("2020-01-03T00:00:00Z")))
    );
    pretty_assertions::assert_eq!(local.instant_span(TimeProp::Modified), None);
}
