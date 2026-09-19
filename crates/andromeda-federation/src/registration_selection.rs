//! Registration-information matching and federated-fragment partitioning.
//!
//! A registration stores the type and attribute names it covers expanded (as
//! IRIs). A forwarded write body, on the other hand, reaches this module with
//! its members named exactly as the client wrote them -- the HTTP `JsonLd`
//! extractor only strips `@context`, it does not expand the body -- so deciding
//! whether a registration covers a body member means expanding that member
//! through the triggering request's own context and comparing IRIs. That is the
//! same comparison [`information_matches_entity`] makes on the entity axis, so
//! selection and narrowing agree even under a `@context` that aliases a term to
//! an unrelated IRI. The per-registration re-compaction that later builds the
//! wire body happens elsewhere (`crate::forwarded_payload`); here the request
//! context is the one that matters.
//!
//! A write body may carry no `@context` at all (clause 6.3.5 makes
//! `application/json` without a context `Link` legal, resolving against the core
//! context), while the registration's declared names were expanded under a
//! different context that the registration does not retain -- only the expanded
//! IRIs survive. The two then cannot meet on an IRI, so coverage also accepts a
//! body member whose NGSI-LD short name (the fragment or last path segment,
//! Table 5.2.28-1) equals a declared IRI's short name. The exact-IRI comparison
//! stays primary and is what an aliased context relies on; the short-name
//! comparison is the fallback for a context the write body does not share.

use crate::{error::FederationError, term_resolver::TermResolver};
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, scope::grammar::ScopeBuf, uri::Uri};
use andromeda_ngsi_ld_model::registration::{ContextSourceRegistration, info::RegistrationInfo};
use andromeda_ngsi_ld_query::scope::query::ScopeQuery;
use iri_rs::IriBuf;
use serde_json::{Map, Value};

/// Attribute names from matching information entries that this request covers.
///
/// # Errors
///
/// Returns [`FederationError::JsonLd`] when a requested attribute cannot be
/// expanded against the request context.
pub(crate) async fn collect_matching_attrs(
    csr: &ContextSourceRegistration,
    entity_type: Option<&IriBuf>,
    entity_id: Option<&Uri>,
    explicit_attribute: Option<&AttributeName>,
    body: Option<&[u8]>,
    resolver: &TermResolver<'_>,
) -> Result<Vec<AttributeName>, FederationError> {
    if let Some(attribute) = explicit_attribute {
        return Ok(vec![attribute.clone()]); // clone: the result outlives the borrowed request.
    }
    let requested = body.and_then(fragment_attribute_names).unwrap_or_default();
    let mut covered = Vec::new();
    for information in csr
        .coverage
        .information
        .iter()
        .filter(|information| information_matches_entity(information, entity_type, entity_id))
    {
        let declared = information.property_names.iter().chain(&information.relationship_names).flatten();
        let declared = declared.collect::<Vec<_>>();
        if declared.is_empty() {
            covered.extend(requested.iter().cloned());
        } else if requested.is_empty() {
            covered.extend(declared.iter().map(|name| (*name).clone()));
        } else {
            for attribute in &requested {
                let iri = resolver.expand_attr(attribute.as_str()).await?;
                if declared.iter().any(|name| attribute_matches_declared(attribute.as_str(), iri.as_str(), name)) {
                    covered.push(attribute.clone()); // clone: the result outlives the borrowed request.
                }
            }
        }
    }
    covered.sort();
    covered.dedup();
    Ok(covered)
}

