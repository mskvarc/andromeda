//! Whether a clause 4.21 projection leaves the history of an entity a member
//! to answer with.
//!
//! Clause 5.7.3.5 reduces the Temporal Evolution "down to only contain the
//! defined Entity members"; when the projection defines none of the members
//! the evolution has, what is left is no evolution at all, and clause 5.7.3.4
//! answers a retrieval of it with `ResourceNotFound`. `@context` is a JSON-LD
//! transport concern rather than an Entity member (clause 5.2.4), so it does
//! not keep an evolution alive.

use andromeda_ngsi_ld_model::temporal::temporal_history::TemporalHistory;
use andromeda_ngsi_ld_query::projection::{compiled::Projection, term::ProjectionTerm};
use andromeda_ngsi_ld_render::structural_projection::StructuralProjection;

/// Whether `projection` leaves `history` an Entity member to write.
///
/// The attribute half of the projection was applied while the history was
/// read, so an attribute still held is one the projection keeps; `id` and
/// `type` are the structural half, decided here.
pub(crate) fn carries_members(history: &TemporalHistory, projection: &Projection) -> bool {
    let structural = StructuralProjection::from(projection);
    let names_entity = structural.keeps(&ProjectionTerm::Id) || structural.keeps(&ProjectionTerm::Type);
    let holds_members = match history {
        TemporalHistory::Instances(document) => !document.entity().attributes().is_empty() || !document.system_times().is_empty(),
        TemporalHistory::Aggregates(entity) => !entity.is_empty(),
    };
    names_entity || holds_members
}
