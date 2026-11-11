//! The canonical digest of the filter parameters of a map-creating request
//! (clause 5.5.14).
//!
//! Clause 5.5.14 holds every follow-up under a map to "the same parameters as
//! in the original request that created the EntityMap, except for the
//! specification of Entity identifiers or parameters related to pagination,
//! or, in the case of temporal requests, the temporal query". The digest
//! therefore covers exactly the parameters that select entities and sources
//! by anything but their identifiers: the type selector, `attrs`, `q`, the
//! geo-query, `scopeQ` and `csf`. The identifiers and `idPattern` (both a
//! specification of Entity identifiers, so a source holding a map may be asked
//! for one mapped id where its map was created for a pattern),
//! `limit`/`offset`/`count`, the temporal window, `lastN` and every
//! representation parameter are left out, and list-valued parameters are
//! sorted so the order the client wrote them in is irrelevant.
//!
//! Clauses 5.7.2.3 and 5.7.4.3 give a query one selector of entity types. The
//! GET binding carries it as the `type` parameter and the POST binding as the
//! `type` of each `entities[]` selector (clause 5.2.33), so the digest records
//! it as the set of its clause 4.17 alternatives, which is the same whichever
//! binding carried it.

use crate::{entity_map_invariant::EntityMapInvariant, error::EntityMapError};
use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
use andromeda_ngsi_ld_model::entity_map_fingerprint::{MapRequestKind, RequestFingerprint};
use andromeda_ngsi_ld_query::{filter::expression::Expr, geo::input::GeoQueryInput, scope::query::ScopeQuery, type_selection::grammar::TypeExprRaw};
use andromeda_ngsi_ld_request::{
    entity::GetEntityParams,
    query::{entity::parameters::EntityQueryParameters, entity_selector::EntitySelectorParams, temporal::TemporalQueryParameters},
};
use sha2::{Digest, Sha256};

/// Separates a field's name from its value in the digested record.
const FIELD_SEPARATOR: u8 = 0x1f;
/// Terminates one field of the digested record.
const RECORD_SEPARATOR: u8 = 0x1e;

