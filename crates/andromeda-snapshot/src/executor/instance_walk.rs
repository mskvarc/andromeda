//! Reading the instances of a captured temporal answer to the end of its
//! window (clauses 5.16.1.4 and 6.3.10).
//!
//! Clause 5.16.1.4: "If the size of the respective results require pagination,
//! all pages are to be retrieved completely." A temporal answer pages in two
//! dimensions: its entities, and the instances of each, which a partial answer
//! cuts at the one instant its `Content-Range` ends at (clause 6.3.10). The
//! capture asks for the entities of one page again, over the rest of the
//! window, until no answer is partial, as a client following the range would.
//!
//! Without `lastN` a page runs forwards and holds every instance at or before
//! its cut. The rest of the window is resumed at that instant, inclusively, so
//! every instance at or before it is left out of the resumed page. The broker
//! leaves the instances on the inclusive lower bound of a resumed window out
//! of its page size, so each resumed page ends past the instant it resumed at.
//!
//! With a `lastN` above the instance maximum a page runs backwards and holds
//! every instance at or after its cut, the least recent instant it holds. The
//! rest is the same query over the window ending at that instant, which a
//! closing instant leaves out (clause 4.11), and of each answer a stream keeps
//! only the most recent instances it still lacks to reach `lastN`.
//!
//! Every page of an entity carries its Scope history whole, since a Scope has
//! no position on the requested axis (clause 4.18), so the Scope of an entity
//! an earlier page captured is left out too.

use crate::executor::last_n_tally::LastNTally;
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, identifier::Identifier, time::rendering::instant_text, time_prop::TimeProp};
use andromeda_ngsi_ld_model::temporal::{temporal_document::TemporalDocument, temporal_history::TemporalHistory};
use andromeda_ngsi_ld_query::temporal::{instance_page::PageDirection, window::TimeInterval};
use andromeda_ngsi_ld_vocabulary::expanded::properties;
use andromeda_temporal::service::TemporalRange;
use chrono::{DateTime, Utc};
use std::{collections::HashSet, num::NonZeroU32};
use thiserror::Error;

/// How far the instances of one page of captured entities have been read.
#[derive(Debug)]
pub(crate) struct InstanceWalk {
    /// The interval the next page of instances is read over.
    interval: TimeInterval,
    /// The temporal property the window applies to.
    timeproperty: TimeProp,
    /// The instant the last partial page was cut at; every instance on the
    /// side of it the page held is captured. `None` until a page was cut.
    edge: Option<DateTime<Utc>>,
    /// The instances captured of every stream under the query's `lastN`;
    /// `None` when the query asks for no `lastN`.
    tally: Option<LastNTally>,
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
    /// The answer was cut no further along the window than the instant the
    /// walk resumed it at, so asking again would answer the same page.
    ///
    /// A Context Source that counts the instances on the inclusive lower bound
    /// of a resumed window into its page, or carries fewer of the instances
    /// one attribute holds at one instant than it holds, stalls so. The
    /// broker's own pages end past the instant they resume at; their walk
    /// keeps the check so that a page that did not would end the capture
    /// rather than repeat it.
    #[error(
        "a partial temporal answer was cut at {}, no further than the instant it was resumed at, so its history cannot be read past that instant",
        instant_text(*at)
    )]
    Stalled {
        /// The instant the page could not be read past.
        at: DateTime<Utc>,
    },
    /// The page was partial but named no instant to resume from: a range of
    /// the broker's own history holding no instant, or a Context Source's 206
    /// without a readable `Content-Range`.
    #[error("a partial temporal answer named no instant to resume from")]
    Unpositioned,
}

impl InstanceWalk {
    /// A walk over `interval`, positioned on `timeproperty`, of a query asking
    /// for `last_n` instances, if any.
    pub(crate) fn new(interval: TimeInterval, timeproperty: TimeProp, last_n: Option<NonZeroU32>) -> InstanceWalk {
        InstanceWalk {
            interval,
            timeproperty,
            edge: None,
            tally: last_n.map(LastNTally::new),
            captured: HashSet::new(),
        }
    }

    /// The interval the next page of instances is read over.
    pub(crate) const fn interval(&self) -> TimeInterval {
        self.interval
    }

    /// Leave out of `histories` what an earlier page of the walk captured, the
    /// instances a stream holds past the query's `lastN`, and an entity such a
    /// page captured that the page holds nothing new of.
    pub(crate) fn trim(&mut self, histories: &mut Vec<TemporalHistory>) {
        histories.retain_mut(|history| match history {
            TemporalHistory::Instances(document) => self.trim_document(document),
            TemporalHistory::Aggregates(_) => true,
        });
        // clone: the walk remembers the entities after the page is stored
        self.captured.extend(histories.iter().filter_map(TemporalHistory::id).cloned());
    }

