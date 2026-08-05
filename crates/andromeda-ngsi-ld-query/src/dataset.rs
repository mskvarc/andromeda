//! The `datasetId` that names one instance of a multi-instance attribute
//! (clause 4.5.5).

use crate::error::QueryError;
use serde::{Deserialize, Serialize};
use std::{fmt, str::FromStr};
use urn_rs::Urn;

/// The reserved `datasetId` value naming the default attribute instance.
///
/// Clause 4.5.5: an attribute instance with no `datasetId` is the default
/// instance, and `datasetId=@none` selects it. `@none` is a JSON-LD keyword,
/// not a URI, so it cannot collide with a real identifier.
pub const DEFAULT_INSTANCE_TOKEN: &str = "@none";

/// One `datasetId`, naming either a specific dataset or the default instance.
///
/// Used both as a filter entry on a temporal query and as a member of an
/// EntityMap (clause 5.2.39), which records which instances of an entity a
/// distributed query resolved. Both are the same clause 4.5.5 value, and the
/// EntityMap resource used to carry a second type spelling it differently.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub enum DatasetSelector {
    /// The default instance, which carries no `datasetId` (clause 4.5.5).
    Default,
    /// A specific dataset identifier.
    Id(Urn),
}

impl DatasetSelector {
    /// Parse one `datasetId` entry.
    pub fn parse(raw: &str) -> Result<DatasetSelector, QueryError> {
        if raw == DEFAULT_INSTANCE_TOKEN {
            return Ok(DatasetSelector::Default);
        }
        raw.parse::<Urn>()
            .map(DatasetSelector::Id)
            .map_err(|_| QueryError::ParamDatasetIdInvalid(raw.to_string()))
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
            DatasetSelector::Id(urn) => write!(formatter, "{urn}"),
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

/// A `datasetId` filter reduced to the shape its SQL takes.
///
/// The default instance is stored as a NULL `dataset_id`, so it cannot be
/// tested by the same `= ANY` predicate as a named one. Splitting the selector
/// list into the three cases that follow from that is done here rather than in
/// each backend, which is where it used to live twice.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DatasetFilter {
    /// No filter: every instance qualifies.
    Unfiltered,
    /// Only the default instance.
    DefaultOnly,
    /// Only the named datasets.
    NamedOnly(Vec<Urn>),
    /// The default instance or any of the named datasets.
    DefaultOrNamed(Vec<Urn>),
}

impl DatasetFilter {
    /// Reduce a selector list to the predicate shape it renders as.
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
                DatasetSelector::Id(urn) => named.push(urn.clone()),
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

    fn dataset_urn(raw: &str) -> Urn {
        raw.parse().expect("valid URN")
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
    fn a_dataset_id_that_is_not_a_urn_is_rejected() {
        let error = DatasetSelector::parse("not a urn").expect_err("must reject");
        assert!(matches!(error, QueryError::ParamDatasetIdInvalid(_)), "got {error:?}");
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
            DatasetFilter::NamedOnly(vec![dataset_urn("urn:ngsi-ld:Dataset:one")])
        );
        assert_eq!(
            DatasetFilter::from_selectors(&selectors(&["@none", "urn:ngsi-ld:Dataset:one"])),
            DatasetFilter::DefaultOrNamed(vec![dataset_urn("urn:ngsi-ld:Dataset:one")])
        );
    }
}
