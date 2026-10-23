//! Entity Map request validation owned by its HTTP binding.

use crate::error::EntityMapResultExt;
use andromeda_http_headers::media_type::ResponseMediaType;
use andromeda_http_response::problem_response::ProblemResponse;
use andromeda_linked_retrieval::admission::TemporalJoin;
use andromeda_ngsi_ld_request::{
    geometry_media_type::require_geojson_for_geometry_property,
    query::{
        entity::{operation::EntityQueryOperation, parameters::EntityQueryParameters},
        temporal::{TemporalQueryParameters, operation::TemporalQueryOperation},
    },
};
use axum::http::Uri;

/// Validate the parameters of an entity map creation (clause 5.14.4.4): the
/// query's own constraints, which are the Query Entities constraints clause
/// 5.14.4.4 restates -- the "too wide query" rule included -- and
/// `geometryProperty`, which the clause admits only with a `GeoJSON` `Accept`;
/// the map resources render no `GeoJSON` (clause 6.34.3.1), so its presence is
/// always an error of type `BadRequestData`.
pub(crate) fn validate_entity_map_query(query: &EntityQueryParameters, media_type: ResponseMediaType, uri: &Uri) -> Result<(), ProblemResponse> {
    query.validate(EntityQueryOperation::EntityMap).with_entity_map_instance(uri)?;
    require_geojson_for_geometry_property(query.geometry_property.as_ref(), media_type).with_entity_map_instance(uri)?;
    Ok(())
}

/// Validate the parameters of a temporal entity map creation (clause
/// 5.14.5.4): the temporal query's own constraints, which the clause restates
/// from Query Temporal Evolution -- the temporal window, the clause 5.7.4.4
/// "too wide query" narrowing rule and the refusal of Linked Entity retrieval,
/// which the `temporal_linked_retrieval` extension lifts as `linked_retrieval`
/// says.
pub(crate) fn validate_temporal_entity_map_query(params: &TemporalQueryParameters, linked_retrieval: TemporalJoin, uri: &Uri) -> Result<(), ProblemResponse> {
    params
        .validate(TemporalQueryOperation::EntityMap, linked_retrieval)
        .with_entity_map_instance(uri)
}

#[cfg(test)]
mod tests {
    use crate::validation::{validate_entity_map_query, validate_temporal_entity_map_query};
    use andromeda_http_headers::media_type::ResponseMediaType;
    use andromeda_linked_retrieval::admission::TemporalJoin;
    use andromeda_ngsi_ld_request::query::{entity::parameters::EntityQueryParameters, temporal::TemporalQueryParameters};
    use axum::http::{StatusCode, Uri};

    fn uri() -> Uri {
        "/ngsi-ld/v1/entityMaps".parse().expect("valid URI")
    }

    fn entity_query(raw: &str) -> EntityQueryParameters {
        serde_urlencoded::from_str(raw).expect("valid entity query")
    }

    fn temporal_query(raw: &str) -> TemporalQueryParameters {
        serde_urlencoded::from_str(raw).expect("valid temporal query")
    }

    /// Clause 5.14.4.4 restates the clause 5.7.2.4 rule word for word, local
    /// scope included, so a creation naming nothing but identifiers is too wide
    /// and one limited to the local scope is not.
    #[test]
    fn a_creation_without_any_narrowing_input_is_a_bad_request() {
        assert!(validate_entity_map_query(&entity_query("id=urn:ngsi-ld:Vehicle:1"), ResponseMediaType::Json, &uri()).is_err());
        assert!(validate_entity_map_query(&entity_query("local=true"), ResponseMediaType::Json, &uri()).is_ok());
        assert!(validate_entity_map_query(&entity_query("type=Vehicle"), ResponseMediaType::Json, &uri()).is_ok());
    }

    /// Clause 5.14.5.4 carries the clause 5.7.4.4 narrowing rule (itself the
    /// clause 5.7.2.4 rule verbatim, local-scope exemption included), so a
    /// temporal creation whose only input is the temporal window is too wide,
    /// while one that also names a type or the local scope is not.
    #[test]
    fn a_temporal_creation_without_any_narrowing_input_is_a_bad_request() {
        const WINDOW: &str = "timerel=before&timeAt=2022-01-01T00:00:00Z";
        assert!(validate_temporal_entity_map_query(&temporal_query(WINDOW), TemporalJoin::Spec, &uri()).is_err());
        assert!(validate_temporal_entity_map_query(&temporal_query(&format!("type=Vehicle&{WINDOW}")), TemporalJoin::Spec, &uri()).is_ok());
        assert!(validate_temporal_entity_map_query(&temporal_query(&format!("local=true&{WINDOW}")), TemporalJoin::Spec, &uri()).is_ok());
    }