    /// [`InstanceWalk::trim`] for one entity's `document`, saying whether
    /// anything of it is still to store.
    fn trim_document(&mut self, document: &mut TemporalDocument) -> bool {
        let held = document.id().is_some_and(|id| self.captured.contains(id));
        let (timeproperty, direction, edge) = (self.timeproperty, self.direction(), self.edge);
        document.retain_instances(|name, instance| {
            if is_scope(name) {
                return !held;
            }
            edge.is_none_or(|edge| instance.instant_on(timeproperty).is_some_and(|instant| !direction.holds(edge, instant)))
        });
        if let Some(tally) = &mut self.tally {
            tally.keep_lacking(document, timeproperty);
        }
        !held || document.instance_count() > 0
    }

    /// Take the range a page answered with, reading the rest of the window
    /// past the instant a partial page was cut at.
    ///
    /// # Errors
    ///
    /// Returns [`WalkRefusal`] when the rest of a partial page cannot be read.
    pub(crate) fn advance(&mut self, range: Option<&TemporalRange>) -> Result<WalkProgress, WalkRefusal> {
        let Some(range) = range else {
            return Ok(WalkProgress::Complete);
        };
        let direction = self.direction();
        // Clause 6.3.10: a forward range ends at the most recent instant of its
        // page and a backward one at the least recent.
        let cut = match (direction, range.observed.ok_or(WalkRefusal::Unpositioned)?) {
            (PageDirection::Forward, (_, most)) => most,
            (PageDirection::Backward { .. }, (least, _)) => least,
        };
        if self.edge.is_some_and(|edge| direction.nearer(cut, edge) == cut) {
            return Err(WalkRefusal::Stalled { at: cut });
        }
        self.edge = Some(cut);
        match rest_of_window(direction, self.interval, cut) {
            Some(interval) => {
                self.interval = interval;
                Ok(WalkProgress::Resumed)
            }
            None => Ok(WalkProgress::Complete),
        }
    }

    /// The direction a partial page of the query runs in: backwards under a
    /// `lastN`, forwards without one (clause 6.3.10).
    fn direction(&self) -> PageDirection {
        self.tally
            .as_ref()
            .map_or(PageDirection::Forward, |tally| PageDirection::Backward { requested: tally.last_n() })
    }
}

/// The window the rest of a page of `interval` paged in `direction` and cut
/// at `cut` is read over, or `None` when no instant of it remains: forwards
/// from the cut on, the cut included so that the instances sharing it are read
/// again and left out rather than skipped, and backwards before the cut.
pub(crate) fn rest_of_window(direction: PageDirection, interval: TimeInterval, cut: DateTime<Utc>) -> Option<TimeInterval> {
    match direction {
        PageDirection::Forward => Some(interval.resumed_at(cut)),
        PageDirection::Backward { .. } => interval.preceding(cut),
    }
}

/// Whether `name` is the reified Scope (clause 4.18).
pub(crate) fn is_scope(name: &AttributeName) -> bool {
    name.as_str() == properties::SCOPE.as_str()
}

