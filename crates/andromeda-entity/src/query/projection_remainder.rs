//! What a clause 4.21 projection leaves of an entity, and the operation's
//! answer when it leaves nothing.
//!
//! ETSI GS CIM 009 v1.9.1 clause 5.7.1.4 states the rule for the selection
//! spelled `attrs`: "If the optional Attribute list is present and the NGSI-LD
//! endpoint does know about a matching Entity for the Entity ID, but this
//! Entity does not have any of the Attributes in the Attribute list, then an
//! error of type `ResourceNotFound` shall be raised." Tables 6.4.3.2-1 and
//! 5.2.14.1-1 make `attrs` "a synonym for pick, except that id, type, scope
//! are not allowed", so a `pick` that names no member the entity has is the
//! very same failure; and clause 5.7.1.5 reduces the Entity "down to only
//! contain the defined Entity members", so an `omit` naming every member it
//! has leaves nothing to answer with either.
//!
//! A query answers the same question as a selection instead: an Entity the
//! projection would reduce to nothing is not selected at all, so the count
//! and the page are taken over the entities that keep a member. Both read the
//! one statement of the rule, [`ProjectedMembers::admits`].

use andromeda_ngsi_ld_entity::entity_state::EntityState;
use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
use andromeda_ngsi_ld_query::projection::{expanded_projection::ExpandedProjection, projected_members::ProjectedMembers};
use chrono::{DateTime, Utc};
use derive_more::IsVariant;

/// What a clause 4.21 projection left of an entity.
#[derive(Clone, Copy, Debug, Eq, IsVariant, PartialEq)]
pub enum ProjectionRemainder {
    /// At least one Entity member survived, so there is an entity to render.
    Members,
    /// Every Entity member was projected away, so there is nothing to render.
    Nothing,
}

/// What `projection` left of `state`, at `now`.
///
/// `state` may have been narrowed by the projection already, which removes
/// only members the projection does not keep. An attribute whose every
/// instance expired at `now` is not one the compacted entity will carry
/// (clause 4.22), so only the standing ones count.
#[must_use]
pub fn state_remainder(state: &EntityState, projection: &ExpandedProjection, now: DateTime<Utc>) -> ProjectionRemainder {
    remainder_under(state, &ProjectedMembers::of(projection), now)
}

/// What a projection requiring `members` of an Entity left of `state`, at
/// `now`, read as [`state_remainder`] reads it.
#[must_use]
pub fn remainder_under(state: &EntityState, members: &ProjectedMembers, now: DateTime<Utc>) -> ProjectionRemainder {
    let standing = state
        .attributes()
        .names()
        .filter(|name| state.attribute_stands(name.as_str(), now))
        .map(AttributeName::as_str);
    if members.admits(standing, state.scope_presence()) {
        ProjectionRemainder::Members
    } else {
        ProjectionRemainder::Nothing
    }
}
