//! How many instances of each attribute stream of each entity a temporal
//! capture holds under the query's `lastN` (Table 5.2.21-1, clause 6.3.10).
//!
//! Table 5.2.21-1: "Only the last n instances, per Attribute, per Entity
//! (under the specified time interval) shall be retrieved". A `lastN` above
//! the instance maximum of the broker or of a Context Source is paged
//! backwards, the most recent instances first (clause 6.3.10), and the capture
//! reads every page: of each, a stream keeps only the most recent instances it
//! still lacks to reach `lastN`, and one already holding `lastN` keeps none. A
//! stream is an attribute and the `datasetId` of its instances, as the broker
//! counts `lastN` itself.

use crate::executor::instance_walk::is_scope;
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, identifier::Identifier, time_prop::TimeProp};
use andromeda_ngsi_ld_model::temporal::temporal_document::TemporalDocument;
use std::{collections::HashMap, num::NonZeroU32};

/// The streams of one entity a capture holds instances of: each stream's
/// attribute, `datasetId` and how many instances.
type Streams = Vec<(AttributeName, Option<Identifier>, u32)>;

/// How many instances of each stream of each entity a capture holds.
#[derive(Debug)]
pub(crate) struct LastNTally {
    /// The `lastN` the query asked for.
    last_n: NonZeroU32,
    /// The instances held of the streams of each entity.
    held: HashMap<Identifier, Streams>,
}

impl LastNTally {
    /// A tally of a query asking for `last_n` instances, holding none yet.
    pub(crate) fn new(last_n: NonZeroU32) -> LastNTally {
        LastNTally { last_n, held: HashMap::new() }
    }

    /// The `lastN` the query asked for.
    pub(crate) const fn last_n(&self) -> NonZeroU32 {
        self.last_n
    }

    /// Keep of `document` only the most recent instances on `timeproperty`
    /// each of its streams still lacks to reach `lastN`, and count them as
    /// held. Its Scope history is kept whole and counts nothing.
    pub(crate) fn keep_lacking(&mut self, document: &mut TemporalDocument, timeproperty: TimeProp) {
        let last_n = self.last_n.get();
        let streams = document.id().and_then(|id| self.held.get(id));
        document.keep_latest(timeproperty, |name, dataset| {
            let held = streams.and_then(|streams| held_of(streams, name, dataset)).unwrap_or(0);
            usize::try_from(last_n.saturating_sub(held)).unwrap_or(usize::MAX)
        });
        let Some(id) = document.id() else {
            return;
        };
        if let Some(streams) = self.held.get_mut(id) {
            count_into(streams, document);
        } else {
            let mut streams = Vec::new();
            count_into(&mut streams, document);
            // clone: the tally remembers the entity past the document it came in
            self.held.insert(id.clone(), streams);
        }
    }
}

/// How many instances of the stream of `name` and `dataset` `streams` holds,
/// if any.
fn held_of(streams: &Streams, name: &AttributeName, dataset: Option<&Identifier>) -> Option<u32> {
    streams
        .iter()
        .find(|(held, held_dataset, _)| held == name && held_dataset.as_ref() == dataset)
        .map(|(_, _, count)| *count)
}

