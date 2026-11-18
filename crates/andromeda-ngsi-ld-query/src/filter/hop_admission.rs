//! The admission of the Relationship hops a `q` filter takes (clause 4.9),
//! named here as the port the `q` compile entry holds a filter to.
//!
//! Which hops an operation admits is the clause 4.5.23 Linked Entity rule,
//! decided per operation under the request's `join` and `joinLevel` by the
//! Linked Entity retrieval engine. That engine reads this crate, so this crate
//! cannot call it: the compile entry takes the decision as this trait, and
//! every pipeline hands it the engine's implementation.

use crate::filter::Filter;

/// Admits or refuses the Linked Entity hops of one `q` filter for the
/// operation a pipeline runs.
///
/// `Sync`, as [`TermExpander`](crate::term_expander::TermExpander) is, so the
/// compile entry's future holds one across its expansion and stays `Send`.
pub trait FilterHopAdmission: Sync {
    /// Why the operation refuses a hop.
    type Refusal;

    /// Admit every Relationship hop `filter` takes.
    ///
    /// # Errors
    ///
    /// Returns the [`FilterHopAdmission::Refusal`] of a hop the operation does
    /// not admit.
    fn admit_hops(&self, filter: &Filter) -> Result<(), Self::Refusal>;
}
