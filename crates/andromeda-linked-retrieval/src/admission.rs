//! Where a request may reference a Linked Entity (ETSI GS CIM 009 v1.9.1
//! clauses 5.6.21.4, 5.7.1.4, 5.7.2.4, 5.7.3.4, 5.7.4.4, 5.8.1.4, 5.14.4.4 and
//! 5.14.5.4).
//!
//! Three request members can reach across a Relationship into another entity:
//! a clause 4.21 sub-projection in `pick`/`omit`, which says how that entity
//! renders; a clause 4.9 `rel{...}` hop in `q`, which asks a question about it;
//! and a retrieving `join`, which asks for it. The specification states one
//! rule per operation for them, and [`admit`] is that rule, written once:
//!
//! - Retrieve Entity, Query Entities and the Entity Map for Query Entities
//!   bound a nesting and a hop by `join`/`joinLevel` (clauses 5.7.1.4, 5.7.2.4,
//!   5.14.4.4): without a retrieving join there is no Linked Entity to reach,
//!   and a reference deeper than the traversal walks is a "too deep query".
//! - Purge Entities refuses both outright (clause 5.6.21.4).
//! - The temporal operations refuse all three (clauses 5.7.3.4, 5.7.4.4,
//!   5.14.5.4), unless the `temporal_linked_retrieval` extension is enabled,
//!   when they follow the Core rule above.
//! - Subscriptions admit every reference (clause 5.8.1.4 adds a Subscription
//!   that passes the clause 5.2.12 checks, none of which bounds them).
//!
//! `@none` is the parameter's own default -- clause 4.5.23 says "By default, it
//! will be '@none'" -- so writing it means no more than leaving the parameter
//! out, and `joinLevel` is "only applicable if join parameter is 'flat', or
//! 'inline'". A request with `@none` therefore declares no traversal at all.

use crate::join::traversal_depth;
use andromeda_ngsi_ld_query::{filter::expression::Expr, projection::grammar::RawProjectionTerm};
use andromeda_ngsi_ld_representation::join_mode::JoinMode;
use derive_more::{Display, From};
use std::num::NonZeroU32;
use thiserror::Error;
use typed_builder::TypedBuilder;

/// Whether a temporal operation runs as the specification states it or with
/// the `temporal_linked_retrieval` extension enabled.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TemporalJoin {
    /// Clauses 5.7.3.4, 5.7.4.4 and 5.14.5.4: no Linked Entity retrieval.
    Spec,
    /// The extension: the Core rule of clauses 5.7.1.4 and 5.7.2.4 applies.
    Extended,
}

/// The operation a request is, as far as Linked Entity retrieval cares.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Display)]
pub enum LinkedOperation {
    /// Retrieve Entity (clause 5.7.1).
    #[display("Retrieve Entity")]
    RetrieveEntity,
    /// Query Entities (clause 5.7.2).
    #[display("Query Entities")]
    QueryEntities,
    /// Entity Map creation for Query Entities (clause 5.14.4).
    #[display("Entity Map for Query Entities")]
    EntityMapQuery,
    /// Purge Entities (clause 5.6.21).
    #[display("Purge Entities")]
    PurgeEntities,
    /// Retrieve Temporal Evolution of an Entity (clause 5.7.3).
    #[display("Retrieve Temporal Evolution of an Entity")]
    RetrieveTemporal(TemporalJoin),
    /// Query Temporal Evolution of Entities (clause 5.7.4).
    #[display("Query Temporal Evolution of Entities")]
    QueryTemporal(TemporalJoin),
    /// Entity Map creation for Query Temporal Evolution (clause 5.14.5).
    #[display("Entity Map for Query Temporal Evolution of Entities")]
    EntityMapTemporalQuery(TemporalJoin),
    /// Create or Update Subscription (clauses 5.8.1, 5.8.2).
    #[display("Subscription")]
    Subscription,
}

/// A request member that reaches into a Linked Entity's members.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Display)]
pub enum ReachingMember {
    /// A `pick` or `omit` term carrying a clause 4.21 sub-projection.
    #[display("pick/omit select Linked Entity members")]
    Projection,
    /// A `q` predicate whose attribute path crosses a Relationship.
    #[display("q traverses a Linked Entity")]
    Filter,
}