/// Retain only attributes covered by matching information entries.
///
/// # Errors
///
/// Returns [`FederationError::JsonLd`] when a body attribute cannot be expanded
/// against the request context.
pub(crate) async fn filter_fragment_for_registration(
    body: Option<&[u8]>,
    csr: &ContextSourceRegistration,
    entity_type: Option<&IriBuf>,
    entity_id: Option<&Uri>,
    resolver: &TermResolver<'_>,
) -> Result<Option<Vec<u8>>, FederationError> {
    let Some(body) = body else {
        return Ok(None);
    };
    let Ok(mut value) = serde_json::from_slice::<Value>(body) else {
        return Ok(Some(body.to_vec()));
    };
    let Some(object) = value.as_object_mut() else {
        return Ok(Some(body.to_vec()));
    };
    if !retain_covered_attributes(object, csr, entity_type, entity_id, resolver).await? {
        return Ok(None);
    }
    Ok(serde_json::to_vec(&value).ok())
}

/// The attributes a registration covers for one entity, read off the
/// information entries that match it (clause 5.12).
enum AttributeCoverage {
    /// No information entry matches the entity: nothing is covered.
    NotMatching,
    /// A matching entry declares no names, so every attribute is covered.
    Unrestricted,
    /// Only these declared names, stored expanded as the IRIs a body member is
    /// compared against once it is itself expanded.
    Names(Vec<AttributeName>),
}

/// What `csr` covers of an entity's attributes.
fn attribute_coverage(csr: &ContextSourceRegistration, entity_type: Option<&IriBuf>, entity_id: Option<&Uri>) -> AttributeCoverage {
    let information = csr
        .coverage
        .information
        .iter()
        .filter(|information| information_matches_entity(information, entity_type, entity_id))
        .collect::<Vec<_>>();
    if information.is_empty() {
        return AttributeCoverage::NotMatching;
    }
    let unrestricted = information
        .iter()
        .any(|entry| entry.property_names.as_ref().is_none_or(Vec::is_empty) && entry.relationship_names.as_ref().is_none_or(Vec::is_empty));
    if unrestricted {
        return AttributeCoverage::Unrestricted;
    }
    AttributeCoverage::Names(
        information
            .iter()
            .flat_map(|entry| entry.property_names.iter().chain(&entry.relationship_names).flatten())
            .cloned() // clone: the coverage owns the declared names borrowed from the registration
            .collect(),
    )
}

/// The members of `object` a registration declaring `declared` covers.
///
/// Each non-control member is expanded through the request context and kept when
/// it matches a declared name, so a body key aliased by the `@context` is
/// matched by what it means rather than by the last segment of the declared IRI.
async fn covered_members(object: &Map<String, Value>, declared: &[AttributeName], resolver: &TermResolver<'_>) -> Result<Vec<String>, FederationError> {
    let mut covered = Vec::new();
    for name in object.keys().filter(|name| !is_entity_control_member(name)) {
        let iri = resolver.expand_attr(name).await?;
        if declared.iter().any(|declared| attribute_matches_declared(name, iri.as_str(), declared)) {
            covered.push(name.clone()); // clone: the result outlives the borrow of the object
        }
    }
    Ok(covered)
}

/// Whether a write body member named `member`, expanding to `member_iri` under
/// the request context, is the attribute a registration `declared` (stored
/// expanded).
///
/// The IRI comparison is exact and primary: it matches when the write and the
/// registration share the context, including an aliased one. The short-name
/// comparison is the fallback for a write body carrying a different context, or
/// none, than the registration's declared names were expanded under -- the
/// registration does not retain that context, so the NGSI-LD short name is the
/// only remaining common reference (clause 6.3.5, Table 5.2.28-1).
fn attribute_matches_declared(member: &str, member_iri: &str, declared: &AttributeName) -> bool {
    declared.as_str() == member_iri || attribute_short_name(declared.as_str()) == Some(member)
}

/// The short name an NGSI-LD attribute IRI compacts to: its fragment, or its
/// last path segment (Table 5.2.28-1).
fn attribute_short_name(iri: &str) -> Option<&str> {
    match iri.rfind('#') {
        Some(position) => Some(&iri[position + 1..]),
        None => iri.rfind('/').map(|position| &iri[position + 1..]),
    }
}

