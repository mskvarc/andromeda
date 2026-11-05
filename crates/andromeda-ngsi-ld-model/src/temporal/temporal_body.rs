//! The Temporal Evolution of one entity as an answer carries it, in either of
//! the two shapes clause 6.3.12 gives it.

use crate::{
    attribute::member_value_reading::ReadError,
    entity_type::EntityTypeValue,
    temporal::{aggregated_entity::AggregatedTemporalEntity, temporal_entity::TemporalEntity},
};
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_ngsi_ld_representation::compact_node::CompactNode;

/// The Temporal Evolution of one entity in an answer: the sequence of the
/// instances each attribute took over time (clauses 4.5.7 and 4.5.8, whichever
/// of the two representations writes it), or the aggregates over them (clause
/// 4.5.19).
#[derive(Clone, Debug)]
pub enum TemporalBody {
    /// The instances, compacted.
    Evolution(TemporalEntity),
    /// The aggregates.
    Aggregated(AggregatedTemporalEntity),
}

impl TemporalBody {
    /// The Temporal Evolution a compact node is, in whichever of the two shapes
    /// a peer answered with: the instances of each attribute, or the aggregates
    /// over them.
    ///
    /// The instances are tried first. An aggregated attribute holds no value
    /// member, which an instance requires, so a node one reads as is never the
    /// other.
    ///
    /// # Errors
    ///
    /// Returns the refusal of reading the node as instances when it is no
    /// aggregate either.
    pub fn from_node<'n, N: CompactNode<'n>>(node: N) -> Result<TemporalBody, ReadError<'n, N>> {
        match TemporalEntity::from_node(node) {
            Ok(entity) => Ok(TemporalBody::Evolution(entity)),
            Err(instances) => AggregatedTemporalEntity::from_node(node).map(TemporalBody::Aggregated).map_err(|_| instances),
        }
    }

    /// The entity's identifier.
    #[must_use]
    pub const fn id(&self) -> &Identifier {
        match self {
            TemporalBody::Evolution(entity) => &entity.id,
            TemporalBody::Aggregated(entity) => &entity.id,
        }
    }

    /// The entity's types.
    #[must_use]
    pub const fn type_(&self) -> &EntityTypeValue {
        match self {
            TemporalBody::Evolution(entity) => &entity.type_,
            TemporalBody::Aggregated(entity) => &entity.type_,
        }
    }
}
