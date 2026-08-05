//! The geography expression a distance ordering measures to (clause 4.23).

use andromeda_ngsi_ld_query::ordering::term::OrderTarget;
use andromeda_ngsi_ld_vocabulary::expanded::properties::{LOCATION, VALUE};
use andromeda_postgresql_query::jsonb::{escape_jsonpath, escape_jsonpath_key};

/// SQL expression yielding the geography a distance ordering measures to.
///
/// The default GeoProperty has a dedicated geography column; every other one is
/// decoded from the JSONB payload. Clause 4.23's `id` and `type` keywords carry
/// no geometry at all, so a distance ordering on either measures from nothing
/// and yields NULL, which sorts every row equally and leaves the `id`
/// tiebreaker to decide.
pub fn geo_extract_geography(target: &OrderTarget) -> String {
    let OrderTarget::Attribute(geoproperty_iri) = target else {
        return "NULL::geography".to_string();
    };
    if geoproperty_iri.as_str() == LOCATION.as_str() {
        return "\"location_geo\"".to_string();
    }
    let jsonpath = format!(
        "$[0].\"{}\"[0].\"{}\"[0].\"@value\"",
        escape_jsonpath_key(geoproperty_iri.as_str()),
        escape_jsonpath_key(VALUE.as_str())
    );
    format!(
        "ST_MakeValid(ST_SetSRID(ST_GeomFromGeoJSON((jsonb_path_query_first(\"payload\", '{}')::text)), 4326))::geography",
        escape_jsonpath(&jsonpath)
    )
}

#[cfg(test)]
mod tests {
    use crate::order_geography::geo_extract_geography;
    use andromeda_ngsi_ld_query::ordering::term::OrderTarget;
    use andromeda_ngsi_ld_vocabulary::expanded::properties::LOCATION;
    use iri_rs::IriBuf;

    fn attribute(iri: &str) -> OrderTarget {
        OrderTarget::Attribute(IriBuf::new(iri.to_string()).expect("valid IRI"))
    }

    #[test]
    fn the_default_geoproperty_reads_its_dedicated_geography_column() {
        pretty_assertions::assert_eq!(geo_extract_geography(&attribute(LOCATION.as_str())), "\"location_geo\"");
    }

    #[test]
    fn a_custom_geoproperty_decodes_geojson_from_the_payload() {
        let sql = geo_extract_geography(&attribute("https://example.com/customGeo"));
        assert!(sql.contains("ST_GeomFromGeoJSON"), "{sql}");
        assert!(sql.contains("::geography"), "{sql}");
    }

    #[test]
    fn an_apostrophe_in_the_geoproperty_iri_is_doubled() {
        let sql = geo_extract_geography(&attribute("https://example.com/o'brien"));
        assert!(sql.contains("o''brien"), "{sql}");
        pretty_assertions::assert_eq!(sql.matches('\'').count() % 2, 0, "unbalanced SQL literal: {sql}");
    }

    /// Clause 4.23 admits `id` and `type` as ordering terms; neither names a
    /// GeoProperty, so a distance ordering on one measures from nothing.
    #[test]
    fn a_keyword_order_target_carries_no_geometry() {
        pretty_assertions::assert_eq!(geo_extract_geography(&OrderTarget::Id), "NULL::geography");
        pretty_assertions::assert_eq!(geo_extract_geography(&OrderTarget::Type), "NULL::geography");
    }
}