/// Narrow an entity object to the attributes `csr` covers for it, keeping the
/// entity control members (clauses 5.6.7.4 and following: "remove
/// non-matching Attributes from the remaining Entities"). Returns whether any
/// attribute remains.
///
/// # Errors
///
/// Returns [`FederationError::JsonLd`] when a body attribute cannot be expanded
/// against the request context.
pub(crate) async fn retain_covered_attributes(
    object: &mut Map<String, Value>,
    csr: &ContextSourceRegistration,
    entity_type: Option<&IriBuf>,
    entity_id: Option<&Uri>,
    resolver: &TermResolver<'_>,
) -> Result<bool, FederationError> {
    match attribute_coverage(csr, entity_type, entity_id) {
        AttributeCoverage::NotMatching => object.retain(|name, _value| is_entity_control_member(name)),
        AttributeCoverage::Unrestricted => {}
        AttributeCoverage::Names(declared) => {
            let kept = covered_members(object, &declared, resolver).await?;
            object.retain(|name, _value| is_entity_control_member(name) || kept.contains(name));
        }
    }
    Ok(object.keys().any(|name| !is_entity_control_member(name)))
}

/// The attribute members of `object` that `csr` covers for the entity: what
/// another registration's copy loses when `csr` owns them exclusively
/// (clauses 5.6.7.4 and following).
///
/// # Errors
///
/// Returns [`FederationError::JsonLd`] when a body attribute cannot be expanded
/// against the request context.
pub(crate) async fn covered_attribute_members(
    object: &Map<String, Value>,
    csr: &ContextSourceRegistration,
    entity_type: Option<&IriBuf>,
    entity_id: Option<&Uri>,
    resolver: &TermResolver<'_>,
) -> Result<Vec<String>, FederationError> {
    match attribute_coverage(csr, entity_type, entity_id) {
        AttributeCoverage::NotMatching => Ok(Vec::new()),
        // clone: the result owns the member names it reports
        AttributeCoverage::Unrestricted => Ok(object.keys().filter(|name| !is_entity_control_member(name)).cloned().collect()),
        AttributeCoverage::Names(declared) => covered_members(object, &declared, resolver).await,
    }
}

/// Attribute names present in an entity or entity-fragment payload.
pub(crate) fn fragment_attribute_names(body: &[u8]) -> Option<Vec<AttributeName>> {
    let value = serde_json::from_slice::<Value>(body).ok()?;
    let object = value.as_object()?;
    Some(
        object
            .keys()
            .filter(|name| !is_entity_control_member(name))
            .filter_map(|name| name.parse().ok())
            .collect(),
    )
}

/// Whether a member is entity metadata rather than a forwarded attribute.
pub(crate) fn is_entity_control_member(name: &str) -> bool {
    matches!(name, "@context" | "id" | "type" | "scope" | "createdAt" | "modifiedAt" | "expiresAt")
}

/// Whether one information entry covers the entity and requested attribute. The
/// attribute is expanded to its IRI by the caller so the comparison is exact.
pub(crate) fn csr_has_matching_information(
    csr: &ContextSourceRegistration,
    entity_type: Option<&IriBuf>,
    entity_id: Option<&Uri>,
    attribute: Option<&IriBuf>,
) -> bool {
    csr.coverage
        .information
        .iter()
        .any(|information| information_matches_entity(information, entity_type, entity_id) && information_covers_attribute(information, attribute))
}

/// Whether a registration's declared scope can answer a scoped read.
pub(crate) fn registration_scope_admits(registration: &ContextSourceRegistration, scope_q: Option<&ScopeQuery>) -> bool {
    let Some(scope_q) = scope_q else {
        return true;
    };
    registration
        .coverage
        .scope
        .as_ref()
        .is_none_or(|scopes| scope_q.matches(&scopes.iter().map(ScopeBuf::as_str).collect::<Vec<_>>()))
}

