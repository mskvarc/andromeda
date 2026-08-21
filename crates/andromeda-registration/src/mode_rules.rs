//! The mode-specific admission rules of clauses 5.9.2.4 and 5.9.3.4.
//!
//! The structural rules need only the registration itself: an exclusive
//! registration names one entity by id and at least one attribute (clause
//! 4.3.6.3), and an auxiliary registration advertises consumption operations
//! only. The overlap rules need the store and the local entities: a proxied
//! footprint may collide with another proxied registration or with context data
//! the broker already holds, and clause 5.9.2.4 makes either a `Conflict`.

use crate::{
    entity_probe::{EntityPresence, RegisteredEntityProbe},
    error::RegistrationError,
    store::RegistrationStore,
};
use andromeda_ngsi_ld_lexicon::uri::Uri;
use andromeda_ngsi_ld_model::{
    registration::{ContextSourceRegistration, CsrOperation, ForwardingMode, RegistrationInfo},
    subscription::EntitySelector,
};
use iri_rs::IriBuf;

/// The operations clause 5.9.2.4 admits on an auxiliary registration.
const AUXILIARY_OPERATIONS: [CsrOperation; 3] = [CsrOperation::RetrieveOps, CsrOperation::RetrieveEntity, CsrOperation::QueryEntity];

/// The mode-dependent shape rules that need nothing beyond the registration.
pub(super) fn validate_structure(mode: ForwardingMode, information: &[RegistrationInfo], operations: &[CsrOperation]) -> Result<(), RegistrationError> {
    match mode {
        ForwardingMode::Exclusive => information.iter().try_for_each(validate_exclusive_entry),
        ForwardingMode::Auxiliary => {
            if operations.iter().all(|operation| AUXILIARY_OPERATIONS.contains(operation)) {
                Ok(())
            } else {
                Err(RegistrationError::csr_validation(
                    "an auxiliary registration may only advertise retrieveOps, retrieveEntity or queryEntity",
                ))
            }
        }
        ForwardingMode::Inclusive | ForwardingMode::Redirect => Ok(()),
    }
}

/// Clause 4.3.6.3: an exclusive registration relates to specific attributes
/// of a single entity, so every block names entities by id and at least one
/// attribute.
fn validate_exclusive_entry(entry: &RegistrationInfo) -> Result<(), RegistrationError> {
    let selectors = entry
        .entities
        .as_deref()
        .ok_or_else(|| RegistrationError::csr_validation("an exclusive registration must name the entity it covers by id"))?;
    if selectors.iter().any(|selector| selector.id.is_none() || selector.id_pattern.is_some()) {
        return Err(RegistrationError::csr_validation(
            "an exclusive registration must name each covered entity by id; idPattern and type-only selectors are not supported",
        ));
    }
    if entry.attribute_names().next().is_none() {
        return Err(RegistrationError::csr_validation(
            "an exclusive registration must name the attributes it covers in propertyNames or relationshipNames",
        ));
    }
    Ok(())
}

/// The attribute IRIs a block declares, on both axes.
fn declared_attributes(entry: &RegistrationInfo) -> Vec<IriBuf> {
    entry.attribute_names().filter_map(|name| IriBuf::new(name.as_str().to_string()).ok()).collect()
}

/// Whether `selector` covers the concrete identifier `entity_id` (clause 5.10.2).
fn selector_covers_id(selector: &EntitySelector, entity_id: &Uri) -> bool {
    match (&selector.id, &selector.id_pattern) {
        (Some(id), _) => id == entity_id,
        (None, Some(pattern)) => pattern.is_match(entity_id.as_str()),
        (None, None) => true,
    }
}

/// Whether a block covers `entity_id`: a block without entity selectors
/// constrains the entity axis not at all.
fn entry_covers_id(entry: &RegistrationInfo, entity_id: &Uri) -> bool {
    entry
        .entities
        .as_deref()
        .is_none_or(|selectors| selectors.iter().any(|selector| selector_covers_id(selector, entity_id)))
}

/// Whether a block's attribute footprint meets `attributes`: a registration
/// that names no attribute registers the whole entity (clause 4.3.6.3), so an
/// empty set on either side overlaps everything on the other.
fn entry_overlaps_attributes(entry: &RegistrationInfo, attributes: &[IriBuf]) -> bool {
    let mut declared = entry.attribute_names().peekable();
    if attributes.is_empty() || declared.peek().is_none() {
        return true;
    }
    declared.any(|name| attributes.iter().any(|attribute| attribute.as_str() == name.as_str()))
}

