//! The ListProperty attribute family (ETSI GS CIM 009 clause 5.2.36).

use crate::attribute::{common_fields::CommonAttributeFields, value::NgsiLdValue};
use andromeda_ngsi_ld_lexicon::reference::NgsiLdReference;
use jsonld::Expandable;
use serde::{Deserialize, Serialize};
use serde_with::skip_serializing_none;

/// NGSI-LD ListProperty representation (ETSI GS CIM 009 clause 5.2.36).
#[skip_serializing_none]
#[derive(Debug, Clone, Default, Serialize, Deserialize, Expandable)]
#[serde(rename_all = "camelCase")]
#[jsonld(
    type = "ngsi:ListProperty",
    prefix(ngsi = "https://uri.etsi.org/ngsi-ld/", xsd = "http://www.w3.org/2001/XMLSchema#")
)]
pub struct ListProperty {
    /// Ordered list of values.
    #[jsonld(property = "ngsi:hasValueList", container = "list")]
    pub value_list: Vec<NgsiLdValue>,
    /// Optional JSON-LD datatype for `valueList`.
    #[jsonld(property = "ngsi:hasValueType", coerce = "@id")]
    pub value_type: Option<NgsiLdReference>,
    /// Previous value list emitted in notifications when `showChanges` is active.
    #[jsonld(property = "ngsi:hasPreviousValueList", container = "list")]
    pub previous_value_list: Option<Vec<NgsiLdValue>>,
    /// Members shared with every other attribute family.
    #[serde(flatten)]
    #[jsonld(flatten)]
    pub common: CommonAttributeFields,
}

#[cfg(test)]
mod tests {
    use crate::attribute::instance::Attribute;
    use jsonld::Expandable;

    #[test]
    fn list_property_expanded_has_list_container() {
        let json = serde_json::json!({
            "type": "ListProperty",
            "valueList": [1, 2, 3],
            "previousValueList": [0, 1, 2]
        });
        let attr: Attribute = serde_json::from_value(json).expect("list_property");
        let Attribute::ListProperty(lp) = &attr else {
            unreachable!("expected ListProperty");
        };
        let expanded = lp.expand::<serde_json::Value>();
        let obj = expanded.as_object().expect("expanded is object");

        pretty_assertions::assert_eq!(obj["@type"], serde_json::json!(["https://uri.etsi.org/ngsi-ld/ListProperty"]));
        let has_value_list = &obj["https://uri.etsi.org/ngsi-ld/hasValueList"];
        pretty_assertions::assert_eq!(has_value_list, &serde_json::json!([{"@list": [1, 2, 3]}]));
        assert!(obj.contains_key("https://uri.etsi.org/ngsi-ld/hasPreviousValueList"));
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
