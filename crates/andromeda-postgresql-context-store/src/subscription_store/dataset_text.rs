//! The text a subscription's `datasetId` selectors are stored as.
//!
//! Table 5.2.12-1 writes each selector as a URI or `@none`, which is what a
//! retrieve echoes, so the column holds the rendering of each selector in the
//! order the subscriber wrote them.

use andromeda_ngsi_ld_query::dataset_selector::DatasetSelector;

/// The stored text of each of `selectors`, or `None` when the subscription
/// names no `datasetId`.
pub(crate) fn stored_dataset_ids(selectors: Option<&[DatasetSelector]>) -> Option<Vec<String>> {
    selectors.map(|selectors| selectors.iter().map(ToString::to_string).collect())
}

#[cfg(test)]
mod tests {
    use crate::subscription_store::dataset_text::stored_dataset_ids;
    use andromeda_ngsi_ld_query::dataset_selector::DatasetSelector;
    use pretty_assertions::assert_eq;

    #[test]
    fn each_selector_is_stored_as_the_text_it_renders_as_in_the_order_written() {
        let selectors: Vec<DatasetSelector> = ["urn:ngsi-ld:Dataset:b", "@none"]
            .iter()
            .map(|raw| raw.parse().expect("a valid datasetId"))
            .collect();
        assert_eq!(
            stored_dataset_ids(Some(&selectors)),
            Some(vec!["urn:ngsi-ld:Dataset:b".to_owned(), "@none".to_owned()])
        );
    }

    #[test]
    fn no_dataset_id_stores_nothing() {
        assert_eq!(stored_dataset_ids(None), None);
    }
}
