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
use std::num::NonZeroU32;

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

fn in_dataset(observed: &str, dataset: &str) -> InstanceDocument {
    InstanceDocument::builder()
        .family(Some(AttributeType::Property))
        .observed_at(Some(Slot::Set(observed.parse::<DateTimeText>().expect("an instant"))))
        .dataset_id(Some(dataset.parse::<Identifier>().expect("an identifier")))
        .build()
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
    local.keep_last(TimeProp::Observed, NonZeroU32::new(3).expect("three"));
    pretty_assertions::assert_eq!(
        observed(&local),
        vec![at("2020-01-03T00:00:00Z"), at("2020-01-04T00:00:00Z"), at("2020-01-05T00:00:00Z")]
    );
}

/// An instance carrying no `observedAt`, held by any holder.
fn undated(id: &str) -> InstanceDocument {
    InstanceDocument::builder()
        .family(Some(AttributeType::Property))
        .instance_id(Some(id.parse::<Identifier>().expect("an identifier")))
        .build()
}

/// Clause 5.7.3.4: `lastN` is "the last n timestamps [...] of the temporal
/// property", so an instance with no `observedAt` is neither counted nor kept,
/// even when fewer dated instances than `n` remain.
#[test]
fn last_n_after_a_merge_leaves_out_the_instances_with_no_instant() {
    let mut local = document(vec![
        undated("urn:ngsi-ld:i:u1"),
        instance("2020-01-02T00:00:00Z", Some("urn:ngsi-ld:i:2")),
        instance("2020-01-03T00:00:00Z", Some("urn:ngsi-ld:i:3")),
    ]);
    let remote = document(vec![instance("2020-01-01T00:00:00Z", Some("urn:ngsi-ld:i:1")), undated("urn:ngsi-ld:i:u2")]);
    local.merge(remote, MergeMode::Inclusive, TimeProp::Observed);

    let mut two = local.clone();
    two.keep_last(TimeProp::Observed, NonZeroU32::new(2).expect("two"));
    pretty_assertions::assert_eq!(observed(&two), vec![at("2020-01-02T00:00:00Z"), at("2020-01-03T00:00:00Z")]);
    pretty_assertions::assert_eq!(two.instance_count(), 2);

    local.keep_last(TimeProp::Observed, NonZeroU32::new(10).expect("ten"));
    pretty_assertions::assert_eq!(
        observed(&local),
        vec![at("2020-01-01T00:00:00Z"), at("2020-01-02T00:00:00Z"), at("2020-01-03T00:00:00Z")]
    );
    pretty_assertions::assert_eq!(local.instance_count(), 3);
}

/// Clause 4.5.7: a read on `observedAt` keeps the instances observed and
/// leaves the others out; an attribute left without an instance stays with
/// the family it recorded.
#[test]
fn keeping_the_positioned_instances_leaves_out_the_unobserved_ones() {
    let mut mixed = document(vec![
        undated("urn:ngsi-ld:i:u1"),
        instance("2020-01-01T00:00:00Z", None),
        undated("urn:ngsi-ld:i:u2"),
    ]);
    mixed.retain_positioned(TimeProp::Observed);
    pretty_assertions::assert_eq!(observed(&mixed), vec![at("2020-01-01T00:00:00Z")]);
    pretty_assertions::assert_eq!(mixed.instance_count(), 1);

    let mut unobserved = document(vec![undated("urn:ngsi-ld:i:u3")]);
    unobserved.retain_positioned(TimeProp::Observed);
    pretty_assertions::assert_eq!(unobserved.instance_count(), 0);
    pretty_assertions::assert_eq!(unobserved.emptied().get(&speed()), Some(&AttributeType::Property));
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

#[test]
fn instances_are_kept_by_the_attribute_they_belong_to_and_an_emptied_attribute_keeps_its_family() {
    let mut held = document(vec![instance("2024-01-01T00:00:00Z", None), instance("2024-01-02T00:00:00Z", None)]);
    held.retain_instances(|name, _| name != &speed());
    pretty_assertions::assert_eq!(held.instance_count(), 0);
    pretty_assertions::assert_eq!(held.emptied().get(&speed()), Some(&AttributeType::Property));
    let mut held = document(vec![instance("2024-01-01T00:00:00Z", None), instance("2024-01-02T00:00:00Z", None)]);
    held.retain_instances(|name, kept| name == &speed() && kept.instant_on(TimeProp::Observed) > Some(at("2024-01-01T00:00:00Z")));
    pretty_assertions::assert_eq!(held.instance_count(), 1);
    assert!(held.emptied().is_empty());
}

/// `speed` holds a default stream of 3 admitted instances and a dataset
/// stream none of which is admitted; `fuelLevel` holds 2 admitted instances.
/// The dataset stream every instance of which lies past the bound counts 0;
/// admitting everything, the dataset stream's one instance is the fewest.
#[test]
fn the_fewest_stream_instances_counts_every_dataset_stream_apart() {
    let fuel: AttributeName = "https://example.com/fuelLevel".parse().expect("a valid attribute name");
    let bound = at("2020-01-01T10:00:00Z");
    let entity = EntityDocument::new(
        Some("urn:ngsi-ld:Vehicle:1".parse().expect("an identifier")),
        vec!["https://example.com/Vehicle".parse().expect("a type")],
        None,
        None,
        IndexMap::from([
            (
                speed(),
                AttributeMember::Instances(vec![
                    instance("2020-01-01T01:00:00Z", None),
                    instance("2020-01-01T02:00:00Z", None),
                    instance("2020-01-01T03:00:00Z", None),
                    in_dataset("2020-01-01T11:00:00Z", "urn:ngsi-ld:Dataset:late"),
                ]),
            ),
            (
                fuel,
                AttributeMember::Instances(vec![
                    instance("2020-01-01T04:00:00Z", None),
                    instance("2020-01-01T05:00:00Z", None),
                    instance("2020-01-01T12:00:00Z", None),
                ]),
            ),
        ]),
    );
    let document = TemporalDocument::new(entity, EntitySystemTimes::default(), IndexMap::new());
    pretty_assertions::assert_eq!(document.fewest_stream_instances(TimeProp::Observed, |instant| instant <= bound), Some(0));
    pretty_assertions::assert_eq!(document.fewest_stream_instances(TimeProp::Observed, |_| true), Some(1));
    pretty_assertions::assert_eq!(TemporalDocument::default().fewest_stream_instances(TimeProp::Observed, |_| true), None);
}

/// Two of the three instances of the one stream lie at or before the bound.
#[test]
fn the_fewest_stream_instances_is_the_admitted_count_of_the_sparsest_stream() {
    let document = document(vec![
        instance("2020-01-01T01:00:00Z", None),
        instance("2020-01-01T02:00:00Z", None),
        instance("2020-01-01T03:00:00Z", None),
    ]);
    pretty_assertions::assert_eq!(
        document.fewest_stream_instances(TimeProp::Observed, |instant| instant <= at("2020-01-01T02:00:00Z")),
        Some(2)
    );
}
