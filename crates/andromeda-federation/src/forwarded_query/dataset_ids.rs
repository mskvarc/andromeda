//! The `datasetId` parameter of a read forwarded to one registration (clause
//! 4.3.6.1, Table 5.2.9-1).
//!
//! A source is asked only for the `datasetId`s its registration lists, which a
//! narrowed query writes among the parameters it builds.

use andromeda_ngsi_ld_model::registration::dataset_overlap::DatasetOverlap;

/// The value of the `datasetId` parameter a source is asked with, its
/// `datasetId`s comma-separated, or `None` when it is asked for none.
pub(crate) fn dataset_ids_parameter(datasets: DatasetOverlap<'_>) -> Option<String> {
    let written = datasets.forwarded().map(ToString::to_string).collect::<Vec<_>>();
    (!written.is_empty()).then(|| written.join(","))
}

#[cfg(test)]
mod tests {
    use crate::forwarded_query::dataset_ids::dataset_ids_parameter;
    use andromeda_ngsi_ld_model::registration::dataset_overlap::DatasetOverlap;
    use andromeda_ngsi_ld_query::dataset::DatasetSelector;
    use pretty_assertions::assert_eq;

    fn datasets(raw: &[&str]) -> Vec<DatasetSelector> {
        raw.iter().map(|selector| selector.parse().expect("a valid datasetId")).collect()
    }

    /// Clause 4.3.6.1: the client's `datasetId`s are narrowed to the ones the
    /// registration lists.
    #[test]
    fn the_client_dataset_ids_are_narrowed_to_the_listed_ones() {
        let requested = datasets(&["urn:ngsi-ld:Dataset:a", "urn:ngsi-ld:Dataset:b"]);
        let listed = datasets(&["urn:ngsi-ld:Dataset:a"]);
        assert_eq!(
            dataset_ids_parameter(DatasetOverlap::of(Some(&requested), Some(&listed))).as_deref(),
            Some("urn:ngsi-ld:Dataset:a")
        );
    }

    /// Table 5.2.9-1: a client naming no `datasetId` selects every instance,
    /// so the source is asked for every one its registration lists.
    #[test]
    fn a_client_naming_none_asks_for_every_listed_dataset_id() {
        let listed = datasets(&["@none", "urn:ngsi-ld:Dataset:a"]);
        assert_eq!(
            dataset_ids_parameter(DatasetOverlap::of(None, Some(&listed))).as_deref(),
            Some("@none,urn:ngsi-ld:Dataset:a")
        );
    }

    /// A registration listing no `datasetId` restricts none, so the source is
    /// asked for the client's `datasetId`s, or none.
    #[test]
    fn an_unrestricted_registration_keeps_the_client_dataset_ids() {
        let requested = datasets(&["urn:ngsi-ld:Dataset:b"]);
        assert_eq!(
            dataset_ids_parameter(DatasetOverlap::of(Some(&requested), None)).as_deref(),
            Some("urn:ngsi-ld:Dataset:b")
        );
        assert_eq!(dataset_ids_parameter(DatasetOverlap::of(None, None)), None);
    }
}
