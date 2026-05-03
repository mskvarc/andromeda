use crate::{
    domain::attribute::{
        AttributeValue,
        common::{CommonAttributeFields, NgsiLdValue},
    },
    jsonld::Expandable,
};
use cefact_units::UnitCode;
use compact_str::CompactString;
use derive_more::{Deref, From};
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use serde_with::skip_serializing_none;

/// Newtype wrapping [`UnitCode`] to provide an [`AsRef<str>`] implementation
/// (orphan rules forbid implementing the foreign trait for the foreign type
/// directly). Used as a field value where the JSON-LD derive needs `&str` access.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, From, Deref)]
#[serde(transparent)]
pub struct UnitCodeRef(pub UnitCode);

impl AsRef<str> for UnitCodeRef {
    fn as_ref(&self) -> &str {
        self.0.as_str()
    }
}

impl From<UnitCodeRef> for UnitCode {
    fn from(value: UnitCodeRef) -> UnitCode {
        value.0
    }
}

/// NGSI-LD Property representation (ETSI GS CIM 009 clause 5.2.5).
#[skip_serializing_none]
#[derive(Debug, Clone, Serialize, Deserialize, Expandable)]
#[serde(rename_all = "camelCase")]
#[jsonld(
    type = "ngsi:Property",
    prefix(ngsi = "https://uri.etsi.org/ngsi-ld/", xsd = "http://www.w3.org/2001/XMLSchema#")
)]
pub struct Property {
    /// Property value.
    #[jsonld(property = "ngsi:hasValue")]
    pub value: NgsiLdValue,
    /// Measurement unit code.
    #[jsonld(property = "ngsi:unitCode", vocab)]
    pub unit_code: Option<UnitCodeRef>,
    /// Optional JSON-LD datatype for `value`.
    #[jsonld(property = "ngsi:hasValueType", vocab)]
    pub value_type: Option<CompactString>,
    /// Previous value emitted in notifications when `showChanges` is active.
    #[jsonld(property = "ngsi:hasPreviousValue")]
    pub previous_value: Option<NgsiLdValue>,
    #[serde(flatten)]
    #[jsonld(flatten_object)]
    pub common: CommonAttributeFields,
    #[serde(flatten)]
    #[jsonld(flatten_map)]
    pub sub_attributes: IndexMap<CompactString, AttributeValue>,
}

impl Default for Property {
    fn default() -> Property {
        Property {
            value: NgsiLdValue::Json(serde_json::Value::Null),
            unit_code: None,
            value_type: None,
            previous_value: None,
            common: CommonAttributeFields::default(),
            sub_attributes: IndexMap::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used)]
    use crate::{
        domain::attribute::Attribute,
        jsonld::{Expandable, vocab},
    };

    #[test]
    fn property_round_trip_supports_value_type_and_previous_value() {
        let json = serde_json::json!({
            "type": "Property",
            "value": "2018-12-04T12:00:00Z",
            "valueType": "DateTime",
            "previousValue": "2018-12-03T12:00:00Z",
            "instanceId": "urn:ngsi-ld:attribute:instance:1"
        });
        let attr: Attribute = serde_json::from_value(json).expect("property");
        let Attribute::Property(property) = attr else {
            unreachable!("expected Property");
        };
        assert!(matches!(property.value, crate::domain::attribute::common::NgsiLdValue::DateTime(_)));
        pretty_assertions::assert_eq!(property.value_type.as_deref(), Some("DateTime"));
        assert!(property.previous_value.is_some());
        assert!(property.common.system.instance_id.is_some());
    }

    #[test]
    fn property_expanded_includes_output_only_and_value_type_members() {
        let json = serde_json::json!({
            "type": "Property",
            "value": 25,
            "valueType": "Integer",
            "createdAt": "2024-01-01T00:00:00Z",
            "modifiedAt": "2024-01-02T00:00:00Z"
        });
        let attr: Attribute = serde_json::from_value(json).expect("property");
        let Attribute::Property(property) = attr else {
            unreachable!("expected Property");
        };
        pretty_assertions::assert_eq!(property.value_type.as_deref(), Some("Integer"));
        let expanded = property.expand::<serde_json::Value>();
        let obj = expanded.as_object().expect("object");
        assert!(obj.contains_key(vocab::expanded::properties::VALUE_TYPE.as_str()));
        assert!(obj.contains_key(vocab::expanded::properties::CREATED_AT.as_str()));
        assert!(obj.contains_key(vocab::expanded::properties::MODIFIED_AT.as_str()));
    }
}
