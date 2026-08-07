//! Compilation of the geo query parameter group into a [`GeoQuery`].

use crate::{
    error::QueryError,
    geo::{input::GeoQueryInput, query::GeoQuery},
    term_expander::TermExpander,
};

/// The GeoProperty a geo query addresses when the request names none.
const DEFAULT_GEOPROPERTY: &str = "location";

/// Compile geo query parameters into a [`GeoQuery`].
///
/// All three of `georel`, `geometry`, `coordinates` must be present together or
/// all absent (clause 4.10). `geoproperty` defaults to `location`.
///
/// `geometry` and `coordinates` are the two halves of one GeoJSON geometry
/// object (clause 4.10, IETF RFC 7946), so they are rejoined and parsed here.
pub async fn compile_geo_query(input: Option<&GeoQueryInput>, expander: &impl TermExpander) -> Result<Option<GeoQuery>, QueryError> {
    let Some(input) = input else {
        return Ok(None);
    };
    let geoproperty = input.geoproperty.as_ref().map_or(DEFAULT_GEOPROPERTY, AsRef::<str>::as_ref);
    Ok(Some(GeoQuery {
        georel: input.georel.clone(),
        geometry: input.geometry.clone(),
        geoproperty_iri: expander.expand_term(geoproperty).await?,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use iri_rs::IriBuf;

    struct PrefixExpander;

    impl TermExpander for PrefixExpander {
        async fn expand_term(&self, term: &str) -> Result<IriBuf, QueryError> {
            Ok(IriBuf::new(format!("https://example.com/{term}")).expect("valid IRI"))
        }

        async fn expand_type(&self, term: &str) -> Result<IriBuf, QueryError> {
            Ok(IriBuf::new(format!("https://example.com/types/{term}")).expect("valid IRI"))
        }
    }

    #[tokio::test]
    async fn expansion_consumes_a_decoded_geo_query_without_reparsing_components() {
        let input = GeoQueryInput::from_wire("within".to_string(), "Point".to_string(), serde_json::json!([1.0, 2.0]), None).expect("valid decoded geo query");
        let compiled = compile_geo_query(Some(&input), &PrefixExpander)
            .await
            .expect("expansion succeeds")
            .expect("geo query present");
        assert_eq!(compiled.geoproperty_iri.as_str(), "https://example.com/location");
    }

    #[test]
    fn malformed_geo_wire_values_fail_before_compilation() {
        assert!(GeoQueryInput::from_wire("within".to_string(), "Point".to_string(), serde_json::json!([1.0]), None).is_err());
    }
}