    /// Clause 5.14.4.4: `geometryProperty` needs a `GeoJSON` `Accept`, which
    /// the map resource never renders.
    #[test]
    fn a_geometry_property_is_refused_on_entity_map_creation() {
        let refused = validate_entity_map_query(&entity_query("type=Vehicle&geometryProperty=location"), ResponseMediaType::Json, &uri()).expect_err("refused");
        pretty_assertions::assert_eq!(refused.problem().status_code(), StatusCode::BAD_REQUEST);
        assert!(validate_entity_map_query(&entity_query("type=Vehicle"), ResponseMediaType::Json, &uri()).is_ok());
    }

    /// Clause 5.14.4.4: a nested projection and a filter hop both need a
    /// retrieving join the request did not ask for.
    #[test]
    fn linked_entity_references_without_a_join_are_refused_on_entity_map_creation() {
        assert!(validate_entity_map_query(&entity_query("type=Vehicle&pick=linkedTo%7Bspeed%7D"), ResponseMediaType::Json, &uri()).is_err());
        assert!(validate_entity_map_query(&entity_query("type=Vehicle&q=linkedTo%7Bspeed%7D%3E5"), ResponseMediaType::Json, &uri()).is_err());
        assert!(
            validate_entity_map_query(
                &entity_query("type=Vehicle&join=inline&pick=linkedTo%7Bspeed%7D"),
                ResponseMediaType::Json,
                &uri()
            )
            .is_ok()
        );
    }

    /// Clause 5.14.5.4: no Linked Entity retrieval on the temporal creation,
    /// whether asked for through `join`, a projection or a filter; `@none`
    /// asks for none. A lone `joinLevel` bounds no join (Table 6.5.3.1-1), so
    /// clause 6.3.20 refuses it.
    #[test]
    fn linked_entity_retrieval_is_refused_on_temporal_entity_map_creation() {
        const WINDOW: &str = "timerel=before&timeAt=2022-01-01T00:00:00Z";
        let refused = validate_temporal_entity_map_query(
            &temporal_query(&format!("type=Vehicle&join=inline&joinLevel=2&{WINDOW}")),
            TemporalJoin::Spec,
            &uri(),
        )
        .expect_err("refused");
        pretty_assertions::assert_eq!(refused.problem().status_code(), StatusCode::BAD_REQUEST);
        for refused in ["join=flat", "q=linkedTo%7Bspeed%7D%3E5", "pick=linkedTo%7Bspeed%7D", "joinLevel=1"] {
            let query = temporal_query(&format!("type=Vehicle&{refused}&{WINDOW}"));
            assert!(validate_temporal_entity_map_query(&query, TemporalJoin::Spec, &uri()).is_err(), "{refused}");
        }
        for admitted in ["join=%40none", "q=speed%3E5&pick=speed"] {
            let query = temporal_query(&format!("type=Vehicle&{admitted}&{WINDOW}"));
            assert!(validate_temporal_entity_map_query(&query, TemporalJoin::Spec, &uri()).is_ok(), "{admitted}");
        }
    }

    /// With the `temporal_linked_retrieval` extension the creation follows
    /// Query Entities: a nesting within the join is admitted.
    #[test]
    fn the_extension_admits_linked_entity_retrieval_on_temporal_entity_map_creation() {
        const WINDOW: &str = "timerel=before&timeAt=2022-01-01T00:00:00Z";
        let joined = temporal_query(&format!("type=Vehicle&join=flat&pick=linkedTo%7Bspeed%7D&{WINDOW}"));
        assert!(validate_temporal_entity_map_query(&joined, TemporalJoin::Extended, &uri()).is_ok());
        let unjoined = temporal_query(&format!("type=Vehicle&pick=linkedTo%7Bspeed%7D&{WINDOW}"));
        assert!(validate_temporal_entity_map_query(&unjoined, TemporalJoin::Extended, &uri()).is_err());
    }
}
