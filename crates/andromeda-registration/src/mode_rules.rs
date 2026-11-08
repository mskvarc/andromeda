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
    registration_conflict_reason::RegistrationConflictReason,
    registration_validation_reason::RegistrationValidationReason,
    store::RegistrationStore,
};
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, identifier::Identifier};
use andromeda_ngsi_ld_model::{
    entity_info::EntityInfo,
    registration::{ContextSourceRegistration, forwarding_mode::ForwardingMode, info::RegistrationInfo, operation::CsrOperation},
    registration_matching::{entity_specification::EntitySpecification, match_specification::RegistrationMatchSpecification},
};
use std::slice;

/// The operations clause 5.9.2.4 admits on an auxiliary registration.
const AUXILIARY_OPERATIONS: [CsrOperation; 3] = [CsrOperation::RetrieveOps, CsrOperation::RetrieveEntity, CsrOperation::QueryEntity];

/// The effective operation set of a registration that names no explicit subset.
///
/// Clause 4.20 makes `federationOps` the default, and Table 5.2.9-1 repeats it.
/// That group reaches well beyond the consumption operations clause 4.3.6.2
/// confines an auxiliary Context Source to, and clause 5.9.2.4 refuses an
/// auxiliary registration whose `operations` leave `retrieveOps`,
/// `retrieveEntity` and `queryEntity`. Handing the general default to an
/// auxiliary registration would therefore make every one of them unregistrable,
/// against clause 4.3.6.2's "Auxiliary Context Source Registrations are always
/// accepted as there can never be a conflict". The default narrows instead to
/// `retrieveOps`, the widest group the auxiliary restriction admits, and a
/// subset of `federationOps`, so nothing outside auxiliary mode moves.
pub(super) const fn default_operations(mode: ForwardingMode) -> [CsrOperation; 1] {
    match mode {
        ForwardingMode::Auxiliary => [CsrOperation::RetrieveOps],
        ForwardingMode::Inclusive | ForwardingMode::Exclusive | ForwardingMode::Redirect => [CsrOperation::FederationOps],
    }
}

