//! Purge modes of the delete-entities-by-query operation (clause 5.6.21).

use crate::{dataset::DatasetSelector, error::QueryError};
use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;

/// Which `datasetId` instances a `drop`/`keep` purge removes (clause 4.5.5).
///
/// The `Unfiltered` case is the standard Purge behaviour of clause 5.6.21: a
/// bare `drop`/`keep` removes the whole attribute, every instance. `All` and
/// `Selector` are the Andromeda `purge_dataset_scope` extension, adding the
/// per-`datasetId` precision the single-entity Delete Attribute operation
/// (clause 5.6.5) already has. Keeping `Unfiltered` distinct from `All`
/// preserves the conformant default — a bare `drop` still deletes the whole
/// attribute — rather than quietly reinterpreting it as a dataset-scoped one.
///
/// The lowering to the model's dataset-selection primitive lives in the entity
/// crate, which owns the mutation: this crate has no dependency on the model.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DatasetPurgeScope {
    /// The whole attribute, every instance (the standard clause 5.6.21 default).
    Unfiltered,
    /// Every instance, named explicitly by `deleteAll` (clause 5.6.13).
    All,
    /// One instance, named by `datasetId` (`@none` selects the default one).
    Selector(DatasetSelector),
}

impl DatasetPurgeScope {
    /// Whether the scope removes the whole attribute rather than named
    /// instances of it.
    #[must_use]
    pub const fn is_unfiltered(&self) -> bool {
        matches!(self, DatasetPurgeScope::Unfiltered)
    }
}

/// Purge mode for the DELETE entities-by-query operation (clause 5.6.21).
///
/// When `drop` or `keep` are provided the operation modifies entity attributes
/// instead of deleting whole entities; the [`DatasetPurgeScope`] carried by each
/// says whether the whole attribute or only named `datasetId` instances go.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PurgeMode {
    /// Delete entire matching entities (default behaviour).
    DeleteEntities,
    /// Remove the listed attributes from matching entities.
    Drop {
        /// The attributes to remove.
        attributes: Vec<AttributeName>,
        /// Which `datasetId` instances of them to remove.
        dataset: DatasetPurgeScope,
    },
    /// Retain only the listed attributes on matching entities, removing all others.
    Keep {
        /// The attributes to retain.
        attributes: Vec<AttributeName>,
        /// Which `datasetId` instances of the non-retained attributes to remove.
        dataset: DatasetPurgeScope,
    },
}

impl PurgeMode {
    /// The `datasetId` scope of a `drop`/`keep` mode, or `None` for whole-entity
    /// deletion.
    #[must_use]
    pub const fn dataset_scope(&self) -> Option<&DatasetPurgeScope> {
        match self {
            PurgeMode::DeleteEntities => None,
            PurgeMode::Drop { dataset, .. } | PurgeMode::Keep { dataset, .. } => Some(dataset),
        }
    }
}

/// Parse the comma-separated `drop` / `keep` parameter value.
///
/// Clause 5.6.21 names attributes, so a value outside the clause 4.6.2 name
/// grammar cannot name one and the request is rejected rather than silently
/// purging nothing.
///
/// # Errors
///
/// Returns [`QueryError::AttributeNameInvalid`] when an entry is outside the
/// clause 4.6.2 attribute-name grammar.
pub fn parse_attribute_list(raw: &str) -> Result<Vec<AttributeName>, QueryError> {
    raw.split(',')
        .map(|entry| entry.trim().parse().map_err(QueryError::AttributeNameInvalid))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn a_comma_separated_list_yields_one_name_per_entry() {
        let parsed = parse_attribute_list("speed, colour").expect("parses");
        let rendered: Vec<&str> = parsed.iter().map(AttributeName::as_str).collect();
        assert_eq!(rendered, vec!["speed", "colour"]);
    }

    #[test]
    fn a_value_outside_the_attribute_name_grammar_is_rejected() {
        let error = parse_attribute_list("speed,bad name").expect_err("must reject");
        assert!(matches!(error, QueryError::AttributeNameInvalid { .. }), "got {error:?}");
    }

    /// The unfiltered scope removes the whole attribute, which is what
    /// preserves the clause 5.6.21 default.
    #[test]
    fn the_unfiltered_scope_removes_the_whole_attribute() {
        assert!(DatasetPurgeScope::Unfiltered.is_unfiltered());
        assert!(!DatasetPurgeScope::All.is_unfiltered());
        assert!(!DatasetPurgeScope::Selector(DatasetSelector::Default).is_unfiltered());
    }

    /// A `drop`/`keep` mode exposes its dataset scope; whole-entity deletion has
    /// none.
    #[test]
    fn only_attribute_modes_carry_a_dataset_scope() {
        assert!(PurgeMode::DeleteEntities.dataset_scope().is_none());
        let drop = PurgeMode::Drop {
            attributes: Vec::new(),
            dataset: DatasetPurgeScope::All,
        };
        assert!(matches!(drop.dataset_scope(), Some(DatasetPurgeScope::All)));
    }
}
