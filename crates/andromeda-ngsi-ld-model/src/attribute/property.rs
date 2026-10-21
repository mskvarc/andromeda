//! The Property attribute family (ETSI GS CIM 009 clause 5.2.5).

use crate::attribute::{common_fields::CommonAttributeFields, value::NgsiLdValue};
use andromeda_json_literal::JsonLiteral;
use andromeda_ngsi_ld_lexicon::coerced_term::CoercedTerm;
use cefact_units::UnitCode;
use serde::Serialize;
use serde_with::skip_serializing_none;

/// NGSI-LD Property representation (ETSI GS CIM 009 clause 5.2.5).
#[skip_serializing_none]
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Property {
    /// Property value.
    pub value: NgsiLdValue,
    /// Measurement unit code.
    pub unit_code: Option<UnitCode>,
    /// Optional JSON-LD datatype for `value`, as written (Table 5.2.5-1).
    pub value_type: Option<CoercedTerm>,
    /// Previous value emitted in notifications when `showChanges` is active.
    pub previous_value: Option<NgsiLdValue>,
    /// The non-reified `entityIdSealed` subproperty of the `ngsildproof`
    /// Property, as written (clause 4.5.2.2).
    pub entity_id_sealed: Option<JsonLiteral>,
    /// The non-reified `entityTypeSealed` subproperty of the `ngsildproof`
    /// Property, as written (clause 4.5.2.2).
    pub entity_type_sealed: Option<JsonLiteral>,
    /// Members shared with every other attribute family.
    #[serde(flatten)]
    pub common: CommonAttributeFields,
}

#[cfg(test)]
mod tests {
    use crate::attribute::{instance::Attribute, value::NgsiLdValue};
    use andromeda_json_literal::JsonLiteral;
    use andromeda_ngsi_ld_lexicon::coerced_term::CoercedTerm;
    use cefact_units::UnitCode;

    /// The request representation reads a value as the JSON the client sent:
    /// whether `valueType` makes it a time value, and the one spelling such a
    /// value is held in, is decided when the write is committed.
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
        pretty_assertions::assert_eq!(
            property.value,
            NgsiLdValue::Json(JsonLiteral::admit(r#""2018-12-04T12:00:00.250000Z""#).expect("a string"))
        );
        pretty_assertions::assert_eq!(property.value_type.as_ref().map(CoercedTerm::as_str), Some("DateTime"));
        assert!(property.previous_value.is_some());
        assert!(property.common.instance_id.is_some());
    }

    /// Table 5.2.5-1 types `valueType` as a string "type coerced to a URI based
    /// on the supplied @context", so a hyphenated term the context defines is
    /// read and written back verbatim.
    #[test]
    fn a_hyphenated_value_type_term_is_read_and_serialised_verbatim() {
        let attr: Attribute = serde_json::from_value(serde_json::json!({"type": "Property", "value": 1, "valueType": "my-datatype"}))
            .expect("property with a hyphenated valueType");
        let Attribute::Property(property) = attr else {
            panic!("expected Property");
        };
        pretty_assertions::assert_eq!(property.value_type.as_ref().map(CoercedTerm::as_str), Some("my-datatype"));
        pretty_assertions::assert_eq!(
            serde_json::to_value(&property).expect("serialises")["valueType"],
            serde_json::json!("my-datatype")
        );
    }

    #[test]
    fn a_property_unit_code_serializes_as_its_common_code() {
        let attr: Attribute = serde_json::from_value(serde_json::json!({"type": "Property", "value": 1, "unitCode": "KGM"})).expect("property");
        let Attribute::Property(property) = attr else {
            panic!("expected Property");
        };
        pretty_assertions::assert_eq!(property.unit_code, Some(UnitCode::Kgm));
        pretty_assertions::assert_eq!(serde_json::to_value(&property).expect("serialises")["unitCode"], serde_json::json!("KGM"));
    }

    #[test]
    fn a_value_type_holding_whitespace_is_refused() {
        assert!(serde_json::from_value::<Attribute>(serde_json::json!({"type": "Property", "value": 1, "valueType": "my datatype"})).is_err());
    }
}
