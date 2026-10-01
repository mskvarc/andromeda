//! Compilation of the geo query parameter group into a [`GeoQuery`].

use crate::{
    error::QueryError,
    filter::path::expanded_attribute_name,
    geo::{input::GeoQueryInput, query::GeoQuery},
    term_expander::TermExpander,
};
use andromeda_ngsi_ld_geometry::geometry_census::GeometryCensusLimits;
use andromeda_ngsi_ld_vocabulary::compact::properties;

/// Compile geo query parameters into a [`GeoQuery`].
///
/// All three of `georel`, `geometry`, `coordinates` must be present together or
/// all absent (clause 4.10). `geoproperty` defaults to `location`.
///
/// The reference geometry is admitted within `limits` (clause 5.5.6): read
/// coordinates by their positions, encoded ones counted from their text before
/// it is parsed.
///
/// # Errors
///
/// Returns the refusal of [`ReferenceGeometry::admit`](crate::geo::reference_geometry::ReferenceGeometry::admit)
/// for a reference geometry past `limits` or encoded coordinates that are no
/// geometry, and [`QueryError::TermExpansion`], [`QueryError::TermNotAnIri`]
/// or [`QueryError::AttributeNameInvalid`] when the `geoproperty` term does
/// not expand to a usable attribute name.
pub async fn compile_geo_query(
    input: Option<&GeoQueryInput>,
    expander: &impl TermExpander,
    limits: GeometryCensusLimits,
) -> Result<Option<GeoQuery>, QueryError> {
    let Some(input) = input else {
        return Ok(None);
    };
    let geometry = input.reference.admit(limits)?;
    let geoproperty = input.geoproperty.as_ref().map_or(properties::LOCATION, AsRef::<str>::as_ref);
    Ok(Some(GeoQuery {
        georel: input.georel.clone(),
        geometry,
        geoproperty: expanded_attribute_name(&expander.expand_term(geoproperty).await?)?,
    }))
}

#[cfg(test)]
mod tests {
    use crate::{
        error::QueryError,
        geo::{compile::compile_geo_query, input::GeoQueryInput},
        term_expander::TermExpander,
    };
    use andromeda_ngsi_ld_geometry::geometry_census::{GeometryCensusLimits, GeometryPositionLimit};
    use andromeda_ngsi_ld_number::json_value::value_census::JsonValueLimit;
    use iri_rs::IriBuf;
    use pretty_assertions::assert_eq;
    use std::{
        future::{Future, ready},
        num::NonZeroUsize,
    };

    struct PrefixExpander;

    /// Limits no fixture of this module reaches.
    fn unbounded() -> GeometryCensusLimits {
        GeometryCensusLimits {
            positions: GeometryPositionLimit::new(NonZeroUsize::MAX),
            values: JsonValueLimit::new(NonZeroUsize::MAX),
        }
    }

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
        let compiled = compile_geo_query(Some(&input), &PrefixExpander, unbounded())
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
