//! Batch attribute update (clause 5.6.9).
//!
//! A read-modify-write over the whole chunk: the stored payloads are read
//! together, each request fragment is applied in memory, and the results go
//! back as one statement, each holding only while its entity is unchanged
//! since the read (see [`crate::batch::chunk_read_modify_write`]).
//!
//! Reading the stored payloads together is only sound because a chunk never
//! carries two instances of the same entity: the coordinator gives each instance of a
//! repeated identifier a round of its own, so the second instance reads what
//! the first committed. Clause 5.5.11.3 requires exactly that, one separate
//! notification per individual update.

use crate::{
    EntityService,
    batch::{
        BatchResult,
        admitted_batch::{AdmittedBatchEntity, prepared_chunk},
        chunk::UpdatedStates,
        chunk_read_modify_write::{ChunkRewrite, PlannedWrite},
        prepare::PreparedEntity,
    },
    entity_snapshot::EntitySnapshot,
    entity_validation_reason::EntityValidationReason,
    error::EntityError,
    side_effect::SideEffectPlacement,
};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_ngsi_ld_entity::{document::EntityDocument, entity_state::EntityState, operation::selection::OverwritePolicy};
use andromeda_ngsi_ld_lexicon::{identifier::Identifier, time::system_instant::SystemInstant};

/// Whether `fragment` names a member an append can add: an attribute, the
/// scope or the expiry. Types alone are not Attributes, so a fragment naming
/// none of these is a success that appends nothing.
fn names_a_member(fragment: &EntityDocument) -> bool {
    fragment.scope().is_some() || fragment.expires_at().is_some() || !fragment.attributes().is_empty()
}

/// Updating each entity of a chunk as an attempt read it (clause 5.6.9.4 runs
/// the behaviour of clause 5.6.3 per entity).
struct UpdateRewrite<'a> {
    service: &'a EntityService,
    target: &'a ContextTarget,
    overwrite: OverwritePolicy,
}

impl ChunkRewrite for UpdateRewrite<'_> {
    type Entry = PreparedEntity;
    type Reads = ();

    fn id(entry: &PreparedEntity) -> &Identifier {
        &entry.id
    }

    fn build(
        &self,
        entry: PreparedEntity,
        snapshot: Option<&EntitySnapshot>,
        _reads: &(),
        now: SystemInstant,
    ) -> Result<PlannedWrite<PreparedEntity>, Box<(Identifier, EntityError)>> {
        let Some(snapshot) = snapshot else {
            let error = EntityError::not_found(&entry.id);
            return Err(Box::new((entry.id, error)));
        };
        let previous = &snapshot.state;
        // clone: reference-count bumps; the update shares every attribute it does not touch with the stored state
        let mut updated = EntityState::clone(previous);

        // Clause 5.6.9.4 runs Append Attributes (clause 5.6.3) locally under
        // either policy: with overwrite an attribute the fragment names is
        // replaced whole, sub-attributes the fragment omits included.
        if let Err(error) = updated.append_attributes(&entry.document, self.overwrite) {
            return Err(Box::new((entry.id, EntityError::from(error))));
        }

        let write = self.service.chunk_update(
            self.target,
            entry.id.clone(), // clone: the entry is kept to rebuild the write if the store refuses it
            UpdatedStates { previous, current: updated },
            now,
        )?;
        // Clause 5.6.9.4 puts an Entity in S when it was "successfully updated
        // (Attributes appended)" and in E otherwise, never in both. Under
        // noOverwrite a fragment that leaves the state this attempt read
        // unchanged appended nothing (every instance, scope and expiry it names
        // is held), so the Entity is an error, making the batch a 207. A
        // fragment that appends one instance or scope is a success even if
        // others are skipped, as with the single Append Attributes operation.
        if write.is_none() && self.overwrite == OverwritePolicy::Preserve && names_a_member(&entry.document) {
            let error = EntityError::Validation(EntityValidationReason::NoOverwriteConflict);
            return Err(Box::new((entry.id, error)));
        }
        Ok(PlannedWrite { entry, write })
    }
}

