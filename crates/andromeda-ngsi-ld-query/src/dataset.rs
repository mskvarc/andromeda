//! The `datasetId` that names one instance of a multi-instance attribute
//! (clause 4.5.5).

use crate::error::QueryError;
use andromeda_ngsi_ld_lexicon::{default_instance::DEFAULT_INSTANCE_TOKEN, identifier::Identifier};
use serde::{Deserialize, Serialize};
use std::{fmt, str::FromStr};

/// One `datasetId`, naming either a specific dataset or the default instance.
///
/// Used both as a filter entry on a temporal query and as a member of an
/// `EntityMap` (clause 5.2.39), which records which instances of an entity a
/// distributed query resolved. Both are the same clause 4.5.5 value, and the
/// `EntityMap` resource used to carry a second type spelling it differently.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub enum DatasetSelector {
    /// The default instance, which carries no `datasetId` (clause 4.5.5).
    Default,
    /// A specific dataset identifier.
    Id(Identifier),
}

impl DatasetSelector {
    /// Parse one `datasetId` entry.
    ///
    /// # Errors
    ///
    /// Returns [`QueryError::ParamDatasetIdInvalid`] when `raw` is neither the
    /// `@none` token of clause 4.5.5 nor a URI.
    pub fn parse(raw: &str) -> Result<DatasetSelector, QueryError> {
        if raw == DEFAULT_INSTANCE_TOKEN {
            return Ok(DatasetSelector::Default);
        }
        raw.parse::<Identifier>().map(DatasetSelector::Id).map_err(QueryError::ParamDatasetIdInvalid)
    }

    /// The `datasetId` an attribute instance written with this selector
    /// carries: `None` for the default instance, which has none (clause 4.5.5).
    #[must_use]
    pub fn into_named(self) -> Option<Identifier> {
        match self {
            DatasetSelector::Default => None,
            DatasetSelector::Id(named) => Some(named),
        }
    }

    /// Whether this selector names the Attribute instance whose `datasetId` is
    /// `dataset_id`, `None` being the default instance (clause 4.5.5).
    #[must_use]
    pub fn selects(&self, dataset_id: Option<&Identifier>) -> bool {
        match self {
            DatasetSelector::Default => dataset_id.is_none(),
            DatasetSelector::Id(named) => dataset_id == Some(named),
        }
    }
}

impl FromStr for DatasetSelector {
    type Err = QueryError;

    fn from_str(raw: &str) -> Result<DatasetSelector, QueryError> {
        DatasetSelector::parse(raw)
    }
}

// Parsing lives in `DatasetSelector::parse` so borrowed call sites do not
// allocate; this adapter exists only because `#[serde(try_from = "String")]`
// requires it.
impl TryFrom<String> for DatasetSelector {
    type Error = QueryError;

    fn try_from(raw: String) -> Result<DatasetSelector, QueryError> {
        DatasetSelector::parse(&raw)
    }
}

// `derive_more::Display` would need a format string per variant and the
// `Default` arm renders a constant that is defined above rather than a field.
impl fmt::Display for DatasetSelector {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DatasetSelector::Default => formatter.write_str(DEFAULT_INSTANCE_TOKEN),
            DatasetSelector::Id(uri) => write!(formatter, "{uri}"),
        }
    }
}

// The wire form is produced by the `Display` above; this exists only because
// `#[serde(into = "String")]` demands an `Into<String>` and cannot use it.
impl From<DatasetSelector> for String {
    fn from(selector: DatasetSelector) -> String {
        selector.to_string()
    }
}

/// A `datasetId` filter reduced to the three cases a store has to tell apart.
///
/// The default instance has no `datasetId`, so it cannot be selected the same
/// way a named one is (clause 4.5.5). Splitting the selector list into the three
/// cases that follow from that — unfiltered, the default only, or a set of named
/// ids — is done here, once, rather than in each store backend where it used to
/// live twice.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DatasetFilter {
    /// No filter: every instance qualifies.
    Unfiltered,
    /// Only the default instance.
    DefaultOnly,
    /// Only the named datasets.
    NamedOnly(Vec<Identifier>),
    /// The default instance or any of the named datasets.
    DefaultOrNamed(Vec<Identifier>),
}

