//! The `ListProperty` attribute family (ETSI GS CIM 009 clause 5.2.36).

use crate::attribute::{common_fields::CommonAttributeFields, value::NgsiLdValue};
use andromeda_ngsi_ld_lexicon::coerced_term::CoercedTerm;
use cefact_units::UnitCode;
use serde::Serialize;
use serde_with::skip_serializing_none;

/// NGSI-LD `ListProperty` representation (ETSI GS CIM 009 clause 5.2.36).
#[skip_serializing_none]
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ListProperty {
    /// Ordered list of values.
    pub value_list: Vec<NgsiLdValue>,
    /// Measurement unit code: clause 4.5.21.2 keeps the `unitCode` a
    /// `ListProperty` inherits from the Property of clause 4.5.2.2.
    pub unit_code: Option<UnitCode>,
    /// Optional JSON-LD datatype for `valueList`.
    pub value_type: Option<CoercedTerm>,
    /// Previous value list emitted in notifications when `showChanges` is active.
    pub previous_value_list: Option<Vec<NgsiLdValue>>,
    /// Members shared with every other attribute family.
    #[serde(flatten)]
    pub common: CommonAttributeFields,
}

#[cfg(test)]
mod tests {
    use crate::attribute::{instance::Attribute, value::NgsiLdValue};
    use andromeda_json_literal::JsonLiteral;

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
                &NgsiLdValue::from(JsonLiteral::admit("1").expect("a number")),
                &NgsiLdValue::from(JsonLiteral::admit("2").expect("a number")),
                &NgsiLdValue::from(JsonLiteral::admit("3").expect("a number"))
            ]
        );
        assert!(lp.previous_value_list.is_some());
    }

    /// Clause 4.5.21.2: a `ListProperty` keeps the `unitCode` of the Property it
    /// derives from, as its own member rather than a sub-attribute.
    #[test]
    fn a_list_property_reads_its_unit_code_as_its_own_member() {
        let json = serde_json::json!({"type": "ListProperty", "valueList": [300, 120], "unitCode": "MMT"});
        let attr: Attribute = serde_json::from_value(json).expect("list_property");
        let Attribute::ListProperty(lp) = &attr else {
            panic!("expected ListProperty");
        };
        pretty_assertions::assert_eq!(lp.unit_code.map(|code| code.as_str().to_owned()).as_deref(), Some("MMT"));
        assert!(lp.common.sub_attributes.is_empty(), "{lp:?}");
        pretty_assertions::assert_eq!(
            serde_json::to_value(&attr).expect("serializes"),
            serde_json::json!({"type": "ListProperty", "valueList": [300, 120], "unitCode": "MMT"})
        );
    }

    /// Clause 4.7.3: a `GeoProperty` carrying further members is an ordinary
    /// Property, so its `unitCode` is its own member as well.
    #[test]
    fn a_geo_property_reads_its_unit_code_as_its_own_member() {
        let json = serde_json::json!({"type": "GeoProperty", "value": {"type": "Point", "coordinates": [13.0, 52.0]}, "unitCode": "MMT"});
        let attr: Attribute = serde_json::from_value(json).expect("geo_property");
        let Attribute::GeoProperty(geo) = &attr else {
            panic!("expected GeoProperty");
        };
        pretty_assertions::assert_eq!(geo.unit_code.map(|code| code.as_str().to_owned()).as_deref(), Some("MMT"));
        assert!(geo.common.sub_attributes.is_empty(), "{geo:?}");
    }

    #[test]
    fn the_list_property_default_matches_the_field_by_field_literal() {
        use crate::attribute::{common_fields::CommonAttributeFields, list_property::ListProperty};
        let expected = ListProperty {
            value_list: Vec::new(),
            unit_code: None,
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
