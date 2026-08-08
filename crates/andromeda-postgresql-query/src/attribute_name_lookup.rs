//! Consumer-declared port for the attribute-name dictionary.
//!
//! Both store crates resolve expanded attribute IRIs to their `SMALLINT`
//! dictionary keys. The concrete cache lives in whichever crate owns the
//! `attribute_names` table; this trait lets the other crate depend on the
//! capability without depending on the concrete implementation.

use crate::{attribute_name_id::AttributeNameId, q_predicate::attribute_names::ResolvedAttributeNames};
use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
use std::{collections::HashMap, fmt::Debug};

/// Errors that can occur during attribute name resolution.
#[derive(Debug, thiserror::Error)]
#[error("{message}")]
pub struct AttributeNameLookupError {
    /// Human-readable description of the failure.
    message: String,
}

impl AttributeNameLookupError {
    /// Create a new lookup error.
    pub fn new(message: impl Into<String>) -> AttributeNameLookupError {
        AttributeNameLookupError { message: message.into() }
    }
}

/// Port for resolving expanded attribute IRIs to their dictionary keys.
///
/// Two operations: read-only [`lookup`](AttributeNameLookup::lookup) for the
/// query path (unknown names produce no match), and
/// [`resolve_or_create`](AttributeNameLookup::resolve_or_create) for the write
/// path (unknown names are minted).
#[async_trait::async_trait]
pub trait AttributeNameLookup: Send + Sync + Debug {
    /// Resolve names that already exist, without creating any.
    ///
    /// An IRI the broker has never recorded resolves to nothing. Its predicates
    /// compile to constant `FALSE`.
    #[must_use = "attribute-name lookups must be awaited"]
    async fn lookup(&self, names: &[AttributeName]) -> Result<ResolvedAttributeNames, AttributeNameLookupError>;

    /// Resolve every name, creating dictionary entries for those that are new.
    #[must_use = "attribute-name resolution must be awaited"]
    async fn resolve_or_create(&self, names: &[&str]) -> Result<HashMap<String, AttributeNameId>, AttributeNameLookupError>;
}
