//! Whether a store applied the result window a query asked for.

use derive_more::IsVariant;

/// What a store did with the clause 5.5.6 result window.
///
/// The clause 6.3.13 count rides [`PaginationOutcome::Applied`] rather than
/// sitting beside this enum, because a store that could not apply the window
/// could not have counted correctly either: its `COUNT` would include the rows
/// the caller's residual filter is about to remove. Making the deferred variant
/// carry no total keeps that state unrepresentable rather than merely
/// documented.
#[derive(Debug, Clone, Copy, PartialEq, Eq, IsVariant)]
pub enum PaginationOutcome {
    /// The store applied `limit` and `offset`, and honoured the count policy.
    Applied {
        /// The total number of matching entities, when the query asked for one.
        total: Option<i64>,
    },
    /// The store returned every matching row with no window applied, because a
    /// predicate it declined would have made the window wrong.
    ///
    /// The caller finishes the filter, then takes the window itself.
    Deferred,
}
