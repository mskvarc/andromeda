//! The Temporal Evolution of one entity as the operations gather it: the
//! instances it took over time, still in the context-independent form, or the
//! aggregates over them (ETSI GS CIM 009 clauses 4.5.7, 4.5.19 and 5.7.4.4).
//!
//! Which of the two a read yields is decided by the `format` of the request,
//! and every holder of the entity's history answers in it, so the histories
//! the holders contributed are put together as that representation puts them
//! together: instances are united, aggregates are never combined.

use crate::temporal::{
    aggregated_entity::AggregatedTemporalEntity,
    temporal_document::{MergeMode, TemporalDocument},
};
use andromeda_ngsi_ld_lexicon::{identifier::Identifier, time_prop::TimeProp};
use chrono::{DateTime, Utc};
use std::num::NonZeroU32;

/// The history of one entity, before it is compacted under a request context.
#[derive(Clone, Debug)]
pub enum TemporalHistory {
    /// The instances of its attributes, in the expanded form.
    Instances(TemporalDocument),
    /// The aggregates over them, which carry the names of the request context
    /// already: an aggregation key is no IRI.
    Aggregates(AggregatedTemporalEntity),
}

impl TemporalHistory {
    /// The entity's identifier, when the history names it.
    #[must_use]
    pub const fn id(&self) -> Option<&Identifier> {
        match self {
            TemporalHistory::Instances(document) => document.id(),
            TemporalHistory::Aggregates(entity) => Some(&entity.id),
        }
    }

    /// Put `other`'s history of the same entity beside this one, as `mode`
    /// says. Histories in different representations cannot be put together, so
    /// the one held stays.
    pub fn merge(&mut self, other: TemporalHistory, mode: MergeMode, timeproperty: TimeProp) {
        match (self, other) {
            (TemporalHistory::Instances(held), TemporalHistory::Instances(received)) => held.merge(received, mode, timeproperty),
            (TemporalHistory::Aggregates(held), TemporalHistory::Aggregates(received)) => held.merge(received, mode),
            (TemporalHistory::Instances(_), TemporalHistory::Aggregates(_)) | (TemporalHistory::Aggregates(_), TemporalHistory::Instances(_)) => {}
        }
    }

    /// Keep, for every attribute, the `last_n` instances with the most recent
    /// instant on `timeproperty`. An aggregate has no instance to keep.
    pub fn keep_last(&mut self, timeproperty: TimeProp, last_n: NonZeroU32) {
        match self {
            TemporalHistory::Instances(document) => document.keep_last(timeproperty, last_n),
            TemporalHistory::Aggregates(_) => {}
        }
    }

    /// Keep only the instances whose instant on `timeproperty` `keep` admits.
    /// An aggregate has no instance to keep.
    pub fn retain_instants(&mut self, timeproperty: TimeProp, keep: impl Fn(DateTime<Utc>) -> bool) {
        match self {
            TemporalHistory::Instances(document) => document.retain_instants(timeproperty, keep),
            TemporalHistory::Aggregates(_) => {}
        }
    }

    /// The least and most recent instants any instance carries on
    /// `timeproperty`; an aggregate carries none.
    #[must_use]
    pub fn instant_span(&self, timeproperty: TimeProp) -> Option<(DateTime<Utc>, DateTime<Utc>)> {
        match self {
            TemporalHistory::Instances(document) => document.instant_span(timeproperty),
            TemporalHistory::Aggregates(_) => None,
        }
    }

    /// How many instances the history holds; an aggregate holds none.
    #[must_use]
    pub fn instance_count(&self) -> usize {
        match self {
            TemporalHistory::Instances(document) => document.instance_count(),
            TemporalHistory::Aggregates(_) => 0,
        }
    }
}