/// Whether an existing registration overlaps the footprint `(entity_id, attributes)`.
fn registration_overlaps(existing: &ContextSourceRegistration, entity_id: &Uri, attributes: &[IriBuf]) -> bool {
    existing
        .coverage
        .information
        .iter()
        .any(|entry| entry_covers_id(entry, entity_id) && entry_overlaps_attributes(entry, attributes))
}

/// Reject the footprint when another proxied registration already claims it.
async fn reject_overlapping_registrations(
    repo: &dyn RegistrationStore,
    self_id: Option<&Uri>,
    entity_id: &Uri,
    attributes: &[IriBuf],
    colliding_modes: &[ForwardingMode],
) -> Result<(), RegistrationError> {
    let candidates = repo.find_overlapping(Some(entity_id), attributes).await?;
    let collision = candidates.iter().find(|candidate| {
        self_id != Some(&candidate.id)
            && colliding_modes.contains(&candidate.access.mode.unwrap_or_default())
            && registration_overlaps(candidate, entity_id, attributes)
    });
    match collision {
        Some(existing) => Err(RegistrationError::conflict(format!(
            "registration {} already covers entity {entity_id} for the registered attributes",
            existing.id
        ))),
        None => Ok(()),
    }
}

/// Clause 5.9.2.4 exclusive rules, per entity the registration names.
async fn check_exclusive(
    repo: &dyn RegistrationStore,
    probe: &dyn RegisteredEntityProbe,
    self_id: Option<&Uri>,
    information: &[RegistrationInfo],
) -> Result<(), RegistrationError> {
    for entry in information {
        let attributes = declared_attributes(entry);
        for selector in entry.entities.as_deref().unwrap_or_default() {
            let Some(entity_id) = selector.id.as_ref() else {
                continue;
            };
            reject_overlapping_registrations(repo, self_id, entity_id, &attributes, &[ForwardingMode::Exclusive, ForwardingMode::Redirect]).await?;
            if probe.holds_any_attribute(entity_id, &attributes).await? == EntityPresence::Present {
                return Err(RegistrationError::conflict(format!(
                    "entity {entity_id} already holds one of the attributes the exclusive registration covers"
                )));
            }
        }
    }
    Ok(())
}

/// Clause 5.9.2.4 redirect rule, plus clause 4.3.6.3's bar on a redirect
/// registration re-claiming a footprint an exclusive registration already holds.
async fn check_redirect(
    repo: &dyn RegistrationStore,
    probe: &dyn RegisteredEntityProbe,
    self_id: Option<&Uri>,
    information: &[RegistrationInfo],
) -> Result<(), RegistrationError> {
    for entry in information {
        let attributes = declared_attributes(entry);
        match entry.entities.as_deref() {
            Some(selectors) => {
                for selector in selectors {
                    if let Some(entity_id) = selector.id.as_ref() {
                        reject_overlapping_registrations(repo, self_id, entity_id, &attributes, &[ForwardingMode::Exclusive]).await?;
                    }
                    reject_matching_entity(probe, Some(selector), &attributes).await?;
                }
            }
            None => reject_matching_entity(probe, None, &attributes).await?,
        }
    }
    Ok(())
}

/// Reject the footprint when a local entity already matches it.
async fn reject_matching_entity(probe: &dyn RegisteredEntityProbe, selector: Option<&EntitySelector>, attributes: &[IriBuf]) -> Result<(), RegistrationError> {
    match probe.any_entity_matching(selector, attributes).await? {
        EntityPresence::Present => Err(RegistrationError::conflict(
            "a local entity already matches the redirect registration's entity and attribute footprint",
        )),
        EntityPresence::Absent => Ok(()),
    }
}

