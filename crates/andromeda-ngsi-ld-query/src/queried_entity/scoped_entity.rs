//! What a scope query reads of an entity (clauses 4.18 and 4.19).

use andromeda_ngsi_ld_lexicon::scope::path::ScopePath;

/// An entity as a scope query reads it.
pub trait ScopedEntity<'a>: Copy {
    /// The scopes the entity belongs to (clause 4.18), none when it carries no
    /// `scope`.
    ///
    /// The iterator is cloned once per requirement a query alternative makes,
    /// so it has to be cheap to clone.
    fn scopes(self) -> impl Iterator<Item = &'a ScopePath> + Clone;
}