/// Whether the entity selector axis of one information entry matches.
pub(crate) fn information_matches_entity(information: &RegistrationInfo, entity_type: Option<&IriBuf>, entity_id: Option<&Uri>) -> bool {
    let Some(selectors) = information.entities.as_deref().filter(|selectors| !selectors.is_empty()) else {
        return true;
    };
    selectors.iter().any(|selector| {
        let type_matches = entity_type.is_none_or(|requested| selector.type_.admits(requested.as_str()));
        let id_matches = entity_id.is_none_or(|requested| match (&selector.id, &selector.id_pattern) {
            (None, None) => true,
            (Some(id), None) => id.as_str() == requested.as_str(),
            (None, Some(pattern)) => pattern.is_match(requested.as_str()),
            (Some(id), Some(pattern)) => id.as_str() == requested.as_str() || pattern.is_match(requested.as_str()),
        });
        type_matches && id_matches
    })
}

/// Whether an attribute is declared by the same information entry. The declared
/// names are stored expanded, so the pre-expanded attribute IRI is compared
/// against them directly.
pub(crate) fn information_covers_attribute(information: &RegistrationInfo, attribute: Option<&IriBuf>) -> bool {
    let Some(attribute) = attribute else {
        return true;
    };
    let names = information.property_names.iter().chain(&information.relationship_names).flatten();
    let mut saw_name = false;
    for name in names {
        saw_name = true;
        if name.as_str() == attribute.as_str() {
            return true;
        }
    }
    !saw_name
}

#[cfg(test)]
mod tests {
    use crate::{
        registration_selection::{covered_attribute_members, retain_covered_attributes},
        term_resolver::TermResolver,
    };
    use andromeda_jsonld::{context_spec::ContextSpec, engine::processor::JsonLdEngine};
    use andromeda_ngsi_ld_lexicon::uri::Uri;
    use andromeda_ngsi_ld_model::registration::ContextSourceRegistration;
    use iri_rs::IriBuf;
    use serde_json::{Map, Value, json};

    /// An engine that resolves only what is embedded, never the network.
    fn offline_engine() -> JsonLdEngine {
        JsonLdEngine::builder().allow_remote(false).build().expect("engine builds")
    }

    fn registration(information: &Value) -> ContextSourceRegistration {
        serde_json::from_value(json!({
            "id": "urn:ngsi-ld:csr:1",
            "type": "ContextSourceRegistration",
            "endpoint": "https://peer.example.org",
            "information": [information],
            "status": "ok",
            "createdAt": "2024-01-01T00:00:00Z",
            "modifiedAt": "2024-01-01T00:00:00Z"
        }))
        .expect("valid registration")
    }

    fn vehicle() -> Map<String, Value> {
        json!({
            "id": "urn:ngsi-ld:Vehicle:1",
            "type": "Vehicle",
            "speed": {"type": "Property", "value": 1},
            "https://example.org/brandName": {"type": "Property", "value": "x"}
        })
        .as_object()
        .expect("object")
        .clone() // clone: each case mutates its own copy
    }

    /// A Vehicle whose speed member is named by the alias `spd`, whose declared
    /// IRI's last segment is `speed` -- the token the removed heuristic guessed.
    fn aliased_vehicle() -> Map<String, Value> {
        json!({
            "id": "urn:ngsi-ld:Vehicle:1",
            "type": "Veh",
            "spd": {"type": "Property", "value": 1},
            "https://example.org/brandName": {"type": "Property", "value": "x"}
        })
        .as_object()
        .expect("object")
        .clone() // clone: each case mutates its own copy
    }

    fn vehicle_type() -> IriBuf {
        IriBuf::new("https://example.org/Vehicle".to_string()).expect("valid IRI")
    }

    fn vehicle_id() -> Uri {
        "urn:ngsi-ld:Vehicle:1".parse().expect("valid URI")
    }

