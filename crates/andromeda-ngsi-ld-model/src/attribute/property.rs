//! The Property attribute family (ETSI GS CIM 009 clause 5.2.5).

use crate::attribute::{common_fields::CommonAttributeFields, value::NgsiLdValue};
use andromeda_ngsi_ld_lexicon::reference::NgsiLdReference;
use cefact_units::UnitCode;
use derive_more::{Deref, From, Into};
use jsonld::Expandable;
use serde::{Deserialize, Serialize};
use serde_with::skip_serializing_none;

/// Newtype wrapping [`UnitCode`] to provide an [`AsRef<str>`] implementation
/// (orphan rules forbid implementing the foreign trait for the foreign type
/// directly). Used as a field value where the JSON-LD derive needs `&str` access.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, From, Into, Deref)]
#[serde(transparent)]
pub struct UnitCodeRef(pub UnitCode);

// `#[as_ref(str)]` needs the field type to implement `AsRef<str>`, and
// `cefact_units::UnitCode` exposes `as_str` without it, so the forwarding
// cannot be derived.
impl AsRef<str> for UnitCodeRef {
    fn as_ref(&self) -> &str {
        self.0.as_str()
    }
}

/// NGSI-LD Property representation (ETSI GS CIM 009 clause 5.2.5).
#[skip_serializing_none]
#[derive(Debug, Clone, Default, Serialize, Deserialize, Expandable)]
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
    #[jsonld(property = "ngsi:unitCode", coerce = "@id")]
    pub unit_code: Option<UnitCodeRef>,
    /// Optional JSON-LD datatype for `value`.
    #[jsonld(property = "ngsi:hasValueType", coerce = "@id")]
    pub value_type: Option<NgsiLdReference>,
    /// Previous value emitted in notifications when `showChanges` is active.
    #[jsonld(property = "ngsi:hasPreviousValue")]
    pub previous_value: Option<NgsiLdValue>,
    /// Members shared with every other attribute family.
    #[serde(flatten)]
    #[jsonld(flatten)]
    pub common: CommonAttributeFields,
}

#[cfg(test)]
mod tests {
    use crate::attribute::{instance::Attribute, value::NgsiLdValue};
    use andromeda_ngsi_ld_vocabulary::expanded::properties as expanded_properties;
    use jsonld::Expandable;
    use serde_json::Value;

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
        assert!(matches!(property.value, NgsiLdValue::DateTime(_)));
        pretty_assertions::assert_eq!(property.value_type.as_deref().map(|value| value.as_str()), Some("DateTime"));
        assert!(property.previous_value.is_some());
        assert!(property.common.instance_id.is_some());
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
        pretty_assertions::assert_eq!(property.value_type.as_deref().map(|value| value.as_str()), Some("Integer"));
        let expanded = property.expand::<serde_json::Value>();
        let obj = expanded.as_object().expect("object");
        assert!(obj.contains_key(expanded_properties::VALUE_TYPE.as_str()));
        assert!(obj.contains_key(expanded_properties::CREATED_AT.as_str()));
        assert!(obj.contains_key(expanded_properties::MODIFIED_AT.as_str()));
    }

    #[test]
    fn the_property_default_matches_the_field_by_field_literal() {
        use crate::attribute::property::Property;
        let actual = Property::default();
        pretty_assertions::assert_eq!(actual.value, NgsiLdValue::Json(Value::Null));
        assert!(actual.unit_code.is_none() && actual.value_type.is_none() && actual.previous_value.is_none());
        assert!(actual.common.sub_attributes.is_empty());
        assert!(actual.common.dataset_id.is_none() && actual.common.observed_at.is_none() && actual.common.expires_at.is_none());
    }
}
