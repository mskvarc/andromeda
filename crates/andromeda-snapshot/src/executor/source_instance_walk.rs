//! Reading what the Context Sources answer a captured temporal query to the
//! end of its window (clauses 5.16.1.4 and 6.3.10).
//!
//! A source pages its answer in two dimensions: its entities, by the offset
//! the capture asks for, and the instances of each, which a partial answer
//! cuts at the instant its `Content-Range` ends at. One offset page cannot be
//! asked again over the rest of the window: clause 5.7.4.4 removes from the
//! result every entity with no instance in the interval, so the same offset
//! over a later window names other entities. The sources are walked in passes
//! instead: once every offset page of a pass is read, the next pass walks them
//! again from the first page, over the rest of the window past the cut any
//! partial answer of the pass reached least far, until a pass holds no partial
//! answer.
//!
//! Without `lastN` an answer runs forwards: a partial one holds every instance
//! at or before its cut of each attribute it renders, and the next pass opens
//! at the earliest cut of the pass. With `lastN` it runs backwards (clause
//! 6.3.10): a partial one holds every instance at or after its cut, the least
//! recent instant it answered, and the next pass asks the same `lastN` over the
//! window ending at the latest cut of the pass. A complete answer holds every
//! instance of the window. What each registration answered for each entity and
//! attribute is remembered, so a later pass keeps only the instances past it;
//! under `lastN`, each stream of what a registration answered keeps only the
//! most recent instances it still lacks to reach `lastN`. An entity a partial
//! answer left out, every instance of which lies past the cut, is kept whole on
//! the pass that reaches it. The Scope of an entity has no position on the
//! requested axis and travels whole on every page (clause 4.18), so it is kept
//! once.
//!
//! A partial answer whose `Content-Range` is missing or cannot be read names
//! no instant the rest of its window can be asked from, so no pass can read
//! it to the end, and the walk refuses it on whichever page or pass it comes.

use crate::executor::{
    instance_walk::{WalkRefusal, is_scope, rest_of_window},
    last_n_tally::LastNTally,
};
use andromeda_federation::{outcome::ForwardedTemporalEntities, partial_page::PartialPage};
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, identifier::Identifier, time_prop::TimeProp};
use andromeda_ngsi_ld_model::temporal::{temporal_document::TemporalDocument, temporal_history::TemporalHistory};
use andromeda_ngsi_ld_query::temporal::{instance_page::PageDirection, window::TimeInterval};
use chrono::{DateTime, Utc};
use std::{collections::HashMap, num::NonZeroU32};

/// How much of one attribute of one source entity a pass kept.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Reach {
    /// Every instance on the side of the instant a partial answer holds: at
    /// or before it forwards, at or after it backwards.
    Cut(DateTime<Utc>),
    /// Every instance of the window.
    Whole,
}

impl Reach {
    /// The further of this reach and `other` in `direction`.
    fn further(self, other: Reach, direction: PageDirection) -> Reach {
        match (self, other) {
            (Reach::Whole, _) | (_, Reach::Whole) => Reach::Whole,
            (Reach::Cut(held), Reach::Cut(cut)) if direction.nearer(held, cut) == held => Reach::Cut(cut),
            (Reach::Cut(held), Reach::Cut(_)) => Reach::Cut(held),
        }
    }
}

/// What one registration answered of the entities it holds, attribute by
/// attribute, and under `lastN` how many instances of each stream it kept.
#[derive(Debug)]
struct Answered {
    /// How far each attribute of each entity was answered.
    reached: HashMap<Identifier, Vec<(AttributeName, Reach)>>,
    /// The instances kept of each stream under the query's `lastN`; `None`
    /// when the query asks for no `lastN`.
    tally: Option<LastNTally>,
}

/// How far the sources' answers to one captured temporal query have been read.
#[derive(Debug)]
pub(crate) struct SourceInstanceWalk {
    /// The window the current pass asks the sources over.
    interval: TimeInterval,
    /// The temporal property the window applies to.
    timeproperty: TimeProp,
    /// The direction a partial answer runs in: backwards under the query's
    /// `lastN`, forwards without one (clause 6.3.10).
    direction: PageDirection,
    /// The cut of the current pass reached least far; `None` while every
    /// answer of the pass is complete.
    pass_cut: Option<DateTime<Utc>>,
    /// The instant the current pass resumed at; `None` on the first pass.
    resumed_at: Option<DateTime<Utc>>,
    /// What each registration answered.
    answered: HashMap<Identifier, Answered>,
}

impl SourceInstanceWalk {
    /// A walk of the sources over `interval`, positioned on `timeproperty`,
    /// for a query asking for `last_n` instances, if any.
    pub(crate) fn new(interval: TimeInterval, timeproperty: TimeProp, last_n: Option<NonZeroU32>) -> SourceInstanceWalk {
        SourceInstanceWalk {
            interval,
            timeproperty,
            direction: last_n.map_or(PageDirection::Forward, |requested| PageDirection::Backward { requested }),
            pass_cut: None,
            resumed_at: None,
            answered: HashMap::new(),
        }
    }

