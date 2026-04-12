use crate::{
    domain::attribute::{
        AttributeValue,
        common::{CommonAttributeFields, NgsiLdValue},
    },
    jsonld::JsonLdExpandableMacro,
};
use compact_str::CompactString;
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use serde_with::skip_serializing_none;

/// NGSI-LD Property representation (ETSI GS CIM 009 clause 5.2.5).
#[skip_serializing_none]
#[derive(Debug, Clone, Serialize, Deserialize, JsonLdExpandableMacro)]
#[serde(rename_all = "camelCase")]
#[jsonld(type = "ngsi:Property")]
pub struct Property {
    /// Property value.
    #[jsonld(property = "ngsi:hasValue")]
    pub value: NgsiLdValue,
    /// Measurement unit code.
    #[jsonld(property = "ngsi:unitCode", vocab)]
    pub unit_code: Option<CompactString>,
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
    use crate::{
        domain::attribute::Attribute,
        jsonld::{JsonLdExpandable, vocab},
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
            panic!("expected Property");
        };
        assert!(matches!(property.value, crate::domain::attribute::common::NgsiLdValue::DateTime(_)));
        assert_eq!(property.value_type.as_deref(), Some("DateTime"));
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
            panic!("expected Property");
        };
        assert_eq!(property.value_type.as_deref(), Some("Integer"));
        let expanded = property.to_expanded();
        let obj = expanded.as_object().expect("object");
        assert!(obj.contains_key(vocab::VALUE_TYPE.as_str()));
        assert!(obj.contains_key(vocab::CREATED_AT.as_str()));
        assert!(obj.contains_key(vocab::MODIFIED_AT.as_str()));
    }
}
