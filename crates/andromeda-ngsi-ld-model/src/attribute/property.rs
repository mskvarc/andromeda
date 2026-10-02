//! The Property attribute family (ETSI GS CIM 009 clause 5.2.5).

use crate::attribute::{common_fields::CommonAttributeFields, value::NgsiLdValue};
use andromeda_ngsi_ld_lexicon::reference::NgsiLdReference;
use cefact_units::UnitCode;
use derive_more::{Deref, From, Into};
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
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Property {
    /// Property value.
    pub value: NgsiLdValue,
    /// Measurement unit code.
    pub unit_code: Option<UnitCodeRef>,
    /// Optional JSON-LD datatype for `value`.
    pub value_type: Option<NgsiLdReference>,
    /// Previous value emitted in notifications when `showChanges` is active.
    pub previous_value: Option<NgsiLdValue>,
    /// Members shared with every other attribute family.
    #[serde(flatten)]
    pub common: CommonAttributeFields,
}

#[cfg(test)]
mod tests {
    use crate::attribute::{instance::Attribute, value::NgsiLdValue};
    use andromeda_ngsi_ld_lexicon::reference::NgsiLdReference;
    use serde_json::Value;

    /// Clause 4.6.3 admits several spellings of one `DateTime`, so the document
    /// representation carries the one the client wrote rather than re-rendering
    /// it. What the value *is* under the clause is decided by
    /// `andromeda_ngsi_ld_lexicon::time_value::TimeValue`, where the write
    /// projection and the clause 4.9 `q` operand parser both read it; this type
    /// carries the text so that a `2018-12-04T12:00:00.250000Z` a client wrote
    /// is not served back as `2018-12-04T12:00:00.250Z`.
    #[test]
    fn property_round_trip_carries_the_written_value_alongside_value_type_and_previous_value() {
        let json = serde_json::json!({
            "type": "Property",
            "value": "2018-12-04T12:00:00.250000Z",
            "valueType": "DateTime",
            "previousValue": "2018-12-03T12:00:00Z",
            "instanceId": "urn:ngsi-ld:attribute:instance:1"
        });
        let attr: Attribute = serde_json::from_value(json).expect("property");
        let Attribute::Property(property) = attr else {
            panic!("expected Property");
        };
        pretty_assertions::assert_eq!(property.value, NgsiLdValue::Json(Value::String("2018-12-04T12:00:00.250000Z".to_owned())));
        pretty_assertions::assert_eq!(property.value_type.as_ref().map(NgsiLdReference::as_str), Some("DateTime"));
        assert!(property.previous_value.is_some());
        assert!(property.common.instance_id.is_some());
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