    /// Leave out of `answers` what an earlier page captured of each entity,
    /// the instances a stream holds past the query's `lastN`, and an entity
    /// such a page captured that the page holds nothing new of, then remember
    /// what each answer holds.
    ///
    /// # Errors
    ///
    /// Returns [`WalkRefusal::Unpositioned`] for an answer a page of which was
    /// cut short at no readable range.
    pub(crate) fn trim(&mut self, answers: &mut [ForwardedTemporalEntities]) -> Result<(), WalkRefusal> {
        answers.iter_mut().try_for_each(|answer| {
            if answer.partial_pages.iter().any(PartialPage::is_unranged) {
                return Err(WalkRefusal::Unpositioned);
            }
            let cut = answer.nearest_range_end(self.direction);
            self.trim_answer(answer.registration_id.as_ref(), &mut answer.entities, cut);
            Ok(())
        })
    }

    /// [`SourceInstanceWalk::trim`] for the `entities` one `registration`
    /// answered, cut at `cut` when the answer is partial.
    fn trim_answer(&mut self, registration: Option<&Identifier>, entities: &mut Vec<TemporalHistory>, cut: Option<DateTime<Utc>>) {
        let direction = self.direction;
        self.pass_cut = self.pass_cut.into_iter().chain(cut).reduce(|left, right| direction.nearer(left, right));
        let Some(registration) = registration else {
            return;
        };
        let reach = cut.map_or(Reach::Whole, Reach::Cut);
        let timeproperty = self.timeproperty;
        // clone: the walk remembers the registration past its answer
        let answered = self.answered.entry(registration.clone()).or_insert_with(|| Answered {
            reached: HashMap::new(),
            tally: requested_last_n(direction).map(LastNTally::new),
        });
        entities.retain_mut(|history| match history {
            TemporalHistory::Instances(document) => answered.keep_unreached(document, reach, timeproperty, direction),
            TemporalHistory::Aggregates(_) => true,
        });
    }

    /// End the current pass: the window the next one reads, or `None` when
    /// every answer of the pass was complete or no instant of the window is
    /// left past its cut.
    ///
    /// # Errors
    ///
    /// Returns [`WalkRefusal::Stalled`] when the pass was cut no further than
    /// the instant it resumed at: a source carries fewer of the instances one
    /// attribute holds there than it holds, or counts those on the inclusive
    /// lower bound into its page.
    pub(crate) fn next_pass(&mut self) -> Result<Option<TimeInterval>, WalkRefusal> {
        let Some(cut) = self.pass_cut.take() else {
            return Ok(None);
        };
        if self.resumed_at.is_some_and(|resumed_at| self.direction.nearer(cut, resumed_at) == cut) {
            return Err(WalkRefusal::Stalled { at: cut });
        }
        self.resumed_at = Some(cut);
        let Some(interval) = rest_of_window(self.direction, self.interval, cut) else {
            return Ok(None);
        };
        self.interval = interval;
        Ok(Some(interval))
    }
}

/// The `lastN` a walk in `direction` reads, if any.
const fn requested_last_n(direction: PageDirection) -> Option<NonZeroU32> {
    match direction {
        PageDirection::Forward => None,
        PageDirection::Backward { requested } => Some(requested),
    }
}

impl Answered {
    /// Leave out of `document` the instances this registration already
    /// answered, and under `lastN` those a stream holds past it, and say
    /// whether anything of it is still to store; then record what it holds as
    /// answered up to `reach`.
    fn keep_unreached(&mut self, document: &mut TemporalDocument, reach: Reach, timeproperty: TimeProp, direction: PageDirection) -> bool {
        let held = document.id().and_then(|id| self.reached.get(id));
        let known = held.is_some();
        if let Some(attributes) = held {
            document.retain_instances(|name, instance| match reach_of(attributes, name) {
                None => true,
                Some(Reach::Whole) => false,
                Some(Reach::Cut(cut)) => instance.instant_on(timeproperty).is_some_and(|instant| !direction.holds(cut, instant)),
            });
            if document.instance_count() == 0 {
                return false;
            }
        }
        if let Some(id) = document.id() {
            if let Some(attributes) = self.reached.get_mut(id) {
                record_reach(attributes, document, reach, direction);
            } else {
                let mut attributes = Vec::new();
                record_reach(&mut attributes, document, reach, direction);
                // clone: the walk remembers the entity past the answer it came in
                self.reached.insert(id.clone(), attributes);
            }
        }
        if let Some(tally) = &mut self.tally {
            tally.keep_lacking(document, timeproperty);
        }
        !known || document.instance_count() > 0
    }
}

/// How much of `name` was kept, if any of it was.
fn reach_of(attributes: &[(AttributeName, Reach)], name: &AttributeName) -> Option<Reach> {
    attributes.iter().find(|(held, _)| held == name).map(|(_, reach)| *reach)
}

/// Raise to `reach` what `attributes` records of every attribute `document`
/// holds an instance of; its Scope is kept whole.
///
/// An attribute whose every answered instance was left out keeps the reach it
/// had: the answer holds every instance up to its cut, so none lies between.
fn record_reach(attributes: &mut Vec<(AttributeName, Reach)>, document: &TemporalDocument, reach: Reach, direction: PageDirection) {
    for (name, member) in document.entity().attributes() {
        if member.instances().is_empty() {
            continue;
        }
        let reach = if is_scope(name) { Reach::Whole } else { reach };
        match attributes.iter_mut().find(|(held, _)| held == name) {
            Some((_, held)) => *held = held.further(reach, direction),
            // clone: the walk remembers the attribute past the answer it came in
            None => attributes.push((name.clone(), reach)),
        }
    }
}

#[cfg(test)]
mod tests;
