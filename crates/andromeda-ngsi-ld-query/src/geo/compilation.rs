//! Compilation of the geo query parameter group into a [`CompiledGeoQuery`].

use crate::{
    filter::expansion::expanded_attribute_name,
    geo::{admission::GeoQueryAdmission, compiled_geo_query::CompiledGeoQuery, written_geo_query::WrittenGeoQuery},
    query_error::QueryError,
    term_expander::TermExpander,
};
use andromeda_ngsi_ld_vocabulary::compact::properties;

/// Compile geo query parameters into a [`CompiledGeoQuery`].
///
/// All three of `georel`, `geometry`, `coordinates` must be present together or
/// all absent (clause 4.10). `geoproperty` defaults to `location`.
///
/// The reference geometry is admitted within the limits of `admission`
/// (clause 5.5.6): read coordinates by their positions, encoded ones counted
/// from their text before it is parsed. A `near` distance band is admitted
/// only when `admission` admits it.
///
/// # Errors
///
/// Returns [`QueryError::NearDistanceBand`] for a distance band `admission`
/// refuses, the refusal of [`ReferenceGeometry::admit`](crate::geo::reference_geometry::ReferenceGeometry::admit)
/// for a reference geometry past the limits or encoded coordinates that are no
/// geometry, and [`QueryError::TermExpansion`], [`QueryError::TermNotAnIri`]
/// or [`QueryError::AttributeNameInvalid`] when the `geoproperty` term does
/// not expand to a usable attribute name.
pub async fn compile_geo_query(
    input: Option<&WrittenGeoQuery>,
    expander: &impl TermExpander,
    admission: GeoQueryAdmission,
) -> Result<Option<CompiledGeoQuery>, QueryError> {
    let Some(input) = input else {
        return Ok(None);
    };
    admission.near_band.admit(&input.georel)?;
    let geometry = input.reference.admit(admission.limits)?;
    let geoproperty = input.geoproperty.as_ref().map_or(properties::LOCATION, AsRef::<str>::as_ref);
    Ok(Some(CompiledGeoQuery {
        georel: input.georel.clone(),
        geometry,
        geoproperty: expanded_attribute_name(&expander.expand_term(geoproperty).await?)?,
    }))
}

#[cfg(test)]
mod tests {
    use crate::{
        geo::{admission::GeoQueryAdmission, compilation::compile_geo_query, near_distance_band::NearDistanceBand, written_geo_query::WrittenGeoQuery},
        namespace_expander::EXAMPLE_NAMESPACE_TYPED,
        query_error::QueryError,
    };
    use andromeda_ngsi_ld_geometry::geometry_census::{GeometryCensusLimits, GeometryPositionLimit};
    use andromeda_ngsi_ld_number::json_value::value_census::JsonValueLimit;
    use pretty_assertions::assert_eq;
    use std::num::NonZeroUsize;

    /// Limits no fixture of this module reaches, under `near_band`.
    fn unbounded(near_band: NearDistanceBand) -> GeoQueryAdmission {
        GeoQueryAdmission {
            limits: GeometryCensusLimits {
                positions: GeometryPositionLimit::new(NonZeroUsize::MAX),
                values: JsonValueLimit::new(NonZeroUsize::MAX),
            },
            near_band,
        }
    }

    #[tokio::test]
    async fn expansion_consumes_a_decoded_geo_query_without_reparsing_components() {
        let input = WrittenGeoQuery::from_wire(
            "within",
            "Point",
            serde_json::from_value(serde_json::json!([1.0, 2.0])).expect("coordinates"),
            None,
        )
        .expect("valid decoded geo query");
        let compiled = compile_geo_query(Some(&input), &EXAMPLE_NAMESPACE_TYPED, unbounded(NearDistanceBand::Refused))
            .await
            .expect("expansion succeeds")
            .expect("geo query present");
        assert_eq!(compiled.geoproperty.as_str(), "https://example.com/location");
    }

    #[tokio::test]
    async fn a_distance_band_compiles_only_under_the_extension() {
        let input = WrittenGeoQuery::from_wire(
            "near;minDistance==500;maxDistance==10000",
            "Point",
            serde_json::from_value(serde_json::json!([1.0, 2.0])).expect("coordinates"),
            None,
        )
        .expect("valid decoded geo query");
        assert!(matches!(
            compile_geo_query(Some(&input), &EXAMPLE_NAMESPACE_TYPED, unbounded(NearDistanceBand::Refused)).await,
            Err(QueryError::NearDistanceBand)
        ));
        assert!(
            compile_geo_query(Some(&input), &EXAMPLE_NAMESPACE_TYPED, unbounded(NearDistanceBand::Admitted))
                .await
                .expect("the band compiles")
                .is_some()
        );
    }

    #[test]
    fn malformed_geo_wire_values_fail_before_compilation() {
        assert!(WrittenGeoQuery::from_wire("within", "Point", serde_json::from_value(serde_json::json!([1.0])).expect("coordinates"), None).is_err());
    }
}
