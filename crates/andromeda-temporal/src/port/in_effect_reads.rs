//! What a read of the instances in effect when a window opens covers.
//!
//! A split query's roots are read for the attributes and the Scope their
//! filters read, and the entities a hop reaches for the attributes the hop
//! reads of them, so the read is told both rather than deriving them from
//! the query it runs under.

use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;

/// What an in-effect read covers.
#[derive(Debug, Clone, Copy)]
pub struct InEffectReads<'r> {
    /// The attributes whose latest instance before the start is read.
    pub attributes: &'r [&'r AttributeName],
    /// Whether the Scope event in force at the start is read.
    pub scope: ScopeRead,
}

/// Whether an in-effect read covers the Scope (clause 4.18).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScopeRead {
    /// The Scope event in force at the start is read.
    Read,
    /// The Scope is not read.
    Unread,
}
