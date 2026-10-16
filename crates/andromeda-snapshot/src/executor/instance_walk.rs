//! Reading the instances of a captured temporal answer to the end of its
//! window (clauses 5.16.1.4 and 6.3.10).
//!
//! Clause 5.16.1.4: "If the size of the respective results require pagination,
//! all pages are to be retrieved completely." A temporal answer pages in two
//! dimensions: its entities, and the instances of each, which a partial answer
//! cuts at the one instant its `Content-Range` ends at (clause 6.3.10). The
//! capture asks for the entities of one page again, with the window resumed at
//! that instant, until no answer is partial, as a client following the range
//! would.
//!
//! The resumed window includes the instant, and a page never splits the
//! instances one attribute holds at the instant it is cut at, so every instance
//! at or before it is already captured and is left out of the resumed page.
//! Every page of an entity carries its Scope history whole, since a Scope has
//! no position on the requested axis (clause 4.18), so the Scope of an entity
//! an earlier page captured is left out too.

use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, identifier::Identifier, rfc3339::sub_second, time_prop::TimeProp};
use andromeda_ngsi_ld_model::temporal::temporal_document::TemporalDocument;
use andromeda_ngsi_ld_query::temporal::{instance_page::PageDirection, window::TimeInterval};
use andromeda_ngsi_ld_vocabulary::expanded::properties;
use andromeda_temporal::service::TemporalRange;
use chrono::{DateTime, Utc};
use std::collections::HashSet;
use thiserror::Error;

/// How far the instances of one page of captured entities have been read.
#[derive(Debug)]
pub(crate) struct InstanceWalk {
    /// The interval the next page of instances is read over.
    interval: TimeInterval,
    /// The temporal property the window applies to.
    timeproperty: TimeProp,
    /// The instant the last partial page was cut at; every instance at or
    /// before it is captured. `None` until a page was cut.
    captured_through: Option<DateTime<Utc>>,
    /// The entities a page of the walk captured, whose Scope history is held.
    captured: HashSet<Identifier>,
}

/// Whether the walk has read the whole window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum WalkProgress {
    /// The page was partial; the rest is read over [`InstanceWalk::interval`].
    Resumed,
    /// The page answered with the rest of the window.
    Complete,
}

/// Why the rest of a partial answer cannot be read.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub(crate) enum WalkRefusal {
    /// A `lastN` above the broker's maximum: clause 6.3.10 pages it backwards,
    /// and the instances still missing differ from one attribute to the next,
    /// which one resumed request cannot say.
    #[error(
        "the temporal query asks for a lastN above the broker's instance maximum, whose remainder pages backwards, and a Snapshot cannot capture it completely"
    )]
    Backward,
    /// The page was cut at an instant no later than the previous one: an
    /// attribute holds more instances at it than one page carries.
    #[error("an attribute holds more instances at {} than one page carries, so its history cannot be read past that instant", sub_second(*at))]
    Stalled {
        /// The instant the page could not be read past.
        at: DateTime<Utc>,
    },
    /// The page was partial but named no instant to resume from.
    #[error("a partial temporal answer named no instant to resume from")]
    Unpositioned,
}

impl InstanceWalk {
    /// A walk over `interval`, positioned on `timeproperty`.
    pub(crate) fn new(interval: TimeInterval, timeproperty: TimeProp) -> InstanceWalk {
        InstanceWalk {
            interval,
            timeproperty,
            captured_through: None,
            captured: HashSet::new(),
        }
    }

    /// The interval the next page of instances is read over.
    pub(crate) const fn interval(&self) -> TimeInterval {
        self.interval
    }

    /// Leave out of `documents` what an earlier page of the walk captured, and
    /// an entity such a page captured that the page holds nothing new of.
    pub(crate) fn trim(&mut self, documents: &mut Vec<TemporalDocument>) {
        if let Some(through) = self.captured_through {
            let timeproperty = self.timeproperty;
            documents.retain_mut(|document| {
                let held = document.id().is_some_and(|id| self.captured.contains(id));
                document.retain_instances(|name, instance| {
                    if is_scope(name) {
                        !held
                    } else {
                        instance.instant_on(timeproperty).is_some_and(|instant| instant > through)
                    }
                });
                !held || document.instance_count() > 0
            });
        }
        // clone: the walk remembers the entities after the page is stored
        self.captured.extend(documents.iter().filter_map(TemporalDocument::id).cloned());
    }

