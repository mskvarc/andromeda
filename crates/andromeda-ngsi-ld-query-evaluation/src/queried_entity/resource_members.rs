//! What `q` reads of a resource that is not an entity: its members, each a
//! target of its own.

use crate::q::target_value::ResolvedTarget;
use andromeda_ngsi_ld_query::filter::reading::term_reading::TermReading;

/// A resource a `q` filter reads by its members, such as a Snapshot that Purge
/// Snapshots selects (clause 5.16.7.3).
///
/// Such a resource holds at most one value per member, so a Query Term names
/// one target or none. It is implemented for cheap `Copy` handles, so every
/// target borrows from the resource for `'t`.
pub trait ResourceMembers<'t>: Copy {
    /// The one target `term` names among the members, `None` when the
    /// resource does not hold that member.
    fn member_target(self, term: &TermReading) -> Option<ResolvedTarget<'t>>;
}