/// Add to `streams` every instance `document` holds, its Scope history aside.
fn count_into(streams: &mut Streams, document: &TemporalDocument) {
    for (name, instance) in document.instances().filter(|(name, _)| !is_scope(name)) {
        let dataset = instance.dataset_id();
        match streams
            .iter_mut()
            .find(|(held, held_dataset, _)| held == name && held_dataset.as_ref() == dataset)
        {
            Some((_, _, count)) => *count = count.saturating_add(1),
            // clone: the tally remembers the stream past the document it came in
            None => streams.push((name.clone(), dataset.cloned(), 1)),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::executor::last_n_tally::LastNTally;
    use andromeda_ngsi_ld_entity::document::{EntityDocument, attribute_member::AttributeMember, instance_document::InstanceDocument, slot::Slot};
    use andromeda_ngsi_ld_lexicon::{
        attribute_name::AttributeName,
        attribute_type::AttributeType,
        identifier::Identifier,
        time::date_time::NgsiLdDateTime,
        time_prop::TimeProp,
    };
    use andromeda_ngsi_ld_model::{entity_system_times::EntitySystemTimes, temporal::temporal_document::TemporalDocument};
    use andromeda_ngsi_ld_vocabulary::expanded::properties;
    use chrono::{DateTime, Utc};
    use indexmap::IndexMap;
    use pretty_assertions::assert_eq;
    use std::num::NonZeroU32;

    const DATASET: &str = "urn:ngsi-ld:Dataset:a";

    fn at(day: u32) -> DateTime<Utc> {
        stamp(day).get()
    }

    fn stamp(day: u32) -> NgsiLdDateTime {
        format!("2024-01-{day:02}T00:00:00Z").parse().expect("a valid instant")
    }

    fn speed() -> AttributeName {
        "https://example.com/speed".parse().expect("a valid attribute name")
    }

    fn scope() -> AttributeName {
        properties::SCOPE.as_str().parse().expect("a valid attribute name")
    }

    /// A `speed` instance observed on day `day` of January, in `dataset`.
    fn instance(day: u32, dataset: Option<&str>) -> InstanceDocument {
        InstanceDocument::builder()
            .family(Some(AttributeType::Property))
            .observed_at(Some(Slot::Set(stamp(day))))
            .dataset_id(dataset.map(|raw| raw.parse::<Identifier>().expect("an identifier")))
            .build()
    }

    /// Vehicle 1 with the `speed` instances given and one Scope instance.
    fn vehicle(speeds: Vec<InstanceDocument>) -> TemporalDocument {
        let entity = EntityDocument::new(
            Some("urn:ngsi-ld:Vehicle:1".parse().expect("an identifier")),
            vec!["https://example.com/Vehicle".parse().expect("a type")],
            None,
            None,
            IndexMap::from([
                (speed(), AttributeMember::Instances(speeds)),
                (scope(), AttributeMember::Instances(vec![instance(1, None)])),
            ]),
        );
        TemporalDocument::new(entity, EntitySystemTimes::default(), IndexMap::new())
    }

    /// The day of every `speed` instance `document` keeps, beside its
    /// dataset, and how many Scope instances it keeps.
    fn kept(document: &TemporalDocument) -> (Vec<(u32, bool)>, usize) {
        let speeds = document
            .instances()
            .filter(|(name, _)| **name == speed())
            .filter_map(|(_, instance)| {
                let instant = instance.instant_on(TimeProp::Observed)?;
                let day = u32::try_from((instant - at(1)).num_days() + 1).ok()?;
                Some((day, instance.dataset_id().is_some()))
            })
            .collect();
        (speeds, document.instances().filter(|(name, _)| **name == scope()).count())
    }

    fn tally(last_n: u32) -> LastNTally {
        LastNTally::new(NonZeroU32::new(last_n).expect("a positive lastN"))
    }

    /// Keep what `tally` lets a page of the `speed` instances on `days` keep.
    fn page(tally: &mut LastNTally, days: &[u32]) -> (Vec<(u32, bool)>, usize) {
        let mut page = vehicle(days.iter().map(|day| instance(*day, None)).collect());
        tally.keep_lacking(&mut page, TimeProp::Observed);
        kept(&page)
    }

    /// Table 5.2.21-1: over three backward pages a stream keeps the instances
    /// it lacks to reach `lastN`, the most recent of a page first, and nothing
    /// once it holds `lastN`; the Scope is kept on every page and counts
    /// nothing.
    #[test]
    fn each_page_keeps_the_most_recent_instances_a_stream_still_lacks() {
        let mut tally = tally(5);
        assert_eq!(page(&mut tally, &[9, 10]), (vec![(9, false), (10, false)], 1));
        assert_eq!(page(&mut tally, &[5, 6, 7, 8]), (vec![(6, false), (7, false), (8, false)], 1));
        assert_eq!(page(&mut tally, &[3, 4]), (vec![], 1));
    }

    /// The broker counts `lastN` per attribute and `datasetId`, so each
    /// dataset of one attribute is a stream of its own.
    #[test]
    fn every_dataset_of_an_attribute_is_counted_apart() {
        let mut tally = tally(2);
        let mut first = vehicle(vec![instance(8, None), instance(9, None), instance(10, Some(DATASET))]);
        tally.keep_lacking(&mut first, TimeProp::Observed);
        let mut second = vehicle(vec![instance(5, None), instance(6, Some(DATASET)), instance(7, Some(DATASET))]);
        tally.keep_lacking(&mut second, TimeProp::Observed);
        assert_eq!(kept(&first).0, vec![(8, false), (9, false), (10, true)]);
        assert_eq!(kept(&second).0, vec![(7, true)]);
    }

    /// An answer holding more instances of a stream than `lastN`, as a page
    /// that keeps the instances sharing its edge instant together can, keeps
    /// the most recent `lastN`.
    #[test]
    fn a_page_holding_more_than_last_n_keeps_the_most_recent() {
        let mut tally = tally(2);
        assert_eq!(page(&mut tally, &[4, 5, 6]), (vec![(5, false), (6, false)], 1));
    }
}
