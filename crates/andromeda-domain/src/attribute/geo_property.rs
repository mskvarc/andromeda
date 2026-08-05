//! The GeoProperty attribute family (ETSI GS CIM 009 clause 5.2.7).

use crate::attribute::common_fields::CommonAttributeFields;
use andromeda_geometry::geometry::Geometry;
use compact_str::CompactString;
use jsonld::Expandable;
use serde::{Deserialize, Serialize};
use serde_with::skip_serializing_none;

/// NGSI-LD GeoProperty representation (ETSI GS CIM 009 clause 5.2.7).
#[skip_serializing_none]
#[derive(Debug, Clone, Serialize, Deserialize, Expandable)]
#[serde(rename_all = "camelCase")]
#[jsonld(
    type = "ngsi:GeoProperty",
    prefix(ngsi = "https://uri.etsi.org/ngsi-ld/", xsd = "http://www.w3.org/2001/XMLSchema#")
)]
pub struct GeoProperty {
    /// Geometry value, held and serialised as GeoJSON per RFC 7946.
    #[jsonld(property = "ngsi:hasValue", coerce = "@json")]
    pub value: Geometry,
    /// Optional JSON-LD datatype for `value`.
    #[jsonld(property = "ngsi:hasValueType", coerce = "@id")]
    pub value_type: Option<CompactString>,
    /// Previous geometry value emitted in notifications when `showChanges` is active.
    #[jsonld(property = "ngsi:hasPreviousValue", coerce = "@json")]
    pub previous_value: Option<Geometry>,
    /// Members shared with every other attribute family.
    #[serde(flatten)]
    #[jsonld(flatten)]
    pub common: CommonAttributeFields,
}

#[cfg(test)]
mod tests {
    use crate::attribute::instance::Attribute;
    use andromeda_ngsi_ld_vocabulary::expanded::properties as expanded_properties;
    use jsonld::Expandable;

    #[test]
    fn geo_property_round_trip_supports_previous_value() {
        let json = serde_json::json!({
            "type": "GeoProperty",
            "value": { "type": "Point", "coordinates": [13.0, 52.0] },
            "previousValue": { "type": "Point", "coordinates": [12.0, 51.0] }
        });
        let attr: Attribute = serde_json::from_value(json).expect("geo property");
        let Attribute::GeoProperty(property) = attr else {
            unreachable!("expected GeoProperty");
        };
        assert!(property.previous_value.is_some());
    }

    #[test]
    fn geo_property_expanded_includes_value_type() {
        let json = serde_json::json!({
            "type": "GeoProperty",
            "value": { "type": "Point", "coordinates": [13.0, 52.0] },
            "valueType": "geojson:Point"
        });
        let attr: Attribute = serde_json::from_value(json).expect("geo property");
        let Attribute::GeoProperty(property) = attr else {
            unreachable!("expected GeoProperty");
        };
        let expanded = property.expand::<serde_json::Value>();
        let obj = expanded.as_object().expect("object");
        assert!(obj.contains_key(expanded_properties::VALUE_TYPE.as_str()));
    }
}