    /// A registration naming attributes keeps a body member when the member's
    /// expanded IRI is one of the declared IRIs. Under an aliased `@context`
    /// `spd` expands to the declared speed IRI and is kept, where the removed
    /// last-segment heuristic (which produced `speed`) would have dropped it;
    /// the full-IRI member matches directly.
    #[tokio::test]
    async fn a_registration_naming_attributes_keeps_them_by_iri_under_an_aliased_context() {
        let engine = offline_engine();
        let context = ContextSpec::none().with_body_context(Some(json!({
            "Veh": "https://example.org/Vehicle",
            "spd": "https://example.org/vocab/speed"
        })));
        let resolver = TermResolver::new(&engine, &context);
        let csr = registration(&json!({
            "entities": [{"type": "https://example.org/Vehicle"}],
            "propertyNames": ["https://example.org/vocab/speed", "https://example.org/brandName"]
        }));
        let mut object = aliased_vehicle();
        assert!(
            retain_covered_attributes(&mut object, &csr, Some(&vehicle_type()), Some(&vehicle_id()), &resolver)
                .await
                .expect("coverage resolves")
        );
        let mut kept = object.keys().cloned().collect::<Vec<_>>();
        kept.sort();
        pretty_assertions::assert_eq!(kept, vec!["https://example.org/brandName", "id", "spd", "type"]);
        let mut covered = covered_attribute_members(&aliased_vehicle(), &csr, Some(&vehicle_type()), Some(&vehicle_id()), &resolver)
            .await
            .expect("coverage resolves");
        covered.sort();
        pretty_assertions::assert_eq!(covered, vec!["https://example.org/brandName", "spd"]);
    }

    #[tokio::test]
    async fn a_registration_declaring_no_attribute_names_covers_every_attribute() {
        let engine = offline_engine();
        let context = ContextSpec::none();
        let resolver = TermResolver::new(&engine, &context);
        let csr = registration(&json!({"entities": [{"type": "https://example.org/Vehicle"}]}));
        let mut object = vehicle();
        assert!(
            retain_covered_attributes(&mut object, &csr, Some(&vehicle_type()), Some(&vehicle_id()), &resolver)
                .await
                .expect("coverage resolves")
        );
        pretty_assertions::assert_eq!(object.len(), 4);
        pretty_assertions::assert_eq!(
            covered_attribute_members(&vehicle(), &csr, Some(&vehicle_type()), Some(&vehicle_id()), &resolver)
                .await
                .expect("coverage resolves")
                .len(),
            2
        );
    }

    #[tokio::test]
    async fn a_registration_not_matching_the_entity_keeps_nothing() {
        let engine = offline_engine();
        let context = ContextSpec::none();
        let resolver = TermResolver::new(&engine, &context);
        let csr = registration(&json!({"entities": [{"type": "https://example.org/Device"}]}));
        let mut object = vehicle();
        assert!(
            !retain_covered_attributes(&mut object, &csr, Some(&vehicle_type()), Some(&vehicle_id()), &resolver)
                .await
                .expect("coverage resolves")
        );
        let mut kept = object.keys().cloned().collect::<Vec<_>>();
        kept.sort();
        pretty_assertions::assert_eq!(kept, vec!["id", "type"]);
        assert_eq!(
            covered_attribute_members(&vehicle(), &csr, Some(&vehicle_type()), Some(&vehicle_id()), &resolver)
                .await
                .expect("coverage resolves"),
            [] as [String; 0]
        );
    }

    #[tokio::test]
    async fn a_registration_naming_only_absent_attributes_leaves_no_attribute() {
        let engine = offline_engine();
        let context = ContextSpec::none();
        let resolver = TermResolver::new(&engine, &context);
        let csr = registration(&json!({
            "entities": [{"type": "https://example.org/Vehicle"}],
            "propertyNames": ["https://example.org/colour"]
        }));
        let mut object = vehicle();
        assert!(
            !retain_covered_attributes(&mut object, &csr, Some(&vehicle_type()), Some(&vehicle_id()), &resolver)
                .await
                .expect("coverage resolves")
        );
    }
}