/// A request member that references a Linked Entity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Display, From)]
pub enum LinkedMember {
    /// A member that reaches into a Linked Entity's members.
    #[display("{_0}")]
    Reaching(ReachingMember),
    /// A `join` of `flat` or `inline`, which asks for the Linked Entities.
    #[display("join asks to retrieve Linked Entities")]
    Join,
}

/// One Linked Entity reference a request carries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkedReference {
    /// The deepest clause 4.21 nesting of `pick` and `omit`.
    Projection {
        /// How many Relationship hops the deepest nested term crosses.
        depth: NonZeroU32,
    },
    /// The deepest clause 4.9 hop chain of `q`.
    Filter {
        /// How many Relationship hops the deepest chain crosses.
        depth: NonZeroU32,
    },
    /// A retrieving `join` (`flat` or `inline`).
    JoinRequest,
}

impl LinkedReference {
    /// The projection reference `pick` and `omit` carry, if either nests.
    #[must_use]
    pub fn of_projection(pick: Option<&[RawProjectionTerm]>, omit: Option<&[RawProjectionTerm]>) -> Option<LinkedReference> {
        let depth = pick
            .unwrap_or_default()
            .iter()
            .chain(omit.unwrap_or_default())
            .map(RawProjectionTerm::linked_depth)
            .max()?;
        NonZeroU32::new(depth).map(|depth| LinkedReference::Projection { depth })
    }

    /// The filter reference `q` carries, if it hops.
    #[must_use]
    pub fn of_filter(q: Option<&Expr>) -> Option<LinkedReference> {
        NonZeroU32::new(q.map_or(0, Expr::linked_depth)).map(|depth| LinkedReference::Filter { depth })
    }

    /// The join reference `join` carries, if it retrieves.
    #[must_use]
    pub const fn of_join(join: Option<JoinMode>) -> Option<LinkedReference> {
        match join {
            Some(JoinMode::Flat | JoinMode::Inline) => Some(LinkedReference::JoinRequest),
            Some(JoinMode::None) | None => None,
        }
    }

    /// The request member this reference is.
    #[must_use]
    pub const fn member(self) -> LinkedMember {
        match self {
            LinkedReference::Projection { .. } => LinkedMember::Reaching(ReachingMember::Projection),
            LinkedReference::Filter { .. } => LinkedMember::Reaching(ReachingMember::Filter),
            LinkedReference::JoinRequest => LinkedMember::Join,
        }
    }
}

/// A Linked Entity reference the operation does not admit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum AdmissionError {
    /// The operation retrieves no Linked Entity at all.
    #[error("{operation} does not retrieve Linked Entities, but {member}")]
    NotRetrievable {
        /// The operation the request is.
        operation: LinkedOperation,
        /// The member that referenced a Linked Entity.
        member: LinkedMember,
    },
    /// The member reaches into a Linked Entity, but the request asks for no
    /// retrieving join.
    #[error("{0}, but the request asks for no join to retrieve them")]
    WithoutJoin(ReachingMember),
    /// The member reaches deeper than the requested traversal walks.
    #[error("{member} {depth} Relationship(s) deep, beyond joinLevel={join_level}")]
    DeeperThanJoinLevel {
        /// The member that reaches too deep.
        member: ReachingMember,
        /// How many Relationship hops it crosses.
        depth: NonZeroU32,
        /// How many hops the requested traversal walks.
        join_level: NonZeroU32,
    },
}

