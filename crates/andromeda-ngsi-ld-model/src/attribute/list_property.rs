//! The `ListProperty` attribute family (ETSI GS CIM 009 clause 5.2.36).

use crate::attribute::{common_fields::CommonAttributeFields, value::NgsiLdValue};
use andromeda_ngsi_ld_lexicon::reference::NgsiLdReference;
use serde::{Deserialize, Serialize};
use serde_with::skip_serializing_none;

/// NGSI-LD `ListProperty` representation (ETSI GS CIM 009 clause 5.2.36).
#[skip_serializing_none]
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListProperty {
    /// Ordered list of values.
    pub value_list: Vec<NgsiLdValue>,
    /// Optional JSON-LD datatype for `valueList`.
    pub value_type: Option<NgsiLdReference>,
    /// Previous value list emitted in notifications when `showChanges` is active.
    pub previous_value_list: Option<Vec<NgsiLdValue>>,
    /// Members shared with every other attribute family.
    #[serde(flatten)]
    pub common: CommonAttributeFields,
}

#[cfg(test)]
mod tests {
    use crate::attribute::{instance::Attribute, value::NgsiLdValue};

    #[test]
    fn a_list_property_keeps_its_value_list_in_order() {
        let json = serde_json::json!({
            "type": "ListProperty",
            "valueList": [1, 2, 3],
            "previousValueList": [0, 1, 2]
        });
        let attr: Attribute = serde_json::from_value(json).expect("list_property");
        let Attribute::ListProperty(lp) = &attr else {
            panic!("expected ListProperty");
        };
        let values: Vec<&NgsiLdValue> = lp.value_list.iter().collect();
        pretty_assertions::assert_eq!(
            values,
            vec![
                &NgsiLdValue::Json(serde_json::json!(1)),
                &NgsiLdValue::Json(serde_json::json!(2)),
                &NgsiLdValue::Json(serde_json::json!(3))
            ]
        );
        assert!(lp.previous_value_list.is_some());
    }

    #[test]
    fn the_list_property_default_matches_the_field_by_field_literal() {
        use crate::attribute::{common_fields::CommonAttributeFields, list_property::ListProperty};
        let expected = ListProperty {
            value_list: Vec::new(),
            value_type: None,
            previous_value_list: None,
            common: CommonAttributeFields::default(),
        };
        let actual = ListProperty::default();
        pretty_assertions::assert_eq!(actual.value_list.len(), expected.value_list.len());
        assert!(actual.value_type.is_none() && actual.previous_value_list.is_none());
        assert!(actual.common.sub_attributes.is_empty());
        assert!(actual.common.dataset_id.is_none() && actual.common.observed_at.is_none() && actual.common.expires_at.is_none());
    }
}
