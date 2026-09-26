//! The port every write and read path resolves interned names through.
//!
//! The store crates map expanded attribute IRIs onto the `INTEGER` key the
//! temporal catalogue, the history hypertable and the
//! `current_attribute_values` projection are written against, and expanded
//! entity type IRIs onto the key the `entity_type_name_keys` arrays carry. The concrete
//! dictionary is [`NameCache`](crate::name_cache::NameCache); this trait is
//! what the stores hold, so none of them depends on another.
//!
//! The two operations differ in more than direction. Resolution that may mint
//! runs *inside the caller's transaction*, on the connection that transaction
//! holds: a dictionary row is part of the write that needed it, so it commits
//! and rolls back with that write, and no phase of a commit reaches into the
//! pool for a second connection while holding one. Read-only lookup takes no
//! connection and runs on the dictionary's own pool, where it sees a committed
//! snapshot.

use crate::dictionary_growth::DictionaryGrowthExceeded;
use andromeda_ngsi_ld_lexicon::reference::NgsiLdReferenceError;
use andromeda_postgresql_query::dictionary::{interned_name::InternedName, resolved_names::ResolvedNames};
use sqlx::PgConnection;
use std::{collections::HashMap, fmt::Debug};

/// Why a name resolution did not answer.
#[derive(Debug, thiserror::Error)]
pub enum NameLookupError {
    /// A dictionary statement failed.
    #[error("name dictionary statement failed: {0}")]
    Statement(#[from] sqlx::Error),
    /// A dictionary statement could not be assembled, which a column/value arity
    /// mismatch is the only cause of.
    #[error("name dictionary statement could not be built: {0}")]
    StatementBuild(String),
    /// A stored `attribute_names` row holds text that is not an attribute name.
    ///
    /// The raw text is carried rather than a typed name because the value is
    /// exactly the one that failed to be one.
    #[error("attribute_names.name '{name}' is not a valid attribute name: {source}")]
    StoredAttributeName {
        /// The stored text the row carried.
        name: String,
        /// Why the grammar of clause 4.6.2 rejected it.
        source: NgsiLdReferenceError,
    },
    /// A stored `entity_type_names` row holds text that is not an entity type
    /// name.
    #[error("entity_type_names.name '{name}' is not a valid entity type name: {source}")]
    StoredEntityTypeName {
        /// The stored text the row carried.
        name: String,
        /// Why the grammar of clause 4.6.2 rejected it.
        source: NgsiLdReferenceError,
    },
    /// The names the caller asked for would take the tenant's dictionary past
    /// its configured bound.
    #[error(transparent)]
    Growth(#[from] DictionaryGrowthExceeded),
}

/// Port for resolving interned names of type `N` to their dictionary keys.
#[async_trait::async_trait]
pub trait NameLookup<N: InternedName>: Send + Sync + Debug {
    /// Resolve names that already exist, without creating any.
    ///
    /// A name the broker has never recorded resolves to nothing. Its predicates
    /// compile to constant `FALSE`, which is correct: no projected, historical
    /// or current row can reference a dictionary entry that does not exist.
    ///
    /// # Errors
    ///
    /// Returns [`NameLookupError::Statement`] when the read fails and one of the
    /// `Stored*` variants when a stored row is unreadable.
    #[must_use = "name lookups must be awaited"]
    async fn lookup(&self, names: &[N]) -> Result<ResolvedNames<N>, NameLookupError>;

    /// Resolve every name in `connection`'s transaction, minting the new ones.
    ///
    /// The caller passes the connection its own transaction holds, so the rows
    /// this creates are part of that transaction: they become visible when it
    /// commits and are gone when it rolls back, and the bound they were admitted
    /// against is likewise released by a rollback.
    ///
    /// # Errors
    ///
    /// Returns [`NameLookupError::Growth`] when the new names would take the
    /// dictionary past its bound, [`NameLookupError::Statement`] when a
    /// statement fails, and one of the `Stored*` variants when a stored row is
    /// unreadable.
    #[must_use = "name resolution must be awaited"]
    async fn resolve_or_create(&self, connection: &mut PgConnection, names: &[N]) -> Result<HashMap<N, N::Key>, NameLookupError>;
}