impl EntityService {
    pub(super) async fn batch_update_chunk(
        &self,
        placement: SideEffectPlacement<'_>,
        chunk: Vec<AdmittedBatchEntity>,
        overwrite: OverwritePolicy,
        now: SystemInstant,
    ) -> Result<BatchResult, EntityError> {
        // Clause 5.5.4 excepts only the fragments "used in partial update and
        // merge operations" from its NGSI-LD Null prohibitions, and clause 5.2.1
        // makes every other placement of the sentinel `BadRequestData`. A batch
        // update is neither: clause 5.6.9.4's local phase names "the behaviour
        // defined by clause 5.6.3", Append Attributes, which carries no
        // NGSI-LD Null support of its own, and clause 5.6.9 does not carry the
        // "NGSI-LD Nulls should be supported by this operation" sentence that
        // clauses 5.6.2, 5.6.4 and 5.6.17 do. The sentinel therefore has no
        // deletion semantics to be given here, and storing it would put
        // "urn:ngsi-ld:null" in a value the broker later serves back.
        let prepared = prepared_chunk(chunk);
        let mut errors = Vec::new();
        let rewrite = UpdateRewrite {
            service: self,
            target: placement.target(),
            overwrite,
        };
        let outcome = self.commit_chunk_read_modify_write(placement, &rewrite, prepared, now).await?;
        errors.extend(outcome.errors);
        Ok(BatchResult {
            success: outcome.success,
            errors,
        })
    }
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::{
        admission_fixture::admitted_batch,
        batch::BatchResult,
        building_subscription_fixture::BUILDING_TYPE_IRI,
        compare_and_set_fixture::{ContendedService, Watchers, building_id, contended_service, request_building, term},
        entity_validation_reason::EntityValidationReason,
        error::EntityError,
        side_effect::SideEffectPlacement,
        write_kind::BatchUpdate,
    };
    use andromeda_context_space::context_target::ContextTarget;
    use andromeda_jsonld::context_spec::ContextSpec;
    use andromeda_ngsi_ld_entity::{entity_state::EntityState, instance::Instance, operation::selection::OverwritePolicy};
    use andromeda_ngsi_ld_lexicon::{identifier::Identifier, time::system_instant::SystemInstant};
    use andromeda_ngsi_ld_model::entity::Entity;
    use serde_json::{Value, json};

    /// One expanded stored Property instance holding `value`, with
    /// `sub_attributes`, each carrying the clause 4.8 `createdAt` and
    /// `modifiedAt` every stored instance has.
    fn building_property(value: i64, sub_attributes: &[(&str, i64)]) -> Value {
        let stamp = json!([{"@type": "https://uri.etsi.org/ngsi-ld/DateTime", "@value": "2024-06-01T10:00:00.000000Z"}]);
        let mut instance = serde_json::Map::new();
        instance.insert("@type".to_owned(), json!(["https://uri.etsi.org/ngsi-ld/Property"]));
        instance.insert("https://uri.etsi.org/ngsi-ld/hasValue".to_owned(), json!([{"@value": value}]));
        instance.insert("https://uri.etsi.org/ngsi-ld/createdAt".to_owned(), stamp.clone()); // clone: both stamps hold the one instant
        instance.insert("https://uri.etsi.org/ngsi-ld/modifiedAt".to_owned(), stamp);
        for (name, value) in sub_attributes {
            instance.insert(term(name), building_property(*value, &[]));
        }
        json!([Value::Object(instance)])
    }

    /// A service whose store holds the Building `a`, its `name` carrying the
    /// sub-attribute `accuracy`.
    fn seeded(contended: &ContendedService) -> Identifier {
        let id = building_id("a");
        let node = json!([{
            "@id": id.as_str(),
            "@type": [BUILDING_TYPE_IRI],
            term("name"): building_property(1, &[("accuracy", 5)]),
        }]);
        let payload = serde_json::to_vec(&node).expect("JSON");
        contended
            .store
            .seed(&id, EntityState::from_slice(&payload).expect("the fixture is a stored entity"));
        id
    }

