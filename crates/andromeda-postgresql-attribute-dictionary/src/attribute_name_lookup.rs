//! The port every write and read path resolves attribute names through.
//!
//! Both store crates map expanded attribute IRIs onto the `INTEGER` key the
//! temporal catalogue, the history hypertable and the
//! `current_attribute_values` projection are written against. The concrete
//! dictionary is [`AttributeNameCache`](crate::attribute_name_cache::AttributeNameCache);
//! this trait is what the stores hold, so neither of them depends on the other.
//!
//! The two operations differ in more than direction. Resolution that may mint
//! runs *inside the caller's transaction*, on the connection that transaction
//! holds: a dictionary row is part of the write that needed it, so it commits
//! and rolls back with that write, and no phase of a commit reaches into the
//! pool for a second connection while holding one. Read-only lookup takes no
//! connection and runs on the dictionary's own pool, where it sees a committed
//! snapshot.

use crate::dictionary_growth::DictionaryGrowthExceeded;
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, reference::NgsiLdReferenceError};
use andromeda_postgresql_query::{attribute_name_id::AttributeNameId, q_predicate::attribute_names::ResolvedAttributeNames};
use sqlx::PgConnection;
use std::{collections::HashMap, fmt::Debug};

/// Why an attribute-name resolution did not answer.
#[derive(Debug, thiserror::Error)]
pub enum AttributeNameLookupError {
    /// A dictionary statement failed.
    #[error("attribute-name dictionary statement failed: {0}")]
    Statement(#[from] sqlx::Error),
    /// A stored dictionary row holds text that is not an attribute name.
    ///
    /// The raw text is carried rather than a typed name because the value is
    /// exactly the one that failed to be one.
    #[error("attribute_names.name '{name}' is not a valid attribute name: {source}")]
    StoredName {
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

/// Port for resolving expanded attribute IRIs to their dictionary keys.
#[async_trait::async_trait]
pub trait AttributeNameLookup: Send + Sync + Debug {
    /// Resolve names that already exist, without creating any.
    ///
    /// An IRI the broker has never recorded resolves to nothing. Its predicates
    /// compile to constant `FALSE`, which is correct: no projected or historical
    /// row can reference a dictionary entry that does not exist.
    ///
    /// # Errors
    ///
    /// Returns [`AttributeNameLookupError::Statement`] when the read fails and
    /// [`AttributeNameLookupError::StoredName`] when a stored row is unreadable.
    #[must_use = "attribute-name lookups must be awaited"]
    async fn lookup(&self, names: &[AttributeName]) -> Result<ResolvedAttributeNames, AttributeNameLookupError>;

    /// Resolve every name in `connection`'s transaction, minting the new ones.
    ///
    /// The caller passes the connection its own transaction holds, so the rows
    /// this creates are part of that transaction: they become visible when it
    /// commits and are gone when it rolls back, and the bound they were admitted
    /// against is likewise released by a rollback.
    ///
    /// # Errors
    ///
    /// Returns [`AttributeNameLookupError::Growth`] when the new names would
    /// take the dictionary past its bound, [`AttributeNameLookupError::Statement`]
    /// when a statement fails, and [`AttributeNameLookupError::StoredName`] when
    /// a stored row is unreadable.
    #[must_use = "attribute-name resolution must be awaited"]
    async fn resolve_or_create(
        &self,
        connection: &mut PgConnection,
        names: &[AttributeName],
    ) -> Result<HashMap<AttributeName, AttributeNameId>, AttributeNameLookupError>;
}