/// The request a map is created for, by the operation it is.
#[derive(Debug, Clone, Copy)]
pub enum FingerprintedRequest<'a> {
    /// Query Entities, through either binding, or Create `EntityMap` for Query Entities.
    QueryEntities(&'a EntityQueryParameters),
    /// Retrieve Entity.
    RetrieveEntity(&'a GetEntityParams),
    /// Query Temporal Evolution of Entities, through either binding, or its map creation.
    QueryTemporal(&'a TemporalQueryParameters),
    /// Retrieve Temporal Evolution of an Entity.
    RetrieveTemporal(&'a TemporalQueryParameters),
}

/// One `(name, value)` field of the canonical record; an absent parameter is
/// recorded as absent rather than omitted, so the record stays injective.
struct CanonicalField<'a> {
    /// The parameter's name.
    name: &'static str,
    /// Its canonical value, when the request carried it.
    value: Option<&'a str>,
}

/// The digest of a request's filter parameters, tagged with its operation.
///
/// # Errors
///
/// A geo-query that cannot be written back canonically, which is an
/// invariant violation of a value the request boundary already validated.
pub fn fingerprint_of(request: FingerprintedRequest<'_>) -> Result<RequestFingerprint, EntityMapError> {
    let (kind, record) = match request {
        FingerprintedRequest::QueryEntities(params) => (MapRequestKind::QueryEntities, filter_record(&QueryFilters::from(params))?),
        FingerprintedRequest::QueryTemporal(params) => (MapRequestKind::QueryTemporal, filter_record(&QueryFilters::from(params))?),
        FingerprintedRequest::RetrieveEntity(params) => (
            MapRequestKind::RetrieveEntity,
            retrieval_record(params.entity_type.as_ref(), params.attrs.as_deref()),
        ),
        FingerprintedRequest::RetrieveTemporal(params) => (MapRequestKind::RetrieveTemporal, retrieval_record(None, params.attrs.as_deref())),
    };
    Ok(RequestFingerprint::new(kind, digest_record(&record)))
}

/// The parameters of a query that select entities and sources (clauses
/// 5.7.2.3 and 5.7.4.3), borrowed from either query's parameter set.
struct QueryFilters<'a> {
    /// The clause 4.17 type selector of the GET binding, which a POST body of
    /// one selector also fills.
    entity_type: Option<&'a TypeExprRaw>,
    /// The attribute list.
    attrs: Option<&'a [AttributeName]>,
    /// The clause 4.9 filter.
    q: Option<&'a Expr>,
    /// The clause 4.10 geo-query.
    geo: Option<&'a GeoQueryInput>,
    /// The clause 4.19 scope query.
    scope_q: Option<&'a ScopeQuery>,
    /// The context source filter.
    csf: Option<&'a Expr>,
    /// The selectors of a POST body's `entities[]` (clause 5.2.33), which
    /// carry the type selection in place of `entity_type` when present.
    selectors: &'a [EntitySelectorParams],
}

impl<'a> From<&'a EntityQueryParameters> for QueryFilters<'a> {
    fn from(params: &'a EntityQueryParameters) -> QueryFilters<'a> {
        QueryFilters {
            entity_type: params.entity_type.as_ref(),
            attrs: params.attrs.as_deref(),
            q: params.q.as_ref(),
            geo: params.geo.0.as_ref(),
            scope_q: params.scope_q.as_ref(),
            csf: params.csf.as_ref(),
            selectors: &params.entity_selectors,
        }
    }
}

impl<'a> From<&'a TemporalQueryParameters> for QueryFilters<'a> {
    fn from(params: &'a TemporalQueryParameters) -> QueryFilters<'a> {
        QueryFilters {
            entity_type: params.entity_type.as_ref(),
            attrs: params.attrs.as_deref(),
            q: params.q.as_ref(),
            geo: params.geo.0.as_ref(),
            scope_q: params.scope_q.as_ref(),
            csf: params.csf.as_ref(),
            selectors: &params.entity_selectors,
        }
    }
}

/// The owned canonical values of a query's filter parameters, in a fixed order.
struct FilterRecord {
    /// Each parameter's canonical value, absent when the request omitted it.
    values: [(&'static str, Option<String>); 6],
}

/// The canonical record of a query's filters.
fn filter_record(filters: &QueryFilters<'_>) -> Result<FilterRecord, EntityMapError> {
    let geo = filters
        .geo
        .map(|geo| serde_json::to_string(geo).map_err(|error| EntityMapError::from(EntityMapInvariant::GeoQueryUnserializable(error))))
        .transpose()?;
    Ok(FilterRecord {
        values: [
            ("type", type_alternatives(filters.entity_type, filters.selectors)),
            ("attrs", filters.attrs.map(sorted_names)),
            ("q", filters.q.map(ToString::to_string)),
            ("geoQ", geo),
            ("scopeQ", filters.scope_q.map(ToString::to_string)),
            ("csf", filters.csf.map(ToString::to_string)),
        ],
    })
}

/// The canonical record of a retrieval, whose filters are the clause 5.7.1.3
/// type selector and `attrs`: the type selector decides which registrations
/// match the target entity, so a map created without it does not describe a
/// retrieval that names one.
fn retrieval_record(entity_type: Option<&TypeExprRaw>, attrs: Option<&[AttributeName]>) -> FilterRecord {
    FilterRecord {
        values: [
            ("type", type_alternatives(entity_type, &[])),
            ("attrs", attrs.map(sorted_names)),
            ("q", None),
            ("geoQ", None),
            ("scopeQ", None),
            ("csf", None),
        ],
    }
}

/// Attribute names in a client-order-independent form.
fn sorted_names(names: &[AttributeName]) -> String {
    let mut names: Vec<&str> = names.iter().map(AttributeName::as_str).collect();
    names.sort_unstable();
    names.join(",")
}

/// The clause 4.17 alternatives a request's type selection is the union of,
/// sorted and deduplicated, whichever binding carried it.
///
/// The selectors of a POST body (clause 5.2.33) are alternatives of one
/// another, as the operands of a top-level `|` are. Their `id` and `idPattern`
/// are left out with the identifiers clause 5.5.14 excepts, so `type=A|B` and
/// a body of the selectors `A` and `B` select alike, whatever ids or patterns
/// those carry. The alternatives are joined with `,`, which no parsed type name
/// holds, so the result is the clause 4.17 text of the same selection.
fn type_alternatives(entity_type: Option<&TypeExprRaw>, selectors: &[EntitySelectorParams]) -> Option<String> {
    let mut alternatives: Vec<String> = EntitySelectorParams::effective(selectors, None, None, entity_type)
        .iter()
        .filter_map(|selector| selector.type_.as_ref())
        .flat_map(TypeExprRaw::alternatives)
        .map(ToString::to_string)
        .collect();
    if alternatives.is_empty() {
        return None;
    }
    alternatives.sort_unstable();
    alternatives.dedup();
    Some(alternatives.join(","))
}

/// SHA-256 over the framed fields of a record.
fn digest_record(record: &FilterRecord) -> [u8; 32] {
    let mut hasher = Sha256::new();
    for (name, value) in &record.values {
        let field = CanonicalField { name, value: value.as_deref() };
        hasher.update(field.name.as_bytes());
        hasher.update([FIELD_SEPARATOR]);
        match field.value {
            Some(value) => {
                hasher.update([1]);
                hasher.update(value.as_bytes());
            }
            None => hasher.update([0]),
        }
        hasher.update([RECORD_SEPARATOR]);
    }
    hasher.finalize().into()
}

#[cfg(test)]
mod tests {
    use crate::{
        error::EntityMapError,
        fingerprint::{FingerprintedRequest, fingerprint_of},
    };
    use andromeda_ngsi_ld_model::{
        entity_map_fingerprint::{MapRequestKind, RequestComparison, RequestFingerprint},
        query::body::Query,
    };
    use andromeda_ngsi_ld_request::{
        entity::GetEntityParams,
        query::{entity::parameters::EntityQueryParameters, temporal::TemporalQueryParameters},
    };

    fn entity_query(query: &str) -> Result<RequestFingerprint, EntityMapError> {
        let params = serde_urlencoded::from_str::<EntityQueryParameters>(query).expect("a valid test fixture");
        fingerprint_of(FingerprintedRequest::QueryEntities(&params))
    }

    fn temporal_query(query: &str) -> Result<RequestFingerprint, EntityMapError> {
        let params = serde_urlencoded::from_str::<TemporalQueryParameters>(query).expect("a valid test fixture");
        fingerprint_of(FingerprintedRequest::QueryTemporal(&params))
    }

    fn retrieve(query: &str) -> Result<RequestFingerprint, EntityMapError> {
        let params = serde_urlencoded::from_str::<GetEntityParams>(query).expect("a valid test fixture");
        fingerprint_of(FingerprintedRequest::RetrieveEntity(&params))
    }

    /// A POST Query Entities body selecting `entities`.
    fn posted_query(entities: &str) -> Result<RequestFingerprint, EntityMapError> {
        let query = serde_json::from_str::<Query>(&format!(r#"{{"type":"Query","entities":{entities}}}"#)).expect("a valid test fixture");
        let params = EntityQueryParameters::try_from(query).expect("a valid test fixture");
        fingerprint_of(FingerprintedRequest::QueryEntities(&params))
    }

    /// A POST Query Temporal Evolution of Entities body selecting `entities`.
    fn posted_temporal_query(entities: &str) -> Result<RequestFingerprint, EntityMapError> {
        let query = serde_json::from_str::<Query>(&format!(
            r#"{{"type":"Query","entities":{entities},"temporalQ":{{"timerel":"after","timeAt":"2020-01-01T00:00:00Z"}}}}"#
        ))
        .expect("a valid test fixture");
        let params = TemporalQueryParameters::try_from(query).expect("a valid test fixture");
        fingerprint_of(FingerprintedRequest::QueryTemporal(&params))
    }

    #[test]
    fn parameter_order_and_list_order_do_not_change_the_fingerprint() -> Result<(), EntityMapError> {
        let first = entity_query("type=Vehicle&attrs=speed,brand&q=speed%3E10&scopeQ=/Madrid")?;
        let second = entity_query("scopeQ=/Madrid&q=speed%3E10&attrs=brand,speed&type=Vehicle")?;
        pretty_assertions::assert_eq!(first.compare(&second), RequestComparison::Same);
        Ok(())
    }

    #[test]
    fn identifiers_pagination_and_the_map_parameters_are_excepted() -> Result<(), EntityMapError> {
        let created = entity_query("type=Vehicle&entityMap=true&entityMapLifetime=PT1H")?;
        let follow_up = entity_query("type=Vehicle&id=urn:ngsi-ld:Vehicle:1,urn:ngsi-ld:Vehicle:2&limit=1&offset=3&count=true&options=keyValues")?;
        pretty_assertions::assert_eq!(created.compare(&follow_up), RequestComparison::Same);
        Ok(())
    }

    /// Clause 5.5.14 excepts the specification of Entity identifiers, which
    /// `idPattern` is as much as `id`: a broker forwarding under a map a source
    /// created for a pattern names the mapped ids instead.
    #[test]
    fn an_id_pattern_is_excepted_like_the_identifiers() -> Result<(), EntityMapError> {
        let created = entity_query("type=Vehicle&idPattern=urn:ngsi-ld:Vehicle:.*&entityMap=true")?;
        pretty_assertions::assert_eq!(
            created.compare(&entity_query("type=Vehicle&id=urn:ngsi-ld:Vehicle:1")?),
            RequestComparison::Same
        );
        pretty_assertions::assert_eq!(
            created.compare(&entity_query("type=Vehicle&idPattern=urn:ngsi-ld:Vehicle:1.*")?),
            RequestComparison::Same
        );
        pretty_assertions::assert_eq!(
            created.compare(&entity_query("type=Building&id=urn:ngsi-ld:Vehicle:1")?),
            RequestComparison::Changed
        );
        let temporal = temporal_query("type=Vehicle&idPattern=urn:ngsi-ld:Vehicle:.*&timerel=before&timeAt=2030-01-01T00:00:00Z")?;
        pretty_assertions::assert_eq!(
            temporal.compare(&temporal_query(
                "type=Vehicle&id=urn:ngsi-ld:Vehicle:1&timerel=before&timeAt=2030-01-01T00:00:00Z"
            )?),
            RequestComparison::Same
        );
        Ok(())
    }

    #[test]
    fn a_changed_filter_is_a_changed_request() -> Result<(), EntityMapError> {
        let created = entity_query("type=Vehicle")?;
        pretty_assertions::assert_eq!(created.compare(&entity_query("type=Vehicle&q=speed%3E99")?), RequestComparison::Changed);
        pretty_assertions::assert_eq!(created.compare(&entity_query("type=Building")?), RequestComparison::Changed);
        pretty_assertions::assert_eq!(created.compare(&entity_query("type=Vehicle&attrs=speed")?), RequestComparison::Changed);
        pretty_assertions::assert_eq!(
            created.compare(&entity_query(
                "type=Vehicle&georel=near%3BmaxDistance%3D%3D2000&geometry=Point&coordinates=%5B0%2C0%5D"
            )?),
            RequestComparison::Changed
        );
        Ok(())
    }

    #[test]
    fn the_temporal_window_and_last_n_are_excepted_for_temporal_maps() -> Result<(), EntityMapError> {
        let created = temporal_query("type=Vehicle&timerel=before&timeAt=2030-01-01T00:00:00Z&lastN=2")?;
        let follow_up = temporal_query("type=Vehicle&timerel=after&timeAt=2020-01-01T00:00:00Z&timeproperty=modifiedAt")?;
        pretty_assertions::assert_eq!(created.kind(), MapRequestKind::QueryTemporal);
        pretty_assertions::assert_eq!(created.compare(&follow_up), RequestComparison::Same);
        pretty_assertions::assert_eq!(
            created.compare(&temporal_query("type=Vehicle&q=speed%3E1&timerel=before&timeAt=2030-01-01T00:00:00Z")?),
            RequestComparison::Changed
        );
        Ok(())
    }

    #[test]
    fn a_retrieval_under_a_query_map_is_a_different_operation() -> Result<(), EntityMapError> {
        let created = entity_query("type=Vehicle")?;
        pretty_assertions::assert_eq!(created.compare(&retrieve("")?), RequestComparison::DifferentOperation);
        pretty_assertions::assert_eq!(retrieve("attrs=speed")?.compare(&retrieve("attrs=speed")?), RequestComparison::Same);
        pretty_assertions::assert_eq!(retrieve("attrs=speed")?.compare(&retrieve("attrs=brand")?), RequestComparison::Changed);
        Ok(())
    }

    /// Clause 5.7.1.3: the type selector narrows which registrations match the
    /// target entity, so it is part of what a retrieval map was created for.
    #[test]
    fn a_retrieval_type_selector_is_part_of_the_fingerprint() -> Result<(), EntityMapError> {
        pretty_assertions::assert_eq!(retrieve("type=Vehicle")?.compare(&retrieve("type=Vehicle")?), RequestComparison::Same);
        pretty_assertions::assert_eq!(retrieve("type=Vehicle")?.compare(&retrieve("")?), RequestComparison::Changed);
        pretty_assertions::assert_eq!(retrieve("type=Vehicle")?.compare(&retrieve("type=Building")?), RequestComparison::Changed);
        pretty_assertions::assert_eq!(
            retrieve("type=Vehicle|Building")?.compare(&retrieve("type=Building,Vehicle")?),
            RequestComparison::Same
        );
        Ok(())
    }

    #[test]
    fn a_post_body_selector_fingerprints_by_type_without_identifiers_or_pattern() -> Result<(), EntityMapError> {
        let created = posted_query(r#"[{"type":"Vehicle","idPattern":"urn:ngsi-ld:Vehicle:.*"},{"type":"Building"}]"#)?;
        pretty_assertions::assert_eq!(
            created.compare(&posted_query(r#"[{"type":"Building"},{"type":"Vehicle","id":"urn:ngsi-ld:Vehicle:2"}]"#)?),
            RequestComparison::Same
        );
        pretty_assertions::assert_eq!(
            created.compare(&posted_query(
                r#"[{"type":"Building"},{"type":"Vehicle","id":"urn:ngsi-ld:Vehicle:1"},{"type":"Vehicle","id":"urn:ngsi-ld:Vehicle:2"}]"#
            )?),
            RequestComparison::Same
        );
        pretty_assertions::assert_eq!(created.compare(&posted_query(r#"[{"type":"Vehicle"}]"#)?), RequestComparison::Changed);
        Ok(())
    }

    /// Clause 5.7.2.3 gives Query Entities one selector of entity types, which
    /// the GET `type` parameter and the POST `entities[]` carry alike.
    #[test]
    fn one_type_digests_alike_through_either_binding() -> Result<(), EntityMapError> {
        let through_get = entity_query("type=Vehicle")?;
        pretty_assertions::assert_eq!(through_get.compare(&posted_query(r#"[{"type":"Vehicle"}]"#)?), RequestComparison::Same);
        pretty_assertions::assert_eq!(
            posted_query(r#"[{"type":"Vehicle","id":"urn:ngsi-ld:Vehicle:1"}]"#)?.compare(&through_get),
            RequestComparison::Same
        );
        pretty_assertions::assert_eq!(through_get.compare(&posted_query(r#"[{"type":"Building"}]"#)?), RequestComparison::Changed);
        Ok(())
    }

    /// The operands of a clause 4.17 disjunction and the selectors of a POST
    /// body are both alternatives, written in any order and either spelling of
    /// the operator.
    #[test]
    fn type_alternatives_digest_alike_through_either_binding() -> Result<(), EntityMapError> {
        let through_get = entity_query("type=Vehicle|Building")?;
        pretty_assertions::assert_eq!(
            through_get.compare(&posted_query(r#"[{"type":"Vehicle"},{"type":"Building"}]"#)?),
            RequestComparison::Same
        );
        pretty_assertions::assert_eq!(through_get.compare(&posted_query(r#"[{"type":"Building|Vehicle"}]"#)?), RequestComparison::Same);
        pretty_assertions::assert_eq!(through_get.compare(&entity_query("type=Building,Vehicle")?), RequestComparison::Same);
        pretty_assertions::assert_eq!(through_get.compare(&entity_query("type=Vehicle|Building|Vehicle")?), RequestComparison::Same);
        pretty_assertions::assert_eq!(
            entity_query("type=(Home;Vehicle)|Motorhome")?.compare(&posted_query(r#"[{"type":"Motorhome"},{"type":"(Home;Vehicle)"}]"#)?),
            RequestComparison::Same
        );
        pretty_assertions::assert_eq!(through_get.compare(&posted_query(r#"[{"type":"Vehicle"}]"#)?), RequestComparison::Changed);
        pretty_assertions::assert_eq!(
            posted_query(r#"[{"type":"Vehicle"},{"type":"Building"}]"#)?.compare(&entity_query("type=Vehicle|Tower")?),
            RequestComparison::Changed
        );
        Ok(())
    }

    /// Clause 5.7.4.3 gives Query Temporal Evolution of Entities the same one
    /// selector of entity types.
    #[test]
    fn a_temporal_type_selection_digests_alike_through_either_binding() -> Result<(), EntityMapError> {
        let through_get = temporal_query("type=Vehicle|Building&timerel=before&timeAt=2030-01-01T00:00:00Z")?;
        pretty_assertions::assert_eq!(
            through_get.compare(&posted_temporal_query(r#"[{"type":"Building"},{"type":"Vehicle"}]"#)?),
            RequestComparison::Same
        );
        pretty_assertions::assert_eq!(
            through_get.compare(&posted_temporal_query(r#"[{"type":"Vehicle"}]"#)?),
            RequestComparison::Changed
        );
        Ok(())
    }
}
