//! The `EntityMap` a purge runs under (Table 6.4.3.3-2).
//!
//! Table 6.4.3.3-2 makes `NGSILD-EntityMap` a request header every
//! implementation "shall support" on `DELETE /entities`: "If present, the
//! EntityMap supplied is used for determining the set of Entities requested
//! during the purge operation. The location of the EntityMap used in the purge
//! operation is returned in the response."
//!
//! A purge only ever reuses the map it is handed, and never creates one. Three
//! readings agree on that. Clause 5.5.14 enumerates the operations that may
//! occur as part of a distributed transactional request -- Retrieve Entity,
//! Query Entities, Retrieve Temporal Evolution of an Entity, Query Temporal
//! Evolution of Entities -- and Purge Entities is not among them, so a purge is
//! never the request a map is created for. Table 6.4.3.3-3 admits only 204, 207
//! and 400, never the "201 Created (in case an `EntityMap` has been
//! (re)created)" that Table 6.4.3.2-3 gives Query Entities, so a purge has no
//! way to report a creation. And Table 6.4.3.3-2 itself says "the location of
//! the `EntityMap` **used** in the purge operation", which is not a map the
//! purge would have created for a later request. Table 6.4.3.3-1 listing
//! neither `entityMap` nor `entityMapLifetime` corroborates it without
//! deciding it, since that table is not exhaustive either -- it omits `attrs`
//! and `local`, both of which clause 5.6.21.4 requires.
//!
//! A reference that expired or cannot be accessed therefore "yields no
//! inference" (clause 5.5.14) and leaves the purge scoped by its own query
//! alone, answering with no map location because it used no map.

use crate::error::EntityResultExt;
use andromeda_context_space::context_target::ContextTarget;
use andromeda_entity_map::{
    fingerprint::{FingerprintedRequest, fingerprint_of},
    query_scoping::hold_selection_to_map,
    service::{EntityMapService, ReferencedEntityMap},
};
use andromeda_http_headers::ngsild_entity_map::NgsiLdEntityMap;
use andromeda_http_response::{headers::with_entity_map, problem_response::ProblemResponse};
use andromeda_ngsi_ld_lexicon::uri::Uri;
use andromeda_ngsi_ld_request::query::entity::parameters::EntityQueryParameters;
use axum::{http::Uri as HttpUri, response::Response};

/// Which `EntityMap` a purge determined its entities from (Table 6.4.3.3-2).
pub(crate) enum PurgeEntityMap {
    /// The live map the request supplied: it fixed the entities purged, and its
    /// location is returned in the response.
    Used(Uri),
    /// No map determined the entity set: the request supplied none, or supplied
    /// one that has expired or cannot be accessed, from which clause 5.5.14
    /// allows no inference. There is no location to return.
    None,
}

impl PurgeEntityMap {
    /// Answer with the location of the map the purge used, when it used one.
    pub(crate) fn attach_to(&self, response: Response) -> Response {
        match self {
            PurgeEntityMap::Used(identifier) => with_entity_map(response, identifier),
            PurgeEntityMap::None => response,
        }
    }
}

/// Resolve the map the purge determines its entities from, and hold the query
/// to it.
///
/// The fingerprint is taken before the query is narrowed, so a purge under a
/// map is compared against the request as written. Clause 5.6.21.4 has the
/// purge "run a Query Entities operation (clause 5.6.2) to retrieve a list of
/// ids of Entities for deletion" -- the cross-reference is the spec's own, and
/// misnumbered: Query Entities is clause 5.7.2. The request the map fixes is
/// therefore that Query Entities, and it is fingerprinted as one: a map minted
/// by `GET /entities?type=Vehicle&entityMap=true` is the map a purge over the
/// same filters reuses, and a purge over different filters is the changed
/// request clause 5.5.14 forbids.
///
/// A snapshot branch contacts no context source and so runs under no map
/// (clause 5.5.15).
///
/// # Errors
///
/// Returns a [`ProblemResponse`] when the request cannot be fingerprinted, when
/// the map store cannot be read, or when the purge changed a filter parameter
/// of the request that created the map it references.
pub(crate) async fn resolve_purge_map(
    maps: &EntityMapService,
    target: &ContextTarget,
    query: &mut EntityQueryParameters,
    referenced: Option<&NgsiLdEntityMap>,
    instance: &HttpUri,
) -> Result<PurgeEntityMap, ProblemResponse> {
    if !target.permits_federation() {
        return Ok(PurgeEntityMap::None);
    }
    let Some(referenced) = referenced else {
        return Ok(PurgeEntityMap::None);
    };
    let fingerprint = fingerprint_of(FingerprintedRequest::QueryEntities(query)).with_entity_instance(instance)?;
    let resolved = maps.resolve_reuse(referenced.as_inner(), &fingerprint).await.with_entity_instance(instance)?;
    match resolved {
        ReferencedEntityMap::Live(map) => {
            hold_selection_to_map(&map, &mut query.id, &mut query.entity_selectors);
            Ok(PurgeEntityMap::Used(map.id))
        }
        ReferencedEntityMap::Unavailable => Ok(PurgeEntityMap::None),
    }
}

#[cfg(test)]
mod tests {
    use crate::entity::purge_entity_map::PurgeEntityMap;
    use andromeda_ngsi_ld_lexicon::uri::Uri;
    use axum::{
        http::StatusCode,
        response::{IntoResponse, Response},
    };

    /// The 204 a purge answers with when nothing failed.
    fn no_content() -> Response {
        StatusCode::NO_CONTENT.into_response()
    }

    /// The location the response advertises, if it advertises one.
    fn advertised_map(response: &Response) -> Option<&str> {
        response.headers().get("ngsild-entitymap").and_then(|value| value.to_str().ok())
    }

    /// Table 6.4.3.3-2: "The location of the `EntityMap` used in the purge
    /// operation is returned in the response."
    #[test]
    fn a_purge_that_used_a_map_answers_with_its_location() {
        let identifier: Uri = "urn:ngsi-ld:EntityMap:used".parse().expect("valid URI");
        let response = PurgeEntityMap::Used(identifier).attach_to(no_content());
        pretty_assertions::assert_eq!(advertised_map(&response), Some("urn:ngsi-ld:EntityMap:used"));
    }

    /// A purge that supplied no map, or one clause 5.5.14 allows no inference
    /// from, used none: there is no location to return, and inventing one would
    /// tell the client its entity set was fixed when it was not.
    #[test]
    fn a_purge_that_used_no_map_answers_with_no_location() {
        let response = PurgeEntityMap::None.attach_to(no_content());
        pretty_assertions::assert_eq!(advertised_map(&response), None);
        pretty_assertions::assert_eq!(response.status(), StatusCode::NO_CONTENT);
    }
}