/// Admit one Linked Entity reference of a request for `operation`.
///
/// # Errors
///
/// Returns [`AdmissionError::NotRetrievable`] when the operation retrieves no
/// Linked Entity, [`AdmissionError::WithoutJoin`] when a nesting or a hop
/// meets no retrieving join, and [`AdmissionError::DeeperThanJoinLevel`] when
/// it reaches past the traversal.
pub fn admit(operation: LinkedOperation, reference: LinkedReference, join: Option<JoinMode>, join_level: Option<NonZeroU32>) -> Result<(), AdmissionError> {
    match (operation, reference) {
        (LinkedOperation::Subscription, _)
        | (
            LinkedOperation::RetrieveEntity
            | LinkedOperation::QueryEntities
            | LinkedOperation::EntityMapQuery
            | LinkedOperation::PurgeEntities
            | LinkedOperation::RetrieveTemporal(TemporalJoin::Extended)
            | LinkedOperation::QueryTemporal(TemporalJoin::Extended)
            | LinkedOperation::EntityMapTemporalQuery(TemporalJoin::Extended),
            LinkedReference::JoinRequest,
        ) => Ok(()),
        (LinkedOperation::PurgeEntities, LinkedReference::Projection { .. } | LinkedReference::Filter { .. })
        | (
            LinkedOperation::RetrieveTemporal(TemporalJoin::Spec)
            | LinkedOperation::QueryTemporal(TemporalJoin::Spec)
            | LinkedOperation::EntityMapTemporalQuery(TemporalJoin::Spec),
            _,
        ) => Err(AdmissionError::NotRetrievable {
            operation,
            member: reference.member(),
        }),
        (
            LinkedOperation::RetrieveEntity
            | LinkedOperation::QueryEntities
            | LinkedOperation::EntityMapQuery
            | LinkedOperation::RetrieveTemporal(TemporalJoin::Extended)
            | LinkedOperation::QueryTemporal(TemporalJoin::Extended)
            | LinkedOperation::EntityMapTemporalQuery(TemporalJoin::Extended),
            LinkedReference::Projection { depth },
        ) => within_join(ReachingMember::Projection, depth, join, join_level),
        (
            LinkedOperation::RetrieveEntity
            | LinkedOperation::QueryEntities
            | LinkedOperation::EntityMapQuery
            | LinkedOperation::RetrieveTemporal(TemporalJoin::Extended)
            | LinkedOperation::QueryTemporal(TemporalJoin::Extended)
            | LinkedOperation::EntityMapTemporalQuery(TemporalJoin::Extended),
            LinkedReference::Filter { depth },
        ) => within_join(ReachingMember::Filter, depth, join, join_level),
    }
}

/// The Linked Entity references of one request.
#[derive(Debug, Clone, Copy, Default, TypedBuilder)]
#[builder(field_defaults(default = Default::default()))]
pub struct LinkedRequest<'r> {
    /// The clause 4.21 `pick` terms.
    pub pick: Option<&'r [RawProjectionTerm]>,
    /// The clause 4.21 `omit` terms.
    pub omit: Option<&'r [RawProjectionTerm]>,
    /// The clause 4.9 `q` filter.
    pub q: Option<&'r Expr>,
    /// The clause 4.5.23 `join`.
    pub join: Option<JoinMode>,
    /// The clause 4.5.23 `joinLevel`.
    pub join_level: Option<NonZeroU32>,
}

/// Admit every Linked Entity reference of `request` for `operation`: the
/// retrieving `join`, then the projection, then the filter.
///
/// # Errors
///
/// Returns the first [`AdmissionError`] [`admit`] raises.
pub fn admit_request(operation: LinkedOperation, request: &LinkedRequest<'_>) -> Result<(), AdmissionError> {
    [
        LinkedReference::of_join(request.join),
        LinkedReference::of_projection(request.pick, request.omit),
        LinkedReference::of_filter(request.q),
    ]
    .into_iter()
    .flatten()
    .try_for_each(|reference| admit(operation, reference, request.join, request.join_level))
}

/// The Core rule: a reaching member needs a retrieving join that walks at
/// least as deep as it reaches (clauses 5.7.1.4, 5.7.2.4).
fn within_join(member: ReachingMember, depth: NonZeroU32, join: Option<JoinMode>, join_level: Option<NonZeroU32>) -> Result<(), AdmissionError> {
    if LinkedReference::of_join(join).is_none() {
        return Err(AdmissionError::WithoutJoin(member));
    }
    let join_level = traversal_depth(join_level);
    if depth > join_level {
        return Err(AdmissionError::DeeperThanJoinLevel { member, depth, join_level });
    }
    Ok(())
}

#[cfg(test)]
mod tests;
