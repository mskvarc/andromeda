//! The non-local half of Linked Entity resolution for the temporal API.
//!
//! Clause 4.5.23.1 makes a Relationship's `objectType` annotation the statement
//! that its object is an Internal Linked Entity: an entity of the declared type
//! that exists somewhere in the distributed system, not necessarily in the
//! store the request reached. Applied to the temporal API the statement is the
//! same, so the traversal has the same two halves — the local history answers
//! for what this broker recorded, and whatever holds the rest answers through
//! this port.
//!
//! What differs from the entity join is that a temporal answer is meaningless
//! without the window it was taken over. A Linked Entity fetched from another
//! broker must be fetched over *this* request's window, or the response would
//! splice two different slices of history together, so the window travels with
//! every call as a [`TemporalLinkedWindow`].
//!
//! The port is deliberately blind to how the remainder is reached. A broker
//! composed without federation supplies no implementation at all and resolves
//! links purely against its own history; a federated composition supplies one
//! that forwards to the registered context sources.

use crate::error::TemporalError;
use andromeda_jsonld::context_spec::ContextSpec;
use andromeda_linked_retrieval::source::{RemoteReach, RemoteResolution};
use andromeda_ngsi_ld_lexicon::{aggr_method::AggrMethod, identifier::Identifier, temporal_format::TemporalFormat, time_prop::TimeProp, time_rel::TimeRel};
use andromeda_ngsi_ld_model::temporal::temporal_history::TemporalHistory;
use andromeda_ngsi_ld_query::temporal::aggregation_period::AggregationPeriod;
use andromeda_ngsi_ld_request::{query::temporal::TemporalQueryParameters, representation_parameter::WrittenValue};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use std::{fmt, num::NonZeroU32};

/// The slice of history a forwarded Linked Entity retrieval must reproduce.
///
/// Only the members that define *which* observations are selected and *how*
/// they are represented belong here. The request's entity filters, pagination
/// and projection deliberately do not: a linked retrieval names one entity by
/// identifier, so filtering it again would be meaningless, and paginating it
/// would silently truncate the join.
#[derive(Debug, Clone, PartialEq)]
pub struct TemporalLinkedWindow {
    /// How the window relates to its anchor (clause 4.11 `timerel`).
    pub timerel: Option<TimeRel>,
    /// The instant the window is anchored at (`timeAt`).
    pub time_at: Option<DateTime<Utc>>,
    /// The closing instant, present when `timerel=between` (`endTimeAt`).
    pub end_time_at: Option<DateTime<Utc>>,
    /// The temporal property the window applies to (clause 4.8).
    pub timeproperty: TimeProp,
    /// How many instances per attribute the answer carries (`lastN`).
    pub last_n: Option<NonZeroU32>,
    /// The representation the answer takes (clause 6.3.12 `format`).
    pub format: TemporalFormat,
    /// The aggregation methods, when the representation is aggregated.
    pub aggr_methods: Option<Vec<AggrMethod>>,
    /// The aggregation bucket width, when the representation is aggregated.
    pub aggr_period_duration: Option<AggregationPeriod>,
}

impl TemporalLinkedWindow {
    /// The window the given request selects.
    ///
    /// Taken from the request rather than from the compiled query because a
    /// forwarded retrieval is spelled in the same vocabulary the client used:
    /// the remote broker compiles it itself, against its own context.
    #[must_use]
    pub fn of_request(params: &TemporalQueryParameters) -> TemporalLinkedWindow {
        TemporalLinkedWindow {
            timerel: params.timerel,
            time_at: params.time_at,
            end_time_at: params.end_time_at,
            timeproperty: params.timeproperty_or_default(),
            last_n: params.last_n,
            format: params.selected_format(),
            aggr_methods: params.aggr_methods.clone(), // clone: the window outlives the borrowed request
            aggr_period_duration: params.aggr_period_duration,
        }
    }

    /// The request a Linked Entity's own history is read with.
    ///
    /// This is the local counterpart of the forwarded query string: the window
    /// and nothing else. Because every member the struct does not hold is left
    /// at its default, an `attrs` or `datasetId` that narrowed the roots cannot
    /// leak into the traversal and hide a linked entity's own attributes.
    #[must_use]
    pub fn to_parameters(&self) -> TemporalQueryParameters {
        TemporalQueryParameters {
            timerel: self.timerel,
            time_at: self.time_at,
            end_time_at: self.end_time_at,
            timeproperty: Some(self.timeproperty),
            last_n: self.last_n,
            format: Some(WrittenValue::Supported(self.format)),
            aggr_methods: self.aggr_methods.clone(), // clone: the derived request owns its methods
            aggr_period_duration: self.aggr_period_duration,
            ..TemporalQueryParameters::default()
        }
    }
}

/// Fetches the temporal evolution of an `objectType`-annotated Linked Entity
/// whose history this broker does not hold (clause 4.5.23.1).
///
/// The temporal operations retrieve Linked Entities only as an extension, so
/// a holder resolves the levels below the entity itself only when it said it
/// serves that extension; [`Self::remote_reach`] reports whether it did, and
/// the traversal asks for the depth that answer allows.
#[async_trait]
pub trait RemoteTemporalLinkedSource: fmt::Debug + Send + Sync + 'static {
    /// How far the holders of `id` resolve on their own: the whole remaining
    /// depth only when every source a retrieval of `id` reaches advertised the
    /// temporal Linked Entity retrieval extension, the entity alone otherwise.
    #[must_use = "how far a holder resolves must be awaited"]
    async fn remote_reach(&self, id: &Identifier, context: &ContextSpec) -> RemoteReach;

    /// Fetch the temporal evolution of the entity `id` names, plus the Linked
    /// Entities reachable from it within the remaining depth.
    ///
    /// `remaining_depth` counts the traversal levels still to be resolved
    /// *including* the level `id` itself sits on, so a value of one means the
    /// entity alone is wanted and nothing it links to. It is never more than
    /// one unless [`Self::remote_reach`] answered
    /// [`RemoteReach::Subgraph`] for `id`.
    ///
    /// `visited` lists, in traversal order, the identifiers the caller has
    /// already resolved or deliberately excluded, and never contains `id`. An
    /// implementation passes them on as the entities the requester already
    /// holds (clause 4.5.23 `containedBy`), which keeps a link back into the
    /// already-walked part of the graph from being fetched twice.
    ///
    /// # Errors
    ///
    /// Returns [`TemporalError`] when the remainder of the system cannot be
    /// consulted at all. A source that merely fails to produce a temporal
    /// representation returns none and a failure describing
    /// why: clause 4.5.23 leaves an unresolvable Relationship as a plain URI
    /// rather than failing the retrieval.
    #[must_use = "non-local Linked Entity resolutions must be awaited"]
    async fn fetch_linked_temporal(
        &self,
        id: &Identifier,
        remaining_depth: NonZeroU32,
        visited: &[Identifier],
        window: &TemporalLinkedWindow,
        context: &ContextSpec,
    ) -> Result<RemoteResolution<TemporalHistory>, TemporalError>;
}
