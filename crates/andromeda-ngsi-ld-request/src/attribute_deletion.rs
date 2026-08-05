//! The parameters of the attribute-deletion endpoints.

use crate::case_insensitive_bool::deserialize_option_bool_case_insensitive;
use serde::Deserialize;

/// Query parameters for `DELETE /entities/{id}/attrs/{attrId}` and its temporal
/// counterpart (clauses 5.6.5 and 5.6.15).
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeleteAttrParams {
    /// The dataset instance to delete, if only one is meant.
    #[serde(rename = "datasetId")]
    pub dataset_id: Option<String>,
    /// Whether every instance of the attribute is deleted.
    #[serde(rename = "deleteAll", default, deserialize_with = "deserialize_option_bool_case_insensitive")]
    pub delete_all: Option<bool>,
}

#[cfg(test)]
mod tests {
    use crate::attribute_deletion::DeleteAttrParams;

    #[test]
    fn both_parameters_default_to_absent() {
        let params: DeleteAttrParams = serde_urlencoded::from_str("").expect("parses");
        pretty_assertions::assert_eq!(params.dataset_id, None);
        pretty_assertions::assert_eq!(params.delete_all, None);
    }

    #[test]
    fn both_parameters_are_read_and_the_flag_is_case_insensitive() {
        let params: DeleteAttrParams = serde_urlencoded::from_str("datasetId=urn:test&deleteAll=TRUE").expect("parses");
        pretty_assertions::assert_eq!(params.dataset_id.as_deref(), Some("urn:test"));
        pretty_assertions::assert_eq!(params.delete_all, Some(true));
    }
}
