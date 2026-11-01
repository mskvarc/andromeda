//! The grain key a stored attribute instance's `datasetId` is held as.
//!
//! A `current_attributes` row and a `temporal_attribute_streams` stream are
//! keyed by Entity, attribute and `datasetId` (clause 4.5.5). The `datasetId`
//! member of that key is non-null text: the empty string for the default
//! instance, which carries no `datasetId`, and the identifier exactly
//! otherwise. No identifier is empty, so the two never meet, and the key
//! compares with plain `=`, which a hash join and an index probe both use.
//! Every writer and decoder of the key converts through here.

use andromeda_ngsi_ld_lexicon::identifier::{Identifier, IdentifierParseError};
use andromeda_postgresql_seaquery::literal::text_literal;
use sea_query::{ExprTrait, SimpleExpr};

/// The stored key of the default instance.
const DEFAULT_INSTANCE: &str = "";

/// One instance's `datasetId` as its grain key holds it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DatasetKey<'a>(&'a str);

impl<'a> DatasetKey<'a> {
    /// The key of the instance `dataset_id` names, the default instance when
    /// it is `None`.
    #[must_use]
    pub fn of(dataset_id: Option<&'a Identifier>) -> DatasetKey<'a> {
        DatasetKey(dataset_id.map_or(DEFAULT_INSTANCE, Identifier::as_str))
    }

    /// A key as a row stores it.
    #[must_use]
    pub const fn stored(key: &'a str) -> DatasetKey<'a> {
        DatasetKey(key)
    }

    /// The text the key is stored as.
    #[must_use]
    pub const fn as_str(self) -> &'a str {
        self.0
    }

    /// The `datasetId` the key stands for, `None` for the default instance.
    ///
    /// # Errors
    ///
    /// Returns [`IdentifierParseError`] when a non-empty key is not an
    /// identifier.
    pub fn dataset_id(self) -> Result<Option<Identifier>, IdentifierParseError> {
        match self.0 {
            DEFAULT_INSTANCE => Ok(None),
            named => named.parse().map(Some),
        }
    }
}

/// Whether the grain key `column` is the default instance's, the empty string
/// written inline so a partial index predicate on it is provable.
#[must_use]
pub fn is_default_instance(column: impl Into<SimpleExpr>) -> SimpleExpr {
    column.into().eq(text_literal(DEFAULT_INSTANCE))
}

/// Whether the grain key `column` is a named instance's.
#[must_use]
pub fn is_named_instance(column: impl Into<SimpleExpr>) -> SimpleExpr {
    column.into().ne(text_literal(DEFAULT_INSTANCE))
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::dataset_key::{DatasetKey, is_default_instance, is_named_instance};
    use andromeda_ngsi_ld_lexicon::identifier::Identifier;
    use sea_query::{Alias, Expr, PostgresQueryBuilder, Query, SimpleExpr};

    fn render(expression: SimpleExpr) -> String {
        Query::select().expr(expression).to_owned().to_string(PostgresQueryBuilder)
    }

    #[test]
    fn the_default_instance_is_stored_as_the_empty_string_and_reads_back_as_none() {
        let key = DatasetKey::of(None);
        pretty_assertions::assert_eq!(key.as_str(), "");
        pretty_assertions::assert_eq!(DatasetKey::stored(key.as_str()).dataset_id().expect("the default key"), None);
    }

    #[test]
    fn a_named_instance_is_stored_as_its_identifier_and_reads_back_as_it() {
        let dataset: Identifier = "urn:ngsi-ld:Dataset:one".parse().expect("a valid identifier");
        let key = DatasetKey::of(Some(&dataset));
        pretty_assertions::assert_eq!(key.as_str(), "urn:ngsi-ld:Dataset:one");
        pretty_assertions::assert_eq!(DatasetKey::stored(key.as_str()).dataset_id().expect("a named key"), Some(dataset));
    }

    #[test]
    fn the_empty_key_never_reads_back_as_a_named_instance() {
        assert!(DatasetKey::stored("").dataset_id().is_ok_and(|dataset| dataset.is_none()));
    }

    #[test]
    fn a_stored_key_that_is_no_identifier_is_refused() {
        assert!(DatasetKey::stored("not an identifier").dataset_id().is_err());
    }

    #[test]
    fn the_default_and_named_tests_compare_the_key_with_the_inline_empty_string() {
        let column = || Expr::col((Alias::new("s"), Alias::new("dataset_key")));
        pretty_assertions::assert_eq!(render(is_default_instance(column())), r#"SELECT "s"."dataset_key" = ('')"#);
        pretty_assertions::assert_eq!(render(is_named_instance(column())), r#"SELECT "s"."dataset_key" <> ('')"#);
    }
}