/// The mode-dependent shape rules that need nothing beyond the registration.
pub(super) fn validate_structure(mode: ForwardingMode, information: &[RegistrationInfo], operations: &[CsrOperation]) -> Result<(), RegistrationError> {
    match mode {
        ForwardingMode::Exclusive => information.iter().try_for_each(validate_exclusive_entry),
        ForwardingMode::Auxiliary => {
            if operations.iter().all(|operation| AUXILIARY_OPERATIONS.contains(operation)) {
                Ok(())
            } else {
                Err(RegistrationError::Validation(RegistrationValidationReason::AuxiliaryOperationsRestricted))
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
        .ok_or_else(|| RegistrationError::Validation(RegistrationValidationReason::ExclusiveMissingEntities))?;
    if !selectors.iter().all(EntityInfo::names_one_entity) {
        return Err(RegistrationError::Validation(RegistrationValidationReason::ExclusiveEntityNotById));
    }
    if entry.attribute_names().next().is_none() {
        return Err(RegistrationError::Validation(RegistrationValidationReason::ExclusiveMissingAttributes));
    }
    Ok(())
}

/// The expanded attribute names a block declares, on both axes.
fn declared_attributes(entry: &RegistrationInfo) -> Vec<AttributeName> {
    entry.attribute_names().cloned().collect()
}

/// Whether an existing registration overlaps the footprint `(entity_id,
/// attributes)` (clause 5.12): a block covering the entity that declares one of
/// the attributes, or none at all, since a registration naming no attribute
/// registers the whole entity (clause 4.3.6.3).
fn registration_overlaps(existing: &ContextSourceRegistration, entity_id: &Identifier, attributes: &[AttributeName]) -> bool {
    let entity = EntitySpecification::of_entity(&[], entity_id);
    RegistrationMatchSpecification {
        entities: slice::from_ref(&entity),
        attributes,
    }
    .covers(existing)
}

/// Reject the footprint when another proxied registration already claims it.
async fn reject_overlapping_registrations(
    repo: &dyn RegistrationStore,
    self_id: Option<&Identifier>,
    entity_id: &Identifier,
    attributes: &[AttributeName],
    colliding_modes: &[ForwardingMode],
) -> Result<(), RegistrationError> {
    let candidates = repo.find_overlapping(Some(entity_id), attributes).await?;
    let collision = candidates.iter().find(|candidate| {
        self_id != Some(&candidate.id)
            && colliding_modes.contains(&candidate.access.mode.unwrap_or_default())
            && registration_overlaps(candidate, entity_id, attributes)
    });
    match collision {
        Some(existing) => Err(RegistrationError::Conflict(RegistrationConflictReason::RegistrationCoversEntity {
            registration: existing.id.clone(),   // clone: the error owns the borrowed candidate identifier.
            entity: Box::new(entity_id.clone()), // clone: the error owns the borrowed request identifier.
        })),
        None => Ok(()),
    }
}

/// Clause 5.9.2.4 exclusive rules, per entity the registration names.
async fn check_exclusive(
    repo: &dyn RegistrationStore,
    probe: &dyn RegisteredEntityProbe,
    self_id: Option<&Identifier>,
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
                return Err(RegistrationError::Conflict(RegistrationConflictReason::EntityHoldsCoveredAttribute {
                    entity: entity_id.clone(), // clone: the error owns the borrowed request identifier.
                }));
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
    self_id: Option<&Identifier>,
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
async fn reject_matching_entity(
    probe: &dyn RegisteredEntityProbe,
    selector: Option<&EntityInfo>,
    attributes: &[AttributeName],
) -> Result<(), RegistrationError> {
    match probe.any_entity_matching(selector, attributes).await? {
        EntityPresence::Present => Err(RegistrationError::Conflict(RegistrationConflictReason::LocalEntityMatchesRedirect)),
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
    self_id: Option<&Identifier>,
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
        entity_probe::EntityPresence,
        error::RegistrationError,
        fixed_probe::FixedProbe,
        fixed_store::{FixedStore, registration},
        mode_rules::{check_conflicts, default_operations, validate_structure},
    };
    use andromeda_ngsi_ld_lexicon::identifier::Identifier;
    use andromeda_ngsi_ld_model::registration::{forwarding_mode::ForwardingMode, info::RegistrationInfo, operation::CsrOperation};
    use serde_json::json;
    use std::slice;

    fn info(value: serde_json::Value) -> RegistrationInfo {
        serde_json::from_value(value).expect("valid RegistrationInfo")
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
                Err(RegistrationError::Validation(_))
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
            Err(RegistrationError::Validation(_))
        ));
        assert!(matches!(
            validate_structure(ForwardingMode::Auxiliary, slice::from_ref(&entry), &[CsrOperation::FederationOps]),
            Err(RegistrationError::Validation(_))
        ));
        assert!(validate_structure(ForwardingMode::Auxiliary, &[entry], &[CsrOperation::RetrieveEntity, CsrOperation::QueryEntity]).is_ok());
    }

    #[test]
    fn the_default_operation_set_of_an_auxiliary_registration_survives_its_own_admission_rule() {
        let entry = info(json!({"entities": [{"type": VEHICLE}]}));
        // Clause 4.3.6.2 promises an auxiliary registration is always accepted;
        // it can only keep that promise if its default set is one clause
        // 5.9.2.4 admits.
        assert_eq!(default_operations(ForwardingMode::Auxiliary), [CsrOperation::RetrieveOps]);
        assert!(validate_structure(ForwardingMode::Auxiliary, &[entry], &default_operations(ForwardingMode::Auxiliary)).is_ok());
    }

    #[test]
    fn every_other_mode_keeps_the_clause_4_20_default_operation_set() {
        for mode in [ForwardingMode::Inclusive, ForwardingMode::Exclusive, ForwardingMode::Redirect] {
            assert_eq!(default_operations(mode), [CsrOperation::FederationOps], "{mode} must default to federationOps");
        }
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
            let store = FixedStore::new(vec![registration(
                "urn:ngsi-ld:csr:existing",
                existing_mode,
                &json!([exclusive_info(V1, &[SPEED])]),
            )]);
            let probe = FixedProbe::new(EntityPresence::Absent, EntityPresence::Absent);
            let result = check_conflicts(&store, &probe, ForwardingMode::Exclusive, &[info(exclusive_info(V1, &[SPEED]))], None).await;
            assert!(matches!(result, Err(RegistrationError::Conflict(_))), "{existing_mode} must conflict");
        }
    }

    #[tokio::test]
    async fn a_disjoint_attribute_or_entity_footprint_does_not_conflict() {
        let store = FixedStore::new(vec![registration(
            "urn:ngsi-ld:csr:existing",
            "exclusive",
            &json!([exclusive_info(V1, &[SPEED])]),
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
        let whole_entity = registration("urn:ngsi-ld:csr:redirect", "redirect", &json!([{"entities": [{"type": VEHICLE, "id": V1}]}]));
        let store = FixedStore::new(vec![whole_entity]);
        let probe = FixedProbe::new(EntityPresence::Absent, EntityPresence::Absent);
        let result = check_conflicts(&store, &probe, ForwardingMode::Exclusive, &[info(exclusive_info(V1, &[FUEL]))], None).await;
        assert!(matches!(result, Err(RegistrationError::Conflict(_))));
    }

    #[tokio::test]
    async fn an_id_pattern_registration_overlaps_the_ids_it_matches_only() {
        let pattern = registration(
            "urn:ngsi-ld:csr:pattern",
            "redirect",
            &json!([{"entities": [{"type": VEHICLE, "idPattern": "urn:ngsi-ld:Vehicle:V.*"}], "propertyNames": [SPEED]}]),
        );
        let store = FixedStore::new(vec![pattern]);
        let probe = FixedProbe::new(EntityPresence::Absent, EntityPresence::Absent);
        assert!(matches!(
            check_conflicts(&store, &probe, ForwardingMode::Exclusive, &[info(exclusive_info(V1, &[SPEED]))], None).await,
            Err(RegistrationError::Conflict(_))
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

    /// Clause 5.2.8: the registered `id` decides, so a wide `idPattern` beside
    /// it does not widen the footprint the existing registration claims.
    #[tokio::test]
    async fn an_exclusive_footprint_over_a_registration_naming_id_and_pattern_overlaps_by_the_id_only() {
        let id_and_pattern = registration(
            "urn:ngsi-ld:csr:id-and-pattern",
            "redirect",
            &json!([{"entities": [{"type": VEHICLE, "id": V1, "idPattern": "urn:ngsi-ld:Vehicle:.*"}], "propertyNames": [SPEED]}]),
        );
        let store = FixedStore::new(vec![id_and_pattern]);
        let probe = FixedProbe::new(EntityPresence::Absent, EntityPresence::Absent);
        assert!(matches!(
            check_conflicts(&store, &probe, ForwardingMode::Exclusive, &[info(exclusive_info(V1, &[SPEED]))], None).await,
            Err(RegistrationError::Conflict(_))
        ));
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
    async fn the_registration_being_updated_never_conflicts_with_itself() {
        let self_id: Identifier = "urn:ngsi-ld:csr:self".parse().expect("valid URI");
        let store = FixedStore::new(vec![registration(self_id.as_str(), "exclusive", &json!([exclusive_info(V1, &[SPEED])]))]);
        let probe = FixedProbe::new(EntityPresence::Absent, EntityPresence::Absent);
        assert!(
            check_conflicts(&store, &probe, ForwardingMode::Exclusive, &[info(exclusive_info(V1, &[SPEED]))], Some(&self_id))
                .await
                .is_ok()
        );
    }

    #[tokio::test]
    async fn an_exclusive_registration_over_a_local_entity_holding_the_attribute_is_a_conflict() {
        let store = FixedStore::new(Vec::new());
        let probe = FixedProbe::new(EntityPresence::Present, EntityPresence::Absent);
        let result = check_conflicts(&store, &probe, ForwardingMode::Exclusive, &[info(exclusive_info(V1, &[SPEED]))], None).await;
        assert!(matches!(result, Err(RegistrationError::Conflict(_))));
        pretty_assertions::assert_eq!(probe.asked(), [format!("holds:{V1}")]);
    }

    #[tokio::test]
    async fn a_redirect_registration_over_a_matching_local_entity_is_a_conflict() {
        let store = FixedStore::new(Vec::new());
        let probe = FixedProbe::new(EntityPresence::Absent, EntityPresence::Present);
        let by_type = info(json!({"entities": [{"type": VEHICLE}]}));
        assert!(matches!(
            check_conflicts(&store, &probe, ForwardingMode::Redirect, &[by_type], None).await,
            Err(RegistrationError::Conflict(_))
        ));
        let attributes_only = info(json!({"propertyNames": [SPEED]}));
        assert!(matches!(
            check_conflicts(&store, &probe, ForwardingMode::Redirect, &[attributes_only], None).await,
            Err(RegistrationError::Conflict(_))
        ));
        assert!(probe.asked().contains(&"matches:<none>".to_string()));
    }

    #[tokio::test]
    async fn a_redirect_registration_over_an_exclusive_footprint_is_a_conflict() {
        let store = FixedStore::new(vec![registration(
            "urn:ngsi-ld:csr:exclusive",
            "exclusive",
            &json!([exclusive_info(V1, &[SPEED])]),
        )]);
        let probe = FixedProbe::new(EntityPresence::Absent, EntityPresence::Absent);
        let redirect = info(json!({"entities": [{"type": VEHICLE, "id": V1}]}));
        assert!(matches!(
            check_conflicts(&store, &probe, ForwardingMode::Redirect, &[redirect], None).await,
            Err(RegistrationError::Conflict(_))
        ));
    }

    #[tokio::test]
    async fn inclusive_and_auxiliary_registrations_consult_neither_store_nor_entities() {
        let store = FixedStore::new(vec![registration(
            "urn:ngsi-ld:csr:existing",
            "exclusive",
            &json!([exclusive_info(V1, &[SPEED])]),
        )]);
        let probe = FixedProbe::new(EntityPresence::Present, EntityPresence::Present);
        let entry = info(exclusive_info(V1, &[SPEED]));
        assert!(
            check_conflicts(&store, &probe, ForwardingMode::Inclusive, slice::from_ref(&entry), None)
                .await
                .is_ok()
        );
        assert!(check_conflicts(&store, &probe, ForwardingMode::Auxiliary, &[entry], None).await.is_ok());
        assert_eq!(probe.asked(), [] as [String; 0]);
    }
}