/// The overlap rules that need the store and the local entities. `self_id` is
/// the registration being updated, which never conflicts with itself.
pub(super) async fn check_conflicts(
    repo: &dyn RegistrationStore,
    probe: &dyn RegisteredEntityProbe,
    mode: ForwardingMode,
    information: &[RegistrationInfo],
    self_id: Option<&Uri>,
) -> Result<(), RegistrationError> {
    match mode {
        ForwardingMode::Exclusive => check_exclusive(repo, probe, self_id, information).await,
        ForwardingMode::Redirect => check_redirect(repo, probe, self_id, information).await,
        ForwardingMode::Inclusive | ForwardingMode::Auxiliary => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        entity_probe::{EntityPresence, RegisteredEntityProbe},
        error::RegistrationError,
        mode_rules::{check_conflicts, validate_structure},
        resolved_update::ResolvedCsrUpdate,
        store::{CreateCsrParams, ForwardingOutcome, RegistrationMutation, RegistrationPage, RegistrationStore},
    };
    use andromeda_ngsi_ld_lexicon::uri::Uri;
    use andromeda_ngsi_ld_model::{
        registration::{ContextSourceRegistration, CsrOperation, ForwardingMode, RegistrationInfo},
        subscription::EntitySelector,
    };
    use andromeda_ngsi_ld_query::{csr::compiled::CompiledCsrQuery, pagination::Pagination};
    use chrono::{DateTime, Utc};
    use futures::future::BoxFuture;
    use iri_rs::IriBuf;
    use serde_json::json;
    use std::{slice, sync::Mutex};

    /// A store holding a fixed registration list; `find_overlapping` returns
    /// every one, as a superset preselect is allowed to.
    struct FixedStore(Vec<ContextSourceRegistration>);

    impl RegistrationStore for FixedStore {
        fn delete_expired<'life0, 'async_trait>(
            &'life0 self,
            _cutoff: DateTime<Utc>,
        ) -> BoxFuture<'async_trait, Result<Vec<ContextSourceRegistration>, RegistrationError>>
        where
            'life0: 'async_trait,
        {
            Box::pin(async { Ok(Vec::new()) })
        }

        fn exists<'life0, 'life1, 'async_trait>(&'life0 self, _id: &'life1 Uri) -> BoxFuture<'async_trait, Result<bool, RegistrationError>>
        where
            'life0: 'async_trait,
            'life1: 'async_trait,
        {
            Box::pin(async { Ok(false) })
        }

        fn create<'life0, 'life1, 'life2, 'async_trait>(
            &'life0 self,
            _id: &'life1 Uri,
            _params: &'life2 CreateCsrParams,
        ) -> BoxFuture<'async_trait, Result<(), RegistrationError>>
        where
            'life0: 'async_trait,
            'life1: 'async_trait,
            'life2: 'async_trait,
        {
            Box::pin(async { Ok(()) })
        }

        fn get<'life0, 'life1, 'async_trait>(
            &'life0 self,
            _id: &'life1 Uri,
        ) -> BoxFuture<'async_trait, Result<Option<ContextSourceRegistration>, RegistrationError>>
        where
            'life0: 'async_trait,
            'life1: 'async_trait,
        {
            Box::pin(async { Ok(None) })
        }

        fn list<'life0, 'async_trait>(&'life0 self, _pagination: Pagination) -> BoxFuture<'async_trait, Result<RegistrationPage, RegistrationError>>
        where
            'life0: 'async_trait,
        {
            Box::pin(async {
                Ok(RegistrationPage {
                    registrations: Vec::new(),
                    total: 0,
                })
            })
        }

        fn query_compiled<'life0, 'life1, 'async_trait>(
            &'life0 self,
            _compiled: &'life1 CompiledCsrQuery,
        ) -> BoxFuture<'async_trait, Result<RegistrationPage, RegistrationError>>
        where
            'life0: 'async_trait,
            'life1: 'async_trait,
        {
            Box::pin(async {
                Ok(RegistrationPage {
                    registrations: Vec::new(),
                    total: 0,
                })
            })
        }

        fn update<'life0, 'life1, 'life2, 'async_trait>(
            &'life0 self,
            _id: &'life1 Uri,
            _values: &'life2 ResolvedCsrUpdate,
        ) -> BoxFuture<'async_trait, Result<RegistrationMutation, RegistrationError>>
        where
            'life0: 'async_trait,
            'life1: 'async_trait,
            'life2: 'async_trait,
        {
            Box::pin(async { Ok(RegistrationMutation::Applied) })
        }

        fn find_matching<'life0, 'life1, 'life2, 'async_trait>(
            &'life0 self,
            _entity_type: Option<&'life1 IriBuf>,
            _entity_id: Option<&'life2 Uri>,
        ) -> BoxFuture<'async_trait, Result<Vec<ContextSourceRegistration>, RegistrationError>>
        where
            'life0: 'async_trait,
            'life1: 'async_trait,
            'life2: 'async_trait,
        {
            Box::pin(async { Ok(Vec::new()) })
        }

        fn find_overlapping<'life0, 'life1, 'life2, 'async_trait>(
            &'life0 self,
            _entity_id: Option<&'life1 Uri>,
            _attributes: &'life2 [IriBuf],
        ) -> BoxFuture<'async_trait, Result<Vec<ContextSourceRegistration>, RegistrationError>>
        where
            'life0: 'async_trait,
            'life1: 'async_trait,
            'life2: 'async_trait,
        {
            Box::pin(async { Ok(self.0.clone()) }) // clone: the fake hands out owned registrations like the port requires
        }

        fn find_all_active<'life0, 'life1, 'async_trait>(
            &'life0 self,
            _after: Option<&'life1 Uri>,
            _limit: u32,
        ) -> BoxFuture<'async_trait, Result<Vec<ContextSourceRegistration>, RegistrationError>>
        where
            'life0: 'async_trait,
            'life1: 'async_trait,
        {
            Box::pin(async { Ok(Vec::new()) })
        }

        fn delete<'life0, 'life1, 'async_trait>(&'life0 self, _id: &'life1 Uri) -> BoxFuture<'async_trait, Result<RegistrationMutation, RegistrationError>>
        where
            'life0: 'async_trait,
            'life1: 'async_trait,
        {
            Box::pin(async { Ok(RegistrationMutation::Applied) })
        }

        fn record_forwarding_outcome<'life0, 'life1, 'async_trait>(
            &'life0 self,
            _id: &'life1 Uri,
            _outcome: ForwardingOutcome,
            _at: DateTime<Utc>,
        ) -> BoxFuture<'async_trait, Result<(), RegistrationError>>
        where
            'life0: 'async_trait,
            'life1: 'async_trait,
        {
            Box::pin(async { Ok(()) })
        }
    }

    /// A probe answering a fixed presence and recording what it was asked.
    struct FixedProbe {
        holds: EntityPresence,
        matches: EntityPresence,
        asked: Mutex<Vec<String>>,
    }

    impl FixedProbe {
        fn new(holds: EntityPresence, matches: EntityPresence) -> FixedProbe {
            FixedProbe {
                holds,
                matches,
                asked: Mutex::new(Vec::new()),
            }
        }
    }

    impl RegisteredEntityProbe for FixedProbe {
        fn holds_any_attribute<'life0, 'life1, 'life2, 'async_trait>(
            &'life0 self,
            entity_id: &'life1 Uri,
            _attributes: &'life2 [IriBuf],
        ) -> BoxFuture<'async_trait, Result<EntityPresence, RegistrationError>>
        where
            'life0: 'async_trait,
            'life1: 'async_trait,
            'life2: 'async_trait,
        {
            self.asked.lock().expect("probe log").push(format!("holds:{entity_id}"));
            Box::pin(async { Ok(self.holds) })
        }

        fn any_entity_matching<'life0, 'life1, 'life2, 'async_trait>(
            &'life0 self,
            selector: Option<&'life1 EntitySelector>,
            _attributes: &'life2 [IriBuf],
        ) -> BoxFuture<'async_trait, Result<EntityPresence, RegistrationError>>
        where
            'life0: 'async_trait,
            'life1: 'async_trait,
            'life2: 'async_trait,
        {
            let description = selector.map_or_else(|| "matches:<none>".to_string(), |selector| format!("matches:{}", selector.type_));
            self.asked.lock().expect("probe log").push(description);
            Box::pin(async { Ok(self.matches) })
        }
    }

    fn info(value: serde_json::Value) -> RegistrationInfo {
        serde_json::from_value(value).expect("valid RegistrationInfo")
    }

    fn registration(id: &str, mode: &str, information: serde_json::Value) -> ContextSourceRegistration {
        serde_json::from_value(json!({
            "id": id,
            "type": "ContextSourceRegistration",
            "endpoint": "https://peer.example.org",
            "information": information,
            "mode": mode,
            "status": "ok",
            "createdAt": "2024-01-01T00:00:00Z",
            "modifiedAt": "2024-01-01T00:00:00Z"
        }))
        .expect("valid registration")
    }

    const VEHICLE: &str = "https://example.org/Vehicle";
    const SPEED: &str = "https://example.org/speed";
    const FUEL: &str = "https://example.org/fuel";
    const V1: &str = "urn:ngsi-ld:Vehicle:V1";

    fn exclusive_info(entity_id: &str, attributes: &[&str]) -> serde_json::Value {
        json!({"entities": [{"type": VEHICLE, "id": entity_id}], "propertyNames": attributes})
    }

    #[test]
    fn an_exclusive_registration_needs_an_entity_id_and_at_least_one_attribute() {
        let type_only = info(json!({"entities": [{"type": VEHICLE}], "propertyNames": [SPEED]}));
        let pattern = info(json!({"entities": [{"type": VEHICLE, "idPattern": "urn:.*"}], "propertyNames": [SPEED]}));
        let no_attributes = info(json!({"entities": [{"type": VEHICLE, "id": V1}]}));
        let attributes_only = info(json!({"propertyNames": [SPEED]}));
        for entry in [type_only, pattern, no_attributes, attributes_only] {
            assert!(matches!(
                validate_structure(ForwardingMode::Exclusive, &[entry], &[CsrOperation::FederationOps]),
                Err(RegistrationError::Validation { .. })
            ));
        }
        let valid = info(exclusive_info(V1, &[SPEED]));
        assert!(validate_structure(ForwardingMode::Exclusive, &[valid], &[CsrOperation::FederationOps]).is_ok());
    }

    #[test]
    fn an_auxiliary_registration_may_only_advertise_consumption_operations() {
        let entry = info(json!({"entities": [{"type": VEHICLE}]}));
        assert!(matches!(
            validate_structure(ForwardingMode::Auxiliary, slice::from_ref(&entry), &[CsrOperation::CreateEntity]),
            Err(RegistrationError::Validation { .. })
        ));
        assert!(matches!(
            validate_structure(ForwardingMode::Auxiliary, slice::from_ref(&entry), &[CsrOperation::FederationOps]),
            Err(RegistrationError::Validation { .. })
        ));
        assert!(validate_structure(ForwardingMode::Auxiliary, &[entry], &[CsrOperation::RetrieveEntity, CsrOperation::QueryEntity]).is_ok());
    }

    #[test]
    fn inclusive_and_redirect_registrations_have_no_structural_mode_rule() {
        let entry = info(json!({"entities": [{"type": VEHICLE}]}));
        assert!(validate_structure(ForwardingMode::Inclusive, slice::from_ref(&entry), &[CsrOperation::CreateEntity]).is_ok());
        assert!(validate_structure(ForwardingMode::Redirect, &[entry], &[CsrOperation::CreateEntity]).is_ok());
    }

    #[tokio::test]
    async fn an_exclusive_registration_over_an_existing_exclusive_or_redirect_footprint_is_a_conflict() {
        for existing_mode in ["exclusive", "redirect"] {
            let store = FixedStore(vec![registration(
                "urn:ngsi-ld:csr:existing",
                existing_mode,
                json!([exclusive_info(V1, &[SPEED])]),
            )]);
            let probe = FixedProbe::new(EntityPresence::Absent, EntityPresence::Absent);
            let result = check_conflicts(&store, &probe, ForwardingMode::Exclusive, &[info(exclusive_info(V1, &[SPEED]))], None).await;
            assert!(matches!(result, Err(RegistrationError::Conflict { .. })), "{existing_mode} must conflict");
        }
    }

    #[tokio::test]
    async fn a_disjoint_attribute_or_entity_footprint_does_not_conflict() {
        let store = FixedStore(vec![registration(
            "urn:ngsi-ld:csr:existing",
            "exclusive",
            json!([exclusive_info(V1, &[SPEED])]),
        )]);
        let probe = FixedProbe::new(EntityPresence::Absent, EntityPresence::Absent);
        assert!(
            check_conflicts(&store, &probe, ForwardingMode::Exclusive, &[info(exclusive_info(V1, &[FUEL]))], None)
                .await
                .is_ok()
        );
        assert!(
            check_conflicts(
                &store,
                &probe,
                ForwardingMode::Exclusive,
                &[info(exclusive_info("urn:ngsi-ld:Vehicle:V2", &[SPEED]))],
                None
            )
            .await
            .is_ok()
        );
    }

    #[tokio::test]
    async fn a_redirect_registering_the_whole_entity_overlaps_every_attribute() {
        let whole_entity = registration("urn:ngsi-ld:csr:redirect", "redirect", json!([{"entities": [{"type": VEHICLE, "id": V1}]}]));
        let store = FixedStore(vec![whole_entity]);
        let probe = FixedProbe::new(EntityPresence::Absent, EntityPresence::Absent);
        let result = check_conflicts(&store, &probe, ForwardingMode::Exclusive, &[info(exclusive_info(V1, &[FUEL]))], None).await;
        assert!(matches!(result, Err(RegistrationError::Conflict { .. })));
    }

    #[tokio::test]
    async fn an_id_pattern_registration_overlaps_the_ids_it_matches_only() {
        let pattern = registration(
            "urn:ngsi-ld:csr:pattern",
            "redirect",
            json!([{"entities": [{"type": VEHICLE, "idPattern": "urn:ngsi-ld:Vehicle:V.*"}], "propertyNames": [SPEED]}]),
        );
        let store = FixedStore(vec![pattern]);
        let probe = FixedProbe::new(EntityPresence::Absent, EntityPresence::Absent);
        assert!(matches!(
            check_conflicts(&store, &probe, ForwardingMode::Exclusive, &[info(exclusive_info(V1, &[SPEED]))], None).await,
            Err(RegistrationError::Conflict { .. })
        ));
        assert!(
            check_conflicts(
                &store,
                &probe,
                ForwardingMode::Exclusive,
                &[info(exclusive_info("urn:ngsi-ld:Car:C1", &[SPEED]))],
                None
            )
            .await
            .is_ok()
        );
    }

    #[tokio::test]
    async fn the_registration_being_updated_never_conflicts_with_itself() {
        let self_id: Uri = "urn:ngsi-ld:csr:self".parse().expect("valid URI");
        let store = FixedStore(vec![registration(self_id.as_str(), "exclusive", json!([exclusive_info(V1, &[SPEED])]))]);
        let probe = FixedProbe::new(EntityPresence::Absent, EntityPresence::Absent);
        assert!(
            check_conflicts(&store, &probe, ForwardingMode::Exclusive, &[info(exclusive_info(V1, &[SPEED]))], Some(&self_id))
                .await
                .is_ok()
        );
    }

    #[tokio::test]
    async fn an_exclusive_registration_over_a_local_entity_holding_the_attribute_is_a_conflict() {
        let store = FixedStore(Vec::new());
        let probe = FixedProbe::new(EntityPresence::Present, EntityPresence::Absent);
        let result = check_conflicts(&store, &probe, ForwardingMode::Exclusive, &[info(exclusive_info(V1, &[SPEED]))], None).await;
        assert!(matches!(result, Err(RegistrationError::Conflict { .. })));
        pretty_assertions::assert_eq!(probe.asked.lock().expect("probe log").as_slice(), [format!("holds:{V1}")]);
    }

    #[tokio::test]
    async fn a_redirect_registration_over_a_matching_local_entity_is_a_conflict() {
        let store = FixedStore(Vec::new());
        let probe = FixedProbe::new(EntityPresence::Absent, EntityPresence::Present);
        let by_type = info(json!({"entities": [{"type": VEHICLE}]}));
        assert!(matches!(
            check_conflicts(&store, &probe, ForwardingMode::Redirect, &[by_type], None).await,
            Err(RegistrationError::Conflict { .. })
        ));
        let attributes_only = info(json!({"propertyNames": [SPEED]}));
        assert!(matches!(
            check_conflicts(&store, &probe, ForwardingMode::Redirect, &[attributes_only], None).await,
            Err(RegistrationError::Conflict { .. })
        ));
        assert!(probe.asked.lock().expect("probe log").contains(&"matches:<none>".to_string()));
    }

    #[tokio::test]
    async fn a_redirect_registration_over_an_exclusive_footprint_is_a_conflict() {
        let store = FixedStore(vec![registration(
            "urn:ngsi-ld:csr:exclusive",
            "exclusive",
            json!([exclusive_info(V1, &[SPEED])]),
        )]);
        let probe = FixedProbe::new(EntityPresence::Absent, EntityPresence::Absent);
        let redirect = info(json!({"entities": [{"type": VEHICLE, "id": V1}]}));
        assert!(matches!(
            check_conflicts(&store, &probe, ForwardingMode::Redirect, &[redirect], None).await,
            Err(RegistrationError::Conflict { .. })
        ));
    }

    #[tokio::test]
    async fn inclusive_and_auxiliary_registrations_consult_neither_store_nor_entities() {
        let store = FixedStore(vec![registration(
            "urn:ngsi-ld:csr:existing",
            "exclusive",
            json!([exclusive_info(V1, &[SPEED])]),
        )]);
        let probe = FixedProbe::new(EntityPresence::Present, EntityPresence::Present);
        let entry = info(exclusive_info(V1, &[SPEED]));
        assert!(
            check_conflicts(&store, &probe, ForwardingMode::Inclusive, slice::from_ref(&entry), None)
                .await
                .is_ok()
        );
        assert!(check_conflicts(&store, &probe, ForwardingMode::Auxiliary, &[entry], None).await.is_ok());
        assert!(probe.asked.lock().expect("probe log").is_empty());
    }
}
