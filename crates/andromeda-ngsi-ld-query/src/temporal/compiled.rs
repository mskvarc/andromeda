//! The compiled, context-expanded form of a temporal query.

use crate::{
    entity_selection::EntitySelection,
    projection::compiled::Projection,
    temporal::{
        contributed_entities::ContributedEntities,
        instance_selection::InstanceSelection,
        ordering::TemporalEntityOrdering,
        representation::Representation,
        window::TemporalWindow,
    },
};
use typed_builder::TypedBuilder;

/// Compiled temporal query ready to be lowered to a store query.
///
/// Produced by expanding and validating `TemporalQueryParameters` against the
/// active JSON-LD context.
///
/// The parts are separated because they change for unrelated reasons and are
/// read by unrelated code: stage 1 of the statement reads only
/// [`EntitySelection`] and [`TemporalWindow`], stage 2 reads only
/// [`InstanceSelection`] and the window, and the renderers read only
/// [`Representation`]. Holding them as one flat record let each of those reach
/// whatever it happened to need.
///
/// The page of entities the query answers with is no part of it: the request
/// boundary decides that window once and the reader is handed it beside the
/// compiled query.
#[derive(Debug, Clone, TypedBuilder)]
pub struct CompiledTemporalQuery {
    /// Which entities qualify.
    pub entities: EntitySelection,
    /// Which slice of the time axis is read.
    pub window: TemporalWindow,
    /// Which attribute instances are returned for each qualifying entity.
    pub instances: InstanceSelection,
    /// How those instances are rendered.
    pub representation: Representation,
    /// Which members of a matching entity's temporal representation reach the
    /// response (clause 4.21 `pick` / `omit`).
    #[builder(default = Default::default())]
    pub projection: Projection,
    /// How the matching entities are ordered, when the request asked for an
    /// order at all (clause 5.7.4.3).
    ///
    /// `None` is not "ascending by default": clause 5.5.9 needs a consistent
    /// order across pages whether or not one was requested, so the store's own
    /// stable order stands, and only a requested ordering carries a client
    /// collation into the comparison.
    #[builder(default = Default::default())]
    pub ordering: Option<TemporalEntityOrdering>,
    /// The entities a Context Source contributed a part of under
    /// `splitEntities` (clause 5.7.4.4), which the local read exempts from the
    /// value, geo and scope filters; empty for every other query.
    #[builder(default = Default::default())]
    pub contributed: ContributedEntities,
}
