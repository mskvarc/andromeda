//! Compilation of the geo query parameter group into a [`GeoQuery`].

use crate::{
    error::QueryError,
    filter::path::expanded_attribute_name,
    geo::{input::GeoQueryInput, query::GeoQuery},
    term_expander::TermExpander,
};
use andromeda_ngsi_ld_vocabulary::compact::properties;

/// Compile geo query parameters into a [`GeoQuery`].
///
/// All three of `georel`, `geometry`, `coordinates` must be present together or
/// all absent (clause 4.10). `geoproperty` defaults to `location`.
///
/// `geometry` and `coordinates` are the two halves of one `GeoJSON` geometry
/// object (clause 4.10, IETF RFC 7946), so they are rejoined and parsed here.
///
/// # Errors
///
/// Returns [`QueryError::TermExpansion`], [`QueryError::TermNotAnIri`] or
/// [`QueryError::AttributeNameInvalid`] when the `geoproperty` term does not
/// expand to a usable attribute name.
pub async fn compile_geo_query(input: Option<&GeoQueryInput>, expander: &impl TermExpander) -> Result<Option<GeoQuery>, QueryError> {
    let Some(input) = input else {
        return Ok(None);
    };
    let geoproperty = input.geoproperty.as_ref().map_or(properties::LOCATION, AsRef::<str>::as_ref);
    Ok(Some(GeoQuery {
        georel: input.georel.clone(),
        geometry: input.geometry.clone(),
        geoproperty: expanded_attribute_name(&expander.expand_term(geoproperty).await?)?,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use iri_rs::IriBuf;
    use std::future::{Future, ready};

    struct PrefixExpander;

    impl TermExpander for PrefixExpander {
        fn expand_term(&self, term: &str) -> impl Future<Output = Result<IriBuf, QueryError>> + Send {
            ready(Ok(IriBuf::new(format!("https://example.com/{term}")).expect("valid IRI")))
        }

        fn expand_type(&self, term: &str) -> impl Future<Output = Result<IriBuf, QueryError>> + Send {
            ready(Ok(IriBuf::new(format!("https://example.com/types/{term}")).expect("valid IRI")))
        }
    }

    #[tokio::test]
    async fn expansion_consumes_a_decoded_geo_query_without_reparsing_components() {
        let input = GeoQueryInput::from_wire("within", "Point", serde_json::json!([1.0, 2.0]), None).expect("valid decoded geo query");
        let compiled = compile_geo_query(Some(&input), &PrefixExpander)
            .await
            .expect("expansion succeeds")
            .expect("geo query present");
        assert_eq!(compiled.geoproperty.as_str(), "https://example.com/location");
    }

    #[test]
    fn malformed_geo_wire_values_fail_before_compilation() {
        assert!(GeoQueryInput::from_wire("within", "Point", serde_json::json!([1.0]), None).is_err());
    }
}
