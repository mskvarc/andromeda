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
//! again from the first page, over the window resumed at the least instant any
//! partial answer of the pass was cut at, until a pass holds no partial answer.
//!
//! A partial answer holds every instance at or before its cut of each
//! attribute it renders, and a complete one every instance of the window.
//! What each registration answered for each entity and attribute is
//! remembered, so a later pass keeps only the instances past it. An entity a
//! partial answer left out, every instance of which lies past the cut, is kept
//! whole on the pass that reaches it. The Scope of an entity has no position
//! on the requested axis and travels whole on every page (clause 4.18), so it
//! is kept once.

use crate::executor::instance_walk::{WalkRefusal, is_scope};
use andromeda_federation::outcome::ForwardedTemporalEntities;
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, identifier::Identifier, time_prop::TimeProp};
use andromeda_ngsi_ld_model::temporal::{temporal_document::TemporalDocument, temporal_history::TemporalHistory};
use andromeda_ngsi_ld_query::temporal::window::TimeInterval;
use chrono::{DateTime, Utc};
use std::{collections::HashMap, num::NonZeroU32};

/// How much of one attribute of one source entity a pass kept.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Reach {
    /// Every instance at or before the instant.
    Through(DateTime<Utc>),
    /// Every instance of the window.
    Whole,
}

/// What each registration answered of the entities it holds, attribute by
/// attribute.
type Reached = HashMap<Identifier, HashMap<Identifier, Vec<(AttributeName, Reach)>>>;

/// How far the sources' answers to one captured temporal query have been read.
#[derive(Debug)]
pub(crate) struct SourceInstanceWalk {
    /// The window the current pass asks the sources over.
    interval: TimeInterval,
    /// The temporal property the window applies to.
    timeproperty: TimeProp,
    /// The query's `lastN`, under which a partial answer pages backwards
    /// (clause 6.3.10).
    last_n: Option<NonZeroU32>,
    /// The least instant a partial answer of the current pass was cut at;
    /// `None` while every answer of the pass is complete.
    pass_cut: Option<DateTime<Utc>>,
    /// The instant the current pass resumed at; `None` on the first pass.
    resumed_at: Option<DateTime<Utc>>,
    /// What each registration answered for each entity, per attribute.
    reached: Reached,
}

impl SourceInstanceWalk {
    /// A walk of the sources over `interval`, positioned on `timeproperty`,
    /// for a query asking for `last_n` instances, if any.
    pub(crate) fn new(interval: TimeInterval, timeproperty: TimeProp, last_n: Option<NonZeroU32>) -> SourceInstanceWalk {
        SourceInstanceWalk {
            interval,
            timeproperty,
            last_n,
            pass_cut: None,
            resumed_at: None,
            reached: HashMap::new(),
        }
    }

    /// Leave out of `answers` what an earlier page captured of each entity,
    /// and an entity such a page captured that the page holds nothing new of,
    /// then remember what each answer holds.
    ///
    /// # Errors
    ///
    /// Returns [`WalkRefusal::Backward`] for a partial answer to a query with
    /// `lastN`.
    pub(crate) fn trim(&mut self, answers: &mut [ForwardedTemporalEntities]) -> Result<(), WalkRefusal> {
        answers.iter_mut().try_for_each(|answer| {
            let cut = answer.least_range_end();
            self.trim_answer(answer.registration_id.as_ref(), &mut answer.entities, cut)
        })
    }

    /// [`SourceInstanceWalk::trim`] for the `entities` one `registration`
    /// answered, cut at `cut` when the answer is partial.
    fn trim_answer(&mut self, registration: Option<&Identifier>, entities: &mut Vec<TemporalHistory>, cut: Option<DateTime<Utc>>) -> Result<(), WalkRefusal> {
        if cut.is_some() && self.last_n.is_some() {
            return Err(WalkRefusal::Backward);
        }
        self.pass_cut = self.pass_cut.into_iter().chain(cut).min();
        let Some(registration) = registration else {
            return Ok(());
        };
        let reach = cut.map_or(Reach::Whole, Reach::Through);
        let timeproperty = self.timeproperty;
        // clone: the walk remembers the registration past its answer
        let reached = self.reached.entry(registration.clone()).or_default();
        entities.retain_mut(|history| match history {
            TemporalHistory::Instances(document) => keep_unreached(reached, document, reach, timeproperty),
            TemporalHistory::Aggregates(_) => true,
        });
        Ok(())
    }

    /// End the current pass: the window the next one reads, or `None` when
    /// every answer of the pass was complete.
    ///
    /// # Errors
    ///
    /// Returns [`WalkRefusal::Stalled`] when the pass was cut no later than
    /// the instant it resumed at: a source carries fewer of the instances one
    /// attribute holds there than it holds, or counts those on the inclusive
    /// lower bound into its page.
    pub(crate) fn next_pass(&mut self) -> Result<Option<TimeInterval>, WalkRefusal> {
        let Some(cut) = self.pass_cut.take() else {
            return Ok(None);
        };
        if self.resumed_at.is_some_and(|resumed_at| cut <= resumed_at) {
            return Err(WalkRefusal::Stalled { at: cut });
        }
        self.resumed_at = Some(cut);
        self.interval = self.interval.resumed_at(cut);
        Ok(Some(self.interval))
    }
}

/// Leave out of `document` the instances `entities` records as kept, and say
/// whether anything of it is still to store; then record what it holds as kept
/// up to `reach`.
fn keep_unreached(
    entities: &mut HashMap<Identifier, Vec<(AttributeName, Reach)>>,
    document: &mut TemporalDocument,
    reach: Reach,
    timeproperty: TimeProp,
) -> bool {
    let held = document.id().and_then(|id| entities.get(id));
    if let Some(attributes) = held {
        document.retain_instances(|name, instance| match reach_of(attributes, name) {
            None => true,
            Some(Reach::Whole) => false,
            Some(Reach::Through(through)) => instance.instant_on(timeproperty).is_some_and(|instant| instant > through),
        });
        if document.instance_count() == 0 {
            return false;
        }
    }
    let Some(id) = document.id() else {
        return true;
    };
    if let Some(attributes) = entities.get_mut(id) {
        record_reach(attributes, document, reach);
    } else {
        let mut attributes = Vec::new();
        record_reach(&mut attributes, document, reach);
        // clone: the walk remembers the entity past the answer it came in
        entities.insert(id.clone(), attributes);
    }
    true
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
fn record_reach(attributes: &mut Vec<(AttributeName, Reach)>, document: &TemporalDocument, reach: Reach) {
    for (name, member) in document.entity().attributes() {
        if member.instances().is_empty() {
            continue;
        }
        let reach = if is_scope(name) { Reach::Whole } else { reach };
        match attributes.iter_mut().find(|(held, _)| held == name) {
            Some((_, held)) => *held = (*held).max(reach),
            // clone: the walk remembers the attribute past the answer it came in
            None => attributes.push((name.clone(), reach)),
        }
    }
}

#[cfg(test)]
mod tests;
