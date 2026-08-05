//! The compiled, context-expanded form of a temporal query.

use crate::{
    pagination::Pagination,
    temporal::{entity_selection::EntitySelection, instance_selection::InstanceSelection, representation::Representation, window::TemporalWindow},
};
use typed_builder::TypedBuilder;

/// Compiled temporal query ready for SQL building.
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
    /// Result window: limit, offset, and the total-count policy.
    #[builder(default = Pagination::DEFAULT)]
    pub pagination: Pagination,
}
