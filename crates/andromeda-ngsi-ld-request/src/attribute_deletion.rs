//! The parameters of the attribute-deletion endpoints.

use crate::{case_insensitive_bool::deserialize_option_bool_case_insensitive, query::deserialization::deserialize_optional_dataset_selector};
use andromeda_ngsi_ld_query::{dataset::DatasetSelector, type_selection::grammar::TypeExprRaw};
use serde::Deserialize;

/// Declare a parameter struct of an attribute deletion: the `datasetId`,
/// `deleteAll` and `local` members every attribute deletion shares, then the
/// `extra` members of the one operation.
///
/// A macro, because the two operations differ by one member and serde's
/// `flatten`, the other way to share the members, cannot be combined with
/// `deny_unknown_fields`, which clause 6.3.20 needs to refuse an unknown
/// parameter.
macro_rules! attribute_deletion_params {
    ($(#[$meta:meta])* $name:ident { $($extra:tt)* }) => {
        $(#[$meta])*
        #[derive(Debug, Deserialize)]
        #[serde(deny_unknown_fields)]
        pub struct $name {
            /// The dataset instance to delete, if only one is meant.
            #[serde(rename = "datasetId")]
            #[serde(default, deserialize_with = "deserialize_optional_dataset_selector")]
            pub dataset_id: Option<DatasetSelector>,
            /// Whether every instance of the attribute is deleted.
            #[serde(rename = "deleteAll", default, deserialize_with = "deserialize_option_bool_case_insensitive")]
            pub delete_all: Option<bool>,
            $($extra)*
            /// Whether the operation is confined to this broker (clause 6.3.18).
            #[serde(default, deserialize_with = "deserialize_option_bool_case_insensitive")]
            pub local: Option<bool>,
        }
    };
}

attribute_deletion_params! {
    /// Query parameters for `DELETE /entities/{id}/attrs/{attrId}` (Delete
    /// Attribute, clause 5.6.5, Table 6.7.3.2-1).
    EntityAttributeDeletionParams {
        /// The clause 4.17 selector the target entity's types must satisfy.
        #[serde(rename = "type")]
        pub entity_type: Option<TypeExprRaw>,
    }
}

attribute_deletion_params! {
    /// Query parameters for `DELETE /temporal/entities/{id}/attrs/{attrId}`
    /// (clause 5.6.13, Table 6.21.3.1-1), which defines no `type` selector.
    DeleteAttrParams {}
}

#[cfg(test)]
mod tests {
    use crate::attribute_deletion::{DeleteAttrParams, EntityAttributeDeletionParams};

    /// Table 6.7.3.2-1 adds `type` to the parameters the temporal deletion of
    /// Table 6.21.3.1-1 has, which does not take it.
    #[test]
    fn only_the_entity_attribute_deletion_takes_a_type_selector() {
        let params: EntityAttributeDeletionParams =
            serde_urlencoded::from_str("type=Vehicle&deleteAll=true&datasetId=urn:ngsi-ld:Dataset:test&local=true").expect("every parameter parses");
        pretty_assertions::assert_eq!(params.entity_type.map(|selector| selector.to_string()), Some("Vehicle".to_string()));
        pretty_assertions::assert_eq!(params.delete_all, Some(true));
        pretty_assertions::assert_eq!(params.local, Some(true));
        assert!(params.dataset_id.is_some());
        let bare: EntityAttributeDeletionParams = serde_urlencoded::from_str("").expect("parses");
        assert!(bare.entity_type.is_none() && bare.dataset_id.is_none() && bare.delete_all.is_none() && bare.local.is_none());
        assert!(serde_urlencoded::from_str::<DeleteAttrParams>("type=Vehicle").is_err());
    }

    #[test]
    fn every_parameter_defaults_to_absent() {
        let params: DeleteAttrParams = serde_urlencoded::from_str("").expect("parses");
        pretty_assertions::assert_eq!(params.dataset_id, None);
        pretty_assertions::assert_eq!(params.delete_all, None);
        pretty_assertions::assert_eq!(params.local, None);
    }

    #[test]
    fn the_local_scope_flag_is_read() {
        let params: DeleteAttrParams = serde_urlencoded::from_str("local=true").expect("parses");
        pretty_assertions::assert_eq!(params.local, Some(true));
    }

    #[test]
    fn both_parameters_are_read_and_the_flag_is_case_insensitive() {
        let params: DeleteAttrParams = serde_urlencoded::from_str("datasetId=urn:ngsi-ld:Dataset:test&deleteAll=TRUE").expect("parses");
        pretty_assertions::assert_eq!(params.dataset_id.as_ref().map(ToString::to_string).as_deref(), Some("urn:ngsi-ld:Dataset:test"));
        pretty_assertions::assert_eq!(params.delete_all, Some(true));
    }

    #[test]
    fn singleton_dataset_id_rejects_a_comma_separated_list() {
        assert!(serde_urlencoded::from_str::<DeleteAttrParams>("datasetId=urn:ngsi-ld:Dataset:a,urn:ngsi-ld:Dataset:b").is_err());
    }
}
