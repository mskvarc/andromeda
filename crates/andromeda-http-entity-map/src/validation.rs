//! Entity Map request validation owned by its HTTP binding.

use crate::error::{EntityMapResultExt, bad_request_data};
use andromeda_http_response::problem_response::ProblemResponse;
use andromeda_ngsi_ld_query::{filter::expression::Expr, projection::grammar::RawProjectionTerm};
use andromeda_ngsi_ld_request::query::{entity::parameters::EntityQueryParameters, temporal::TemporalQueryParameters};
use axum::http::Uri;

/// Validate the parameters of an entity map creation (clause 5.14.4.4): the
/// query's own constraints, which are the Query Entities constraints clause
/// 5.14.4.4 restates -- the "too wide query" rule included -- and
/// `geometryProperty`, which the clause admits only with a `GeoJSON` `Accept`;
/// the map resources render no `GeoJSON` (clause 6.34.3.1), so its presence is
/// always an error of type `BadRequestData`.
pub(crate) fn validate_entity_map_query(query: &EntityQueryParameters, uri: &Uri) -> Result<(), ProblemResponse> {
    query.validate().with_entity_map_instance(uri)?;
    if query.geometry_property.is_some() {
        return Err(bad_request_data(
            "geometryProperty is only applicable when the Accept header is application/geo+json, which an EntityMap is never rendered as",
            uri,
        ));
    }
    Ok(())
}

/// Validate the parameters of a temporal entity map creation (clause
/// 5.14.5.4): the temporal query's own constraints, which the clause restates
/// from Query Temporal Evolution -- the clause 5.7.4.4 "too wide query"
/// narrowing rule included -- and then, on top of them, that Linked Entity
/// retrieval is not available on the operation, so `join`/`joinLevel`, a
/// nested projection term and a `q` hop into a Linked Entity are each an error
/// of type `BadRequestData`.
pub(crate) fn validate_temporal_entity_map_query(params: &TemporalQueryParameters, uri: &Uri) -> Result<(), ProblemResponse> {
    // Clauses 5.14.5.4 / 5.7.4.4: the creation is a temporal query and inherits
    // its parameter validation, including the clause 5.7.2.4 narrowing rule that
    // the map's own join/linked-hop checks below do not cover. `require_temporal`
    // is true, matching the temporal query endpoint the map is created from.
    params.validate(true).with_entity_map_instance(uri)?;
    if params.join.is_some() || params.join_level.is_some() {
        return Err(bad_request_data(
            "join and joinLevel are not applicable to temporal EntityMap creation: Linked Entity retrieval is not available on this operation",
            uri,
        ));
    }
    if params.q.as_ref().is_some_and(Expr::contains_linked_hop) {
        return Err(bad_request_data(
            "the query filter reaches into a Linked Entity, which temporal EntityMap creation does not retrieve",
            uri,
        ));
    }
    let nests = params
        .pick
        .iter()
        .chain(params.omit.iter())
        .flatten()
        .any(|term| RawProjectionTerm::linked_depth(term) > 0);
    if nests {
        return Err(bad_request_data(
            "the projection names Linked Entity members, which temporal EntityMap creation does not retrieve",
            uri,
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::validation::{validate_entity_map_query, validate_temporal_entity_map_query};
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
        assert!(validate_entity_map_query(&entity_query("id=urn:ngsi-ld:Vehicle:1"), &uri()).is_err());
        assert!(validate_entity_map_query(&entity_query("local=true"), &uri()).is_ok());
        assert!(validate_entity_map_query(&entity_query("type=Vehicle"), &uri()).is_ok());
    }

    /// Clause 5.14.5.4 carries the clause 5.7.4.4 narrowing rule (itself the
    /// clause 5.7.2.4 rule verbatim, local-scope exemption included), so a
    /// temporal creation whose only input is the temporal window is too wide,
    /// while one that also names a type or the local scope is not.
    #[test]
    fn a_temporal_creation_without_any_narrowing_input_is_a_bad_request() {
        const WINDOW: &str = "timerel=before&timeAt=2022-01-01T00:00:00Z";
        assert!(validate_temporal_entity_map_query(&temporal_query(WINDOW), &uri()).is_err());
        assert!(validate_temporal_entity_map_query(&temporal_query(&format!("type=Vehicle&{WINDOW}")), &uri()).is_ok());
        assert!(validate_temporal_entity_map_query(&temporal_query(&format!("local=true&{WINDOW}")), &uri()).is_ok());
    }

    /// Clause 5.14.4.4: `geometryProperty` needs a `GeoJSON` `Accept`, which
    /// the map resource never renders.
    #[test]
    fn a_geometry_property_is_refused_on_entity_map_creation() {
        let refused = validate_entity_map_query(&entity_query("type=Vehicle&geometryProperty=location"), &uri()).expect_err("refused");
        pretty_assertions::assert_eq!(refused.problem().status_code(), StatusCode::BAD_REQUEST);
        assert!(validate_entity_map_query(&entity_query("type=Vehicle"), &uri()).is_ok());
    }

    /// Clause 5.14.4.4: a nested projection and a filter hop both need a
    /// retrieving join the request did not ask for.
    #[test]
    fn linked_entity_references_without_a_join_are_refused_on_entity_map_creation() {
        assert!(validate_entity_map_query(&entity_query("type=Vehicle&pick=linkedTo%7Bspeed%7D"), &uri()).is_err());
        assert!(validate_entity_map_query(&entity_query("type=Vehicle&q=linkedTo%7Bspeed%3E5%7D"), &uri()).is_err());
        assert!(validate_entity_map_query(&entity_query("type=Vehicle&join=inline&pick=linkedTo%7Bspeed%7D"), &uri()).is_ok());
    }

    /// Clause 5.14.5.4: no Linked Entity retrieval on the temporal creation,
    /// whether asked for through the join parameters, a projection or a filter.
    #[test]
    fn linked_entity_retrieval_is_refused_on_temporal_entity_map_creation() {
        const WINDOW: &str = "timerel=before&timeAt=2022-01-01T00:00:00Z";
        let refused =
            validate_temporal_entity_map_query(&temporal_query(&format!("type=Vehicle&join=inline&joinLevel=2&{WINDOW}")), &uri()).expect_err("refused");
        pretty_assertions::assert_eq!(refused.problem().status_code(), StatusCode::BAD_REQUEST);
        assert!(validate_temporal_entity_map_query(&temporal_query(&format!("type=Vehicle&join=flat&{WINDOW}")), &uri()).is_err());
        assert!(validate_temporal_entity_map_query(&temporal_query(&format!("type=Vehicle&joinLevel=1&{WINDOW}")), &uri()).is_err());
        assert!(validate_temporal_entity_map_query(&temporal_query(&format!("type=Vehicle&q=linkedTo%7Bspeed%3E5%7D&{WINDOW}")), &uri()).is_err());
        assert!(validate_temporal_entity_map_query(&temporal_query(&format!("type=Vehicle&pick=linkedTo%7Bspeed%7D&{WINDOW}")), &uri()).is_err());
        assert!(validate_temporal_entity_map_query(&temporal_query(&format!("type=Vehicle&q=speed%3E5&pick=speed&{WINDOW}")), &uri()).is_ok());
    }
}
