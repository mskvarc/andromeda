//! The `datasetId` parameter of a read forwarded to one registration (clause
//! 4.3.6.1, Table 5.2.9-1).
//!
//! A source is asked only for the `datasetId`s its registration lists. A
//! narrowed query writes the parameter among the ones it builds, and a query
//! string forwarded as the client wrote it carries the same value in place of
//! the client's parameter.

use crate::forwarded_parameter::ForwardedParameter;
use andromeda_ngsi_ld_model::registration::dataset_overlap::DatasetOverlap;
use url::form_urlencoded;

/// The value of the `datasetId` parameter a source is asked with, its
/// `datasetId`s comma-separated, or `None` when it is asked for none.
pub(crate) fn dataset_ids_parameter(datasets: DatasetOverlap<'_>) -> Option<String> {
    let written = datasets.forwarded().map(ToString::to_string).collect::<Vec<_>>();
    (!written.is_empty()).then(|| written.join(","))
}

/// `query_string` carrying the `datasetId` parameter `datasets` writes in
/// place of the client's: where the client wrote one, after every other
/// parameter when the client wrote none, and nowhere when `datasets` names
/// none.
pub(crate) fn with_forwarded_dataset_ids(query_string: &str, datasets: DatasetOverlap<'_>) -> String {
    let mut forwarded = dataset_ids_parameter(datasets);
    let mut pairs = Vec::new();
    for (key, value) in form_urlencoded::parse(query_string.as_bytes()) {
        if key.parse::<ForwardedParameter>().ok() == Some(ForwardedParameter::DatasetId) {
            if let Some(forwarded) = forwarded.take() {
                pairs.push((key.into_owned(), forwarded));
            }
        } else {
            pairs.push((key.into_owned(), value.into_owned()));
        }
    }
    if let Some(forwarded) = forwarded {
        pairs.push((ForwardedParameter::DatasetId.to_string(), forwarded));
    }
    form_urlencoded::Serializer::new(String::new()).extend_pairs(pairs).finish()
}

#[cfg(test)]
mod tests {
    use crate::forwarded_query::dataset_ids::with_forwarded_dataset_ids;
    use andromeda_ngsi_ld_model::registration::dataset_overlap::DatasetOverlap;
    use andromeda_ngsi_ld_query::dataset::DatasetSelector;
    use pretty_assertions::assert_eq;

    fn datasets(raw: &[&str]) -> Vec<DatasetSelector> {
        raw.iter().map(|selector| selector.parse().expect("a valid datasetId")).collect()
    }

    /// Clause 4.3.6.1: the client's `datasetId` is narrowed in place to the
    /// ones the registration lists, every other parameter travelling as
    /// written.
    #[test]
    fn the_client_dataset_id_is_narrowed_in_place() {
        let requested = datasets(&["urn:ngsi-ld:Dataset:a", "urn:ngsi-ld:Dataset:b"]);
        let listed = datasets(&["urn:ngsi-ld:Dataset:a"]);
        assert_eq!(
            with_forwarded_dataset_ids(
                "type=Vehicle&datasetId=urn:ngsi-ld:Dataset:a,urn:ngsi-ld:Dataset:b&limit=5",
                DatasetOverlap::of(Some(&requested), Some(&listed))
            ),
            "type=Vehicle&datasetId=urn%3Angsi-ld%3ADataset%3Aa&limit=5"
        );
    }

    /// Table 5.2.9-1: a client naming no `datasetId` selects every instance,
    /// so the source is asked for every one its registration lists.
    #[test]
    fn a_query_string_naming_none_gains_the_listed_dataset_ids() {
        let listed = datasets(&["@none", "urn:ngsi-ld:Dataset:a"]);
        assert_eq!(
            with_forwarded_dataset_ids("type=Vehicle", DatasetOverlap::of(None, Some(&listed))),
            "type=Vehicle&datasetId=%40none%2Curn%3Angsi-ld%3ADataset%3Aa"
        );
    }

    /// A registration listing no `datasetId` restricts none, so the query
    /// string keeps the client's `datasetId`, or none.
    #[test]
    fn an_unrestricted_registration_keeps_the_client_dataset_ids() {
        let requested = datasets(&["urn:ngsi-ld:Dataset:b"]);
        assert_eq!(
            with_forwarded_dataset_ids("datasetId=urn:ngsi-ld:Dataset:b&type=Vehicle", DatasetOverlap::of(Some(&requested), None)),
            "datasetId=urn%3Angsi-ld%3ADataset%3Ab&type=Vehicle"
        );
        assert_eq!(with_forwarded_dataset_ids("type=Vehicle", DatasetOverlap::of(None, None)), "type=Vehicle");
    }
}