    /// Update `entities` in one batch under `overwrite`, inline.
    async fn batch_update(contended: &ContendedService, entities: Vec<Entity>, overwrite: OverwritePolicy) -> BatchResult {
        let admitted = admitted_batch(&contended.service, BatchUpdate, entities, &ContextSpec::core_only()).await;
        contended
            .service
            .batch_update(SideEffectPlacement::Inline(&ContextTarget::Live), admitted, overwrite)
            .await
            .expect("the batch completes")
    }

    /// The default instance of `attribute` in `state`.
    fn default_instance<'a>(state: &'a EntityState, attribute: &str) -> &'a Instance {
        let (_, instance) = state
            .attributes()
            .get(term(attribute).as_str())
            .and_then(|held| held.default_instance())
            .expect("the attribute holds a default instance");
        instance
    }

    /// Whether the stored `name` of the Building `id` carries `accuracy`.
    fn stored_name_keeps_accuracy(contended: &ContendedService, id: &Identifier) -> bool {
        let (state, _) = contended.store.stored(id).expect("the Building is stored");
        default_instance(&state, "name").sub_attributes().contains(term("accuracy").as_str())
    }

    /// The `modifiedAt` the `name` of every write submitted for `id` carries.
    fn written_name_modified_at(contended: &ContendedService, id: &Identifier) -> Vec<Option<SystemInstant>> {
        contended
            .store
            .submitted_for(id)
            .iter()
            .map(|write| default_instance(&write.entity.record().expect("the write stores a state").state, "name").modified_at())
            .collect()
    }

    /// Clause 5.6.9.4 runs Append Attributes (clause 5.6.3) locally, and with
    /// overwrite allowed "the existing default Attribute in the target Entity
    /// shall be replaced by the new one supplied": a sub-attribute the
    /// fragment omits does not survive.
    #[tokio::test]
    async fn a_default_batch_update_replaces_an_attribute_whole_dropping_an_omitted_sub_attribute() {
        let contended = contended_service(Watchers::None).await;
        let id = seeded(&contended);

        let result = batch_update(
            &contended,
            vec![request_building(&id, &json!({"name": {"type": "Property", "value": 2}}))],
            OverwritePolicy::Overwrite,
        )
        .await;

        pretty_assertions::assert_eq!(result.success, vec![id.clone()]);
        assert!(result.errors.is_empty(), "{:?}", result.errors);
        assert!(!stored_name_keeps_accuracy(&contended, &id));
    }

    /// Clause 5.6.3.4 under `noOverwrite`: "the existing default Attribute in
    /// the target Entity shall be left untouched", sub-attributes included.
    #[tokio::test]
    async fn a_no_overwrite_batch_update_leaves_an_attribute_and_its_sub_attribute_untouched() {
        let contended = contended_service(Watchers::None).await;
        let id = seeded(&contended);

        batch_update(
            &contended,
            vec![request_building(&id, &json!({"name": {"type": "Property", "value": 2}}))],
            OverwritePolicy::Preserve,
        )
        .await;

        assert!(stored_name_keeps_accuracy(&contended, &id));
    }

    /// Whether `result` reports `id` only in E, as a `noOverwrite` conflict.
    fn reported_only_as_a_conflict(result: &BatchResult, id: &Identifier) -> bool {
        !result.success.contains(id)
            && matches!(
                result.errors.as_slice(),
                [(failed, EntityError::Validation(EntityValidationReason::NoOverwriteConflict))] if failed == id
            )
    }

    /// Clause 5.6.9.4 puts an Entity in S when "successfully updated
    /// (Attributes appended)" and in E when its update failed, so a
    /// `noOverwrite` update naming only held attributes appends nothing and
    /// reports the Entity once, as an error.
    #[tokio::test]
    async fn a_no_overwrite_update_of_held_attributes_reports_the_entity_only_as_an_error() {
        let contended = contended_service(Watchers::None).await;
        let id = seeded(&contended);

        let result = batch_update(
            &contended,
            vec![request_building(&id, &json!({"name": {"type": "Property", "value": 2}}))],
            OverwritePolicy::Preserve,
        )
        .await;

        assert!(reported_only_as_a_conflict(&result, &id), "{:?} {:?}", result.success, result.errors);
    }

    /// Clause 5.6.3.4 appends an instance whose `datasetId` the Attribute does
    /// not hold, so naming a held Attribute does not by itself append nothing.
    #[tokio::test]
    async fn a_no_overwrite_update_appending_a_new_dataset_instance_of_a_held_attribute_is_a_success() {
        let contended = contended_service(Watchers::None).await;
        let id = seeded(&contended);

        let result = batch_update(
            &contended,
            vec![request_building(
                &id,
                &json!({"name": {"type": "Property", "value": 2, "datasetId": "urn:ngsi-ld:Dataset:b"}}),
            )],
            OverwritePolicy::Preserve,
        )
        .await;

        pretty_assertions::assert_eq!(result.success, vec![id.clone()]);
        assert!(result.errors.is_empty(), "{:?}", result.errors);
    }

    /// Clause 5.6.3.4 without overwrite appends "the Scopes in the Fragment
    /// that are not part of the value of scope of the target Entity": a new
    /// Scope beside a held one is appended, a held Scope alone is not.
    #[tokio::test]
    async fn a_no_overwrite_scope_is_a_success_only_when_it_holds_a_scope_the_entity_lacks() {
        let contended = contended_service(Watchers::None).await;
        let id = seeded(&contended);
        let update = async |scope: Value| batch_update(&contended, vec![request_building(&id, &json!({"scope": scope}))], OverwritePolicy::Preserve).await;

        pretty_assertions::assert_eq!(update(json!("/Berlin")).await.success, vec![id.clone()]);
        pretty_assertions::assert_eq!(update(json!(["/Berlin", "/Madrid"])).await.success, vec![id.clone()]);
        let held = update(json!(["/Madrid", "/Berlin"])).await;

        assert!(reported_only_as_a_conflict(&held, &id), "{:?} {:?}", held.success, held.errors);
    }

    /// Clause 5.5.11.0: instances of one entity are processed in array order
    /// "as if they had all arrived in separate requests", yet "All Entities and
    /// Attributes in the batch will get the same modifiedAt timestamp". The
    /// later instance's `name` replaces the earlier one's, each instance is its
    /// own write, and every write, the other entity's included, carries one
    /// `modifiedAt`.
    #[tokio::test]
    async fn repeated_instances_of_one_entity_end_with_the_later_values_and_share_one_modified_at() {
        let contended = contended_service(Watchers::None).await;
        let id = seeded(&contended);
        let other = building_id("b");
        contended
            .store
            .seed(&other, EntityState::clone(&contended.store.stored(&id).expect("the Building is stored").0));

        let result = batch_update(
            &contended,
            vec![
                request_building(
                    &id,
                    &json!({"name": {"type": "Property", "value": 2, "accuracy": {"type": "Property", "value": 6}}, "first": {"type": "Property", "value": 1}}),
                ),
                request_building(&other, &json!({"name": {"type": "Property", "value": 2}})),
                request_building(
                    &id,
                    &json!({"name": {"type": "Property", "value": 3}, "second": {"type": "Property", "value": 1}}),
                ),
            ],
            OverwritePolicy::Overwrite,
        )
        .await;

        pretty_assertions::assert_eq!(result.success.len(), 3, "{:?}", result.errors);
        let (state, _) = contended.store.stored(&id).expect("the Building is stored");
        assert!(
            !default_instance(&state, "name").sub_attributes().contains(term("accuracy").as_str()),
            "the later name replaced the earlier one"
        );
        assert!(state.attributes().contains(term("first").as_str()) && state.attributes().contains(term("second").as_str()));
        let stamps = written_name_modified_at(&contended, &id);
        pretty_assertions::assert_eq!(stamps.len(), 2, "one write per instance");
        assert!(stamps[0].is_some(), "{stamps:?}");
        pretty_assertions::assert_eq!(stamps[0], stamps[1]);
        pretty_assertions::assert_eq!(written_name_modified_at(&contended, &other), vec![stamps[0]]);
    }
}