    /// Take the range a page answered with, resuming the interval at the
    /// instant a partial page ends at.
    ///
    /// # Errors
    ///
    /// Returns [`WalkRefusal`] when the rest of a partial page cannot be read.
    pub(crate) fn advance(&mut self, range: Option<&TemporalRange>) -> Result<WalkProgress, WalkRefusal> {
        let Some(range) = range else {
            return Ok(WalkProgress::Complete);
        };
        if let PageDirection::Backward { .. } = range.direction {
            return Err(WalkRefusal::Backward);
        }
        let (_, cut) = range.observed.ok_or(WalkRefusal::Unpositioned)?;
        if self.captured_through.is_some_and(|through| cut <= through) {
            return Err(WalkRefusal::Stalled { at: cut });
        }
        self.captured_through = Some(cut);
        self.interval = self.interval.resumed_at(cut);
        Ok(WalkProgress::Resumed)
    }
}

/// Whether `name` is the reified Scope (clause 4.18).
fn is_scope(name: &AttributeName) -> bool {
    name.as_str() == properties::SCOPE.as_str()
}

#[cfg(test)]
mod tests {
    use crate::executor::instance_walk::{InstanceWalk, WalkProgress, WalkRefusal};
    use andromeda_ngsi_ld_entity::{
        date_time_text::DateTimeText,
        document::{EntityDocument, attribute_member::AttributeMember, instance_document::InstanceDocument, slot::Slot},
    };
    use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, attribute_type::AttributeType, time_prop::TimeProp};
    use andromeda_ngsi_ld_model::{entity_system_times::EntitySystemTimes, temporal::temporal_document::TemporalDocument};
    use andromeda_ngsi_ld_query::temporal::{instance_page::PageDirection, window::TimeInterval};
    use andromeda_ngsi_ld_vocabulary::expanded::properties;
    use andromeda_temporal::service::TemporalRange;
    use chrono::{DateTime, Utc};
    use indexmap::IndexMap;
    use pretty_assertions::assert_eq;
    use std::num::NonZeroU32;

    fn at(raw: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(raw).expect("a valid instant").with_timezone(&Utc)
    }

    fn speed() -> AttributeName {
        "https://example.com/speed".parse().expect("a valid attribute name")
    }

    fn scope() -> AttributeName {
        properties::SCOPE.as_str().parse().expect("a valid attribute name")
    }

    fn instance(observed: &str) -> InstanceDocument {
        InstanceDocument::builder()
            .family(Some(AttributeType::Property))
            .observed_at(Some(Slot::Set(observed.parse::<DateTimeText>().expect("an instant"))))
            .build()
    }

    /// One entity's page: the `speed` instances observed at `observed`, and one
    /// Scope instance.
    fn page_of(entity: &str, observed: &[&str]) -> TemporalDocument {
        let entity = EntityDocument::new(
            Some(entity.parse().expect("an identifier")),
            vec!["https://example.com/Vehicle".parse().expect("a type")],
            None,
            None,
            IndexMap::from([
                (speed(), AttributeMember::Instances(observed.iter().map(|raw| instance(raw)).collect())),
                (scope(), AttributeMember::Instances(vec![instance("2020-01-01T00:00:00Z")])),
            ]),
        );
        TemporalDocument::new(entity, EntitySystemTimes::default(), IndexMap::new())
    }

    fn forward_cut(least: &str, most: &str) -> TemporalRange {
        TemporalRange {
            interval: TimeInterval::After(at("2024-01-01T00:00:00Z")),
            observed: Some((at(least), at(most))),
            direction: PageDirection::Forward,
        }
    }

    /// The `speed` instants a page keeps, and how many Scope instances.
    fn kept(page: &TemporalDocument) -> (Vec<DateTime<Utc>>, usize) {
        let speeds = page
            .instances()
            .filter(|(name, _)| **name == speed())
            .filter_map(|(_, instance)| instance.instant_on(TimeProp::Observed))
            .collect();
        (speeds, page.instances().filter(|(name, _)| **name == scope()).count())
    }

    /// Clause 5.16.1.4: a temporal answer cut three times is read to the end
    /// of its window, each page resumed at the instant the previous one ended
    /// at (clause 6.3.10), and every instance is stored once.
    #[test]
    fn a_temporal_capture_follows_the_instance_range_to_its_end() {
        let mut walk = InstanceWalk::new(TimeInterval::After(at("2024-01-01T00:00:00Z")), TimeProp::Observed);
        let answers = [
            (
                vec!["2024-01-01T00:00:00Z", "2024-01-02T00:00:00Z"],
                Some(forward_cut("2024-01-01T00:00:00Z", "2024-01-02T00:00:00Z")),
            ),
            (
                vec!["2024-01-02T00:00:00Z", "2024-01-03T00:00:00Z"],
                Some(forward_cut("2024-01-02T00:00:00Z", "2024-01-03T00:00:00Z")),
            ),
            (
                vec!["2024-01-03T00:00:00Z", "2024-01-04T00:00:00Z"],
                Some(forward_cut("2024-01-03T00:00:00Z", "2024-01-04T00:00:00Z")),
            ),
            (vec!["2024-01-04T00:00:00Z", "2024-01-05T00:00:00Z"], None),
        ];
        let mut intervals = vec![walk.interval()];
        let mut stored = Vec::new();
        let mut progress = Vec::new();
        for (observed, range) in answers {
            let mut documents = vec![page_of("urn:ngsi-ld:Vehicle:1", &observed)];
            walk.trim(&mut documents);
            stored.extend(documents.iter().map(kept));
            progress.push(walk.advance(range.as_ref()).expect("a page the walk can resume past"));
            intervals.push(walk.interval());
        }
        assert_eq!(
            progress,
            vec![WalkProgress::Resumed, WalkProgress::Resumed, WalkProgress::Resumed, WalkProgress::Complete]
        );
        assert_eq!(
            intervals[..4],
            [
                TimeInterval::After(at("2024-01-01T00:00:00Z")),
                TimeInterval::After(at("2024-01-02T00:00:00Z")),
                TimeInterval::After(at("2024-01-03T00:00:00Z")),
                TimeInterval::After(at("2024-01-04T00:00:00Z")),
            ]
        );
        assert_eq!(
            stored,
            vec![
                (vec![at("2024-01-01T00:00:00Z"), at("2024-01-02T00:00:00Z")], 1),
                (vec![at("2024-01-03T00:00:00Z")], 0),
                (vec![at("2024-01-04T00:00:00Z")], 0),
                (vec![at("2024-01-05T00:00:00Z")], 0),
            ]
        );
    }

    /// An entity a cut page emptied first appears on a resumed page, and its
    /// Scope history is captured with it there.
    #[test]
    fn an_entity_first_answered_on_a_resumed_page_keeps_its_scope() {
        let mut walk = InstanceWalk::new(TimeInterval::After(at("2024-01-01T00:00:00Z")), TimeProp::Observed);
        let mut first = vec![page_of("urn:ngsi-ld:Vehicle:1", &["2024-01-01T00:00:00Z"])];
        walk.trim(&mut first);
        walk.advance(Some(&forward_cut("2024-01-01T00:00:00Z", "2024-01-01T00:00:00Z")))
            .expect("a resumable page");
        let mut resumed = vec![
            page_of("urn:ngsi-ld:Vehicle:1", &["2024-01-01T00:00:00Z"]),
            page_of("urn:ngsi-ld:Vehicle:2", &["2024-01-02T00:00:00Z"]),
        ];
        walk.trim(&mut resumed);
        assert_eq!(resumed.len(), 1, "the first entity has nothing new: {resumed:?}");
        assert_eq!(kept(&resumed[0]), (vec![at("2024-01-02T00:00:00Z")], 1));
    }

    /// A `between` window keeps its closing instant as it resumes.
    #[test]
    fn a_between_window_resumes_up_to_its_closing_instant() {
        let to = at("2024-02-01T00:00:00Z");
        let mut walk = InstanceWalk::new(
            TimeInterval::Between {
                from: at("2024-01-01T00:00:00Z"),
                to,
            },
            TimeProp::Observed,
        );
        walk.advance(Some(&forward_cut("2024-01-01T00:00:00Z", "2024-01-10T00:00:00Z")))
            .expect("a resumable page");
        assert_eq!(
            walk.interval(),
            TimeInterval::Between {
                from: at("2024-01-10T00:00:00Z"),
                to
            }
        );
    }

    /// A page cut where the previous one was cut cannot be read past.
    #[test]
    fn a_page_cut_no_later_than_the_previous_one_stalls_the_walk() {
        let mut walk = InstanceWalk::new(TimeInterval::After(at("2024-01-01T00:00:00Z")), TimeProp::Observed);
        let cut = forward_cut("2024-01-01T00:00:00Z", "2024-01-02T00:00:00Z");
        walk.advance(Some(&cut)).expect("a resumable page");
        assert_eq!(
            walk.advance(Some(&cut)),
            Err(WalkRefusal::Stalled {
                at: at("2024-01-02T00:00:00Z")
            })
        );
    }

    /// Clause 6.3.10 pages a `lastN` above the maximum backwards, which the
    /// capture refuses rather than store its most recent page alone.
    #[test]
    fn a_backward_page_is_refused() {
        let mut walk = InstanceWalk::new(TimeInterval::After(at("2024-01-01T00:00:00Z")), TimeProp::Observed);
        let backward = TemporalRange {
            direction: PageDirection::Backward {
                requested: NonZeroU32::new(5000).expect("a positive lastN"),
            },
            ..forward_cut("2024-01-01T00:00:00Z", "2024-01-02T00:00:00Z")
        };
        assert_eq!(walk.advance(Some(&backward)), Err(WalkRefusal::Backward));
    }
}
