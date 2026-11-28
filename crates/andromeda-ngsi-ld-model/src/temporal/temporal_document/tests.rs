use crate::{
    entity_system_times::EntitySystemTimes,
    temporal::temporal_document::{MergeMode, TemporalDocument},
};
use andromeda_ngsi_ld_entity::document::{EntityDocument, attribute_member::AttributeMember, instance_document::InstanceDocument, slot::Slot};
use andromeda_ngsi_ld_lexicon::{
    attribute_name::AttributeName,
    attribute_type::AttributeType,
    identifier::Identifier,
    time::{date_time::NgsiLdDateTime, system_instant::SystemInstant},
    time_prop::TimeProp,
};
use andromeda_ngsi_ld_vocabulary::expanded::properties as expanded_properties;
use chrono::{DateTime, Utc};
use indexmap::IndexMap;
use std::num::NonZeroU32;

fn at(raw: &str) -> DateTime<Utc> {
    raw.parse::<NgsiLdDateTime>().expect("a valid instant").get()
}

fn speed() -> AttributeName {
    "https://example.com/speed".parse().expect("a valid attribute name")
}

fn instance(observed: &str, id: Option<&str>) -> InstanceDocument {
    InstanceDocument::builder()
        .family(Some(AttributeType::Property))
        .observed_at(Some(Slot::Set(observed.parse::<NgsiLdDateTime>().expect("an instant"))))
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
        .observed_at(Some(Slot::Set(observed.parse::<NgsiLdDateTime>().expect("an instant"))))
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

/// An instance carrying `instanceId` `id`, observed at `observed` when given,
/// last modified at `modified`.
fn modified(id: &str, observed: Option<&str>, modified: &str) -> InstanceDocument {
    InstanceDocument::builder()
        .family(Some(AttributeType::Property))
        .observed_at(observed.map(|raw| Slot::Set(raw.parse::<NgsiLdDateTime>().expect("an instant"))))
        .modified_at(Some(modified.parse::<SystemInstant>().expect("an instant")))
        .instance_id(Some(id.parse::<Identifier>().expect("an identifier")))
        .build()
}

/// The `modifiedAt` of every instance of the document, in order.
fn modifications(document: &TemporalDocument) -> Vec<Option<SystemInstant>> {
    document.instances().map(|(_, instance)| instance.modified_at()).collect()
}

/// Clause 4.5.5.3: two holders answering one `instanceId` without
/// `observedAt` are in conflict, and the instance modified later is provided,
/// whichever holder answered first.
#[test]
fn a_shared_instance_is_the_one_modified_later_whichever_holder_answered_first() {
    let earlier = || document(vec![modified("urn:ngsi-ld:i:1", None, "2020-01-01T00:00:00Z")]);
    let later = || document(vec![modified("urn:ngsi-ld:i:1", None, "2020-02-01T00:00:00Z")]);
    let expected = vec![Some("2020-02-01T00:00:00Z".parse::<SystemInstant>().expect("an instant"))];
    let mut held_earlier = earlier();
    held_earlier.merge(later(), MergeMode::Inclusive, TimeProp::Modified);
    pretty_assertions::assert_eq!(modifications(&held_earlier), expected);
    let mut held_later = later();
    held_later.merge(earlier(), MergeMode::Inclusive, TimeProp::Modified);
    pretty_assertions::assert_eq!(modifications(&held_later), expected);
}

/// Clause 4.5.5.3: `observedAt` decides before `modifiedAt`, and an instance
/// carrying it is the more current over one carrying none.
#[test]
fn a_shared_instance_is_the_observed_one_before_the_modified_one() {
    let mut local = document(vec![modified("urn:ngsi-ld:i:1", None, "2020-03-01T00:00:00Z")]);
    local.merge(
        document(vec![modified("urn:ngsi-ld:i:1", Some("2020-01-01T00:00:00Z"), "2020-01-01T00:00:00Z")]),
        MergeMode::Inclusive,
        TimeProp::Observed,
    );
    pretty_assertions::assert_eq!(observed(&local), vec![at("2020-01-01T00:00:00Z")]);
    let mut observed_later = document(vec![modified("urn:ngsi-ld:i:1", Some("2020-02-01T00:00:00Z"), "2020-01-01T00:00:00Z")]);
    observed_later.merge(
        document(vec![modified("urn:ngsi-ld:i:1", Some("2020-01-01T00:00:00Z"), "2020-05-01T00:00:00Z")]),
        MergeMode::Inclusive,
        TimeProp::Observed,
    );
    pretty_assertions::assert_eq!(observed(&observed_later), vec![at("2020-02-01T00:00:00Z")]);
}

/// A replaced instance takes its place in the order of the time property.
#[test]
fn a_replaced_instance_is_reordered_on_the_time_property() {
    let mut local = document(vec![
        modified("urn:ngsi-ld:i:1", None, "2020-01-01T00:00:00Z"),
        modified("urn:ngsi-ld:i:2", None, "2020-01-02T00:00:00Z"),
    ]);
    local.merge(
        document(vec![modified("urn:ngsi-ld:i:1", None, "2020-01-03T00:00:00Z")]),
        MergeMode::Inclusive,
        TimeProp::Modified,
    );
    let ids: Vec<Option<&str>> = local.instances().map(|(_, instance)| instance.instance_id().map(Identifier::as_str)).collect();
    pretty_assertions::assert_eq!(ids, vec![Some("urn:ngsi-ld:i:2"), Some("urn:ngsi-ld:i:1")]);
}

/// Clause 6.3.11: without `sysAttrs` neither the entity nor an instance keeps
/// its `createdAt` or `modifiedAt`; an instance's `deletedAt` and
/// `observedAt` stay.
#[test]
fn forgetting_the_system_times_keeps_the_observations_and_deletions() {
    let instant = || "2020-01-01T00:00:00Z".parse::<SystemInstant>().expect("an instant");
    let deleted_at = "2020-01-02T00:00:00Z".parse::<NgsiLdDateTime>().expect("an instant");
    let instance = InstanceDocument::builder()
        .family(Some(AttributeType::Property))
        .observed_at(Some(Slot::Set("2019-12-31T00:00:00Z".parse::<NgsiLdDateTime>().expect("an instant"))))
        .created_at(Some(instant()))
        .modified_at(Some(instant()))
        .deleted_at(Some(deleted_at))
        .build();
    let entity = EntityDocument::new(
        Some("urn:ngsi-ld:Vehicle:1".parse().expect("an identifier")),
        vec!["https://example.com/Vehicle".parse().expect("a type")],
        None,
        None,
        IndexMap::from([(speed(), AttributeMember::Instances(vec![instance]))]),
    );
    let times = EntitySystemTimes {
        created_at: Some(at("2020-01-01T00:00:00Z")),
        modified_at: Some(at("2020-01-01T00:00:00Z")),
        deleted_at: None,
    };
    let mut evolution = TemporalDocument::new(entity, times, IndexMap::new());
    evolution.forget_system_times();
    pretty_assertions::assert_eq!(evolution.system_times(), EntitySystemTimes::default());
    let (_, kept) = evolution.instances().next().expect("the instance");
    pretty_assertions::assert_eq!((kept.created_at(), kept.modified_at()), (None, None));
    pretty_assertions::assert_eq!(kept.deleted_at(), Some(deleted_at));
    pretty_assertions::assert_eq!(kept.instant_on(TimeProp::Observed), Some(at("2019-12-31T00:00:00Z")));
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

/// Each stream keeps the most recent instances its room admits, oldest first:
/// two of the default instance, one of dataset one and none of dataset two.
#[test]
fn keeping_the_latest_keeps_the_most_recent_instances_each_stream_has_room_for() {
    let one = "urn:ngsi-ld:Dataset:one".parse::<Identifier>().expect("an identifier");
    let mut history = document(vec![
        instance("2020-01-03T00:00:00Z", None),
        instance("2020-01-01T00:00:00Z", None),
        instance("2020-01-02T00:00:00Z", None),
        in_dataset("2020-01-04T00:00:00Z", "urn:ngsi-ld:Dataset:one"),
        in_dataset("2020-01-05T00:00:00Z", "urn:ngsi-ld:Dataset:one"),
        in_dataset("2020-01-06T00:00:00Z", "urn:ngsi-ld:Dataset:two"),
        undated("urn:ngsi-ld:i:u1"),
    ]);
    history.keep_latest(TimeProp::Observed, |_, dataset| match dataset {
        None => 2,
        Some(dataset) if *dataset == one => 1,
        Some(_) => 0,
    });
    pretty_assertions::assert_eq!(
        observed(&history),
        vec![at("2020-01-02T00:00:00Z"), at("2020-01-03T00:00:00Z"), at("2020-01-05T00:00:00Z")]
    );
    pretty_assertions::assert_eq!(history.instance_count(), 3);
}

/// Clause 4.18: a Scope is valid over a span rather than at an instant, so
/// keeping the latest instances keeps its history whole.
#[test]
fn keeping_the_latest_keeps_the_scope_whole() {
    let scope = expanded_properties::SCOPE.as_str().parse::<AttributeName>().expect("a valid attribute name");
    let entity = EntityDocument::new(
        Some("urn:ngsi-ld:Vehicle:1".parse().expect("an identifier")),
        vec!["https://example.com/Vehicle".parse().expect("a type")],
        None,
        None,
        IndexMap::from([
            (speed(), AttributeMember::Instances(vec![instance("2020-01-01T00:00:00Z", None)])),
            (
                scope.clone(),
                AttributeMember::Instances(vec![undated("urn:ngsi-ld:i:s1"), undated("urn:ngsi-ld:i:s2")]),
            ),
        ]),
    );
    let mut history = TemporalDocument::new(entity, EntitySystemTimes::default(), IndexMap::new());
    history.keep_latest(TimeProp::Observed, |_, _| 0);
    pretty_assertions::assert_eq!(history.instances().filter(|(name, _)| **name == scope).count(), 2);
    pretty_assertions::assert_eq!(history.instance_count(), 2);
}