#[cfg(test)]
mod tests {
    use crate::executor::instance_walk::{InstanceWalk, WalkProgress, WalkRefusal};
    use andromeda_ngsi_ld_entity::document::{EntityDocument, attribute_member::AttributeMember, instance_document::InstanceDocument, slot::Slot};
    use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, attribute_type::AttributeType, time::date_time::NgsiLdDateTime, time_prop::TimeProp};
    use andromeda_ngsi_ld_model::{
        entity_system_times::EntitySystemTimes,
        temporal::{temporal_document::TemporalDocument, temporal_history::TemporalHistory},
    };
    use andromeda_ngsi_ld_query::temporal::{instance_page::PageDirection, window::TimeInterval};
    use andromeda_ngsi_ld_vocabulary::expanded::properties;
    use andromeda_temporal::service::TemporalRange;
    use chrono::{DateTime, Utc};
    use indexmap::IndexMap;
    use pretty_assertions::assert_eq;
    use std::num::NonZeroU32;

    fn at(raw: &str) -> DateTime<Utc> {
        raw.parse::<NgsiLdDateTime>().expect("a valid instant").get()
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
            .observed_at(Some(Slot::Set(observed.parse::<NgsiLdDateTime>().expect("an instant"))))
            .build()
    }

    /// One entity's page: the `speed` instances observed at `observed`, and one
    /// Scope instance.
    fn page_of(entity: &str, observed: &[&str]) -> TemporalHistory {
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
        TemporalHistory::Instances(TemporalDocument::new(entity, EntitySystemTimes::default(), IndexMap::new()))
    }

    fn forward_cut(least: &str, most: &str) -> TemporalRange {
        TemporalRange {
            interval: TimeInterval::After(at("2024-01-01T00:00:00Z")),
            observed: Some((at(least), at(most))),
            direction: PageDirection::Forward,
            next: None,
        }
    }

    fn backward_cut(least: &str, most: &str) -> TemporalRange {
        TemporalRange {
            direction: PageDirection::Backward { requested: last_n() },
            ..forward_cut(least, most)
        }
    }

    fn last_n() -> NonZeroU32 {
        NonZeroU32::new(5).expect("a positive lastN")
    }

    /// The `speed` instants a page keeps, and how many Scope instances.
    fn kept(page: &TemporalHistory) -> (Vec<DateTime<Utc>>, usize) {
        let TemporalHistory::Instances(page) = page else {
            return (Vec::new(), 0);
        };
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
        let mut walk = InstanceWalk::new(TimeInterval::After(at("2024-01-01T00:00:00Z")), TimeProp::Observed, None);
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
        let mut walk = InstanceWalk::new(TimeInterval::After(at("2024-01-01T00:00:00Z")), TimeProp::Observed, None);
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
            None,
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
        let mut walk = InstanceWalk::new(TimeInterval::After(at("2024-01-01T00:00:00Z")), TimeProp::Observed, None);
        let cut = forward_cut("2024-01-01T00:00:00Z", "2024-01-02T00:00:00Z");
        walk.advance(Some(&cut)).expect("a resumable page");
        assert_eq!(
            walk.advance(Some(&cut)),
            Err(WalkRefusal::Stalled {
                at: at("2024-01-02T00:00:00Z")
            })
        );
    }

    /// Clauses 5.16.1.4 and 6.3.10: a `lastN` above the maximum is read in
    /// backward pages, each over the window ending where the previous one was
    /// cut, until the stream holds `lastN` instances and an answer is whole;
    /// the Scope is kept once and a page holding nothing new is left out.
    #[test]
    fn a_last_n_capture_follows_backward_pages_to_the_end_of_its_window() {
        let mut walk = InstanceWalk::new(TimeInterval::After(at("2024-01-01T00:00:00Z")), TimeProp::Observed, Some(last_n()));
        let answers = [
            (
                vec!["2024-01-08T00:00:00Z", "2024-01-09T00:00:00Z", "2024-01-10T00:00:00Z"],
                Some(backward_cut("2024-01-08T00:00:00Z", "2024-01-10T00:00:00Z")),
            ),
            (
                vec!["2024-01-05T00:00:00Z", "2024-01-06T00:00:00Z", "2024-01-07T00:00:00Z"],
                Some(backward_cut("2024-01-05T00:00:00Z", "2024-01-07T00:00:00Z")),
            ),
            (vec!["2024-01-03T00:00:00Z", "2024-01-04T00:00:00Z"], None),
        ];
        let mut intervals = vec![walk.interval()];
        let mut stored = Vec::new();
        let mut progress = Vec::new();
        for (observed, range) in answers {
            let mut histories = vec![page_of("urn:ngsi-ld:Vehicle:1", &observed)];
            walk.trim(&mut histories);
            stored.push(histories.iter().map(kept).collect::<Vec<_>>());
            progress.push(walk.advance(range.as_ref()).expect("a page the walk can resume past"));
            intervals.push(walk.interval());
        }
        assert_eq!(progress, vec![WalkProgress::Resumed, WalkProgress::Resumed, WalkProgress::Complete]);
        assert_eq!(
            intervals[..3],
            [
                TimeInterval::After(at("2024-01-01T00:00:00Z")),
                TimeInterval::Between {
                    from: at("2024-01-01T00:00:00Z"),
                    to: at("2024-01-08T00:00:00Z")
                },
                TimeInterval::Between {
                    from: at("2024-01-01T00:00:00Z"),
                    to: at("2024-01-05T00:00:00Z")
                },
            ]
        );
        assert_eq!(
            stored,
            vec![
                vec![(vec![at("2024-01-08T00:00:00Z"), at("2024-01-09T00:00:00Z"), at("2024-01-10T00:00:00Z")], 1)],
                vec![(vec![at("2024-01-06T00:00:00Z"), at("2024-01-07T00:00:00Z")], 0)],
                vec![],
            ]
        );
    }

    /// A backward page cut no earlier than the instant the walk resumed it at
    /// cannot be read past, as a stalled forward page cannot.
    #[test]
    fn a_backward_page_cut_no_earlier_than_the_previous_one_stalls_the_walk() {
        for later in ["2024-01-05T00:00:00Z", "2024-01-06T00:00:00Z"] {
            let mut walk = InstanceWalk::new(TimeInterval::After(at("2024-01-01T00:00:00Z")), TimeProp::Observed, Some(last_n()));
            walk.advance(Some(&backward_cut("2024-01-05T00:00:00Z", "2024-01-09T00:00:00Z")))
                .expect("a resumable page");
            assert_eq!(
                walk.advance(Some(&backward_cut(later, "2024-01-09T00:00:00Z"))),
                Err(WalkRefusal::Stalled { at: at(later) })
            );
        }
    }

    /// Clause 4.11: a backward page cut at the instant its window opens at
    /// leaves nothing of the window before it.
    #[test]
    fn a_backward_page_cut_at_the_opening_instant_ends_the_walk() {
        let mut walk = InstanceWalk::new(TimeInterval::After(at("2024-01-01T00:00:00Z")), TimeProp::Observed, Some(last_n()));
        assert_eq!(
            walk.advance(Some(&backward_cut("2024-01-01T00:00:00Z", "2024-01-09T00:00:00Z"))),
            Ok(WalkProgress::Complete)
        );
    }
}