impl DatasetFilter {
    /// Reduce a selector list to the predicate shape it renders as.
    #[must_use]
    pub fn from_selectors(selectors: &[DatasetSelector]) -> DatasetFilter {
        if selectors.is_empty() {
            return DatasetFilter::Unfiltered;
        }
        let mut default_selected = false;
        let mut named = Vec::with_capacity(selectors.len());
        for selector in selectors {
            match selector {
                DatasetSelector::Default => default_selected = true,
                // clone: the reduced filter owns its identifiers, and the
                // selector list it came from is borrowed
                DatasetSelector::Id(uri) => named.push(uri.clone()),
            }
        }
        match (default_selected, named.is_empty()) {
            (true, true) => DatasetFilter::DefaultOnly,
            (true, false) => DatasetFilter::DefaultOrNamed(named),
            (false, _) => DatasetFilter::NamedOnly(named),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dataset_uri(raw: &str) -> Identifier {
        raw.parse().expect("valid URI")
    }

    fn selectors(raw: &[&str]) -> Vec<DatasetSelector> {
        raw.iter().map(|entry| DatasetSelector::parse(entry).expect("valid selector")).collect()
    }

    #[test]
    fn the_none_token_selects_the_default_instance() {
        assert_eq!(DatasetSelector::parse("@none").unwrap(), DatasetSelector::Default);
    }

    #[test]
    fn a_urn_selects_one_named_dataset() {
        let selector = DatasetSelector::parse("urn:ngsi-ld:Dataset:one").unwrap();
        assert!(matches!(selector, DatasetSelector::Id(_)), "got {selector:?}");
    }

    #[test]
    fn an_https_uri_selects_one_named_dataset() {
        let selector = DatasetSelector::parse("https://example.com/datasets/one").expect("valid dataset URI");
        assert!(matches!(selector, DatasetSelector::Id(_)), "got {selector:?}");
        pretty_assertions::assert_eq!(selector.to_string(), "https://example.com/datasets/one");
    }

    #[test]
    fn a_dataset_id_that_is_not_a_uri_is_rejected() {
        let error = DatasetSelector::parse("not a uri").expect_err("must reject");
        assert!(matches!(error, QueryError::ParamDatasetIdInvalid { .. }), "got {error:?}");
    }

    #[test]
    fn the_none_token_selects_the_instance_that_carries_no_dataset_id() {
        let selector = DatasetSelector::parse("@none").expect("valid selector");
        assert!(selector.selects(None));
        assert!(!selector.selects(Some(&dataset_uri("urn:ngsi-ld:Dataset:1"))));
    }

    #[test]
    fn a_named_dataset_id_selects_only_the_instance_carrying_it() {
        let selector = DatasetSelector::parse("urn:ngsi-ld:Dataset:1").expect("valid selector");
        assert!(selector.selects(Some(&dataset_uri("urn:ngsi-ld:Dataset:1"))));
        assert!(!selector.selects(Some(&dataset_uri("urn:ngsi-ld:Dataset:2"))));
        assert!(!selector.selects(None));
    }

    #[test]
    fn a_mixed_selection_admits_both_the_default_instance_and_the_named_one() {
        let wanted = selectors(&["@none", "urn:ngsi-ld:Dataset:1"]);
        let admitted = |dataset_id: Option<&Identifier>| wanted.iter().any(|selector| selector.selects(dataset_id));
        assert!(admitted(None));
        assert!(admitted(Some(&dataset_uri("urn:ngsi-ld:Dataset:1"))));
        assert!(!admitted(Some(&dataset_uri("urn:ngsi-ld:Dataset:2"))));
    }

    #[test]
    fn an_empty_selector_list_filters_nothing() {
        assert_eq!(DatasetFilter::from_selectors(&[]), DatasetFilter::Unfiltered);
    }

    #[test]
    fn the_three_selector_shapes_reduce_as_clause_four_five_five_requires() {
        assert_eq!(DatasetFilter::from_selectors(&selectors(&["@none"])), DatasetFilter::DefaultOnly);
        assert_eq!(
            DatasetFilter::from_selectors(&selectors(&["urn:ngsi-ld:Dataset:one"])),
            DatasetFilter::NamedOnly(vec![dataset_uri("urn:ngsi-ld:Dataset:one")])
        );
        assert_eq!(
            DatasetFilter::from_selectors(&selectors(&["@none", "urn:ngsi-ld:Dataset:one"])),
            DatasetFilter::DefaultOrNamed(vec![dataset_uri("urn:ngsi-ld:Dataset:one")])
        );
    }
}
