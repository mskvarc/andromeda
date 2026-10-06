//! Batch merge-patch.
//!
//! Like batch update, a read-modify-write over the whole chunk; unlike it, the
//! fragment is applied with the Merge Entity semantics of clause 5.6.17
//! (behaviour in clause 5.5.12) rather than by updating and appending in turn.
//!
//! A chunk carries at most one instance of any entity, because the coordinator
//! gives each instance of a repeated identifier a round of its own. That is
//! what makes the stored entity "the entity resulting from merging all of the
//! entity instances, in the correct order" and puts every instance into the
//! Temporal Evolution of Entities (clause 5.5.11.5).

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
    error::EntityError,
    side_effect::SideEffectPlacement,
};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_ngsi_ld_entity::{
    entity_state::EntityState,
    history::written_observation::AddressedInstances,
    operation::selection::OverwritePolicy,
    system_instant::SystemInstant,
};
use andromeda_ngsi_ld_lexicon::identifier::Identifier;

/// Merging each entity of a chunk into the entity as an attempt read it
/// (clause 5.6.20.4 runs the merge of clause 5.6.17 per entity).
struct MergeRewrite<'a> {
    service: &'a EntityService,
    target: &'a ContextTarget,
}

impl ChunkRewrite for MergeRewrite<'_> {
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
        // clone: reference-count bumps; the merge shares every attribute it does not touch with the stored state
        let mut merged = EntityState::clone(previous);
        if let Err(error) = merged.merge(&entry.document, None) {
            return Err(Box::new((entry.id, EntityError::from(error))));
        }
        let write = self.service.chunk_fragment_update(
            self.target,
            entry.id.clone(), // clone: the entry is kept to rebuild the write if the store refuses it
            UpdatedStates {
                previous,
                current: merged,
                addressed: AddressedInstances::EntityFragment {
                    fragment: &entry.document,
                    overwrite: OverwritePolicy::Overwrite,
                },
            },
            now,
        )?;
        Ok(PlannedWrite { entry, write, alongside: None })
    }
}

impl EntityService {
    pub(super) async fn batch_merge_chunk(&self, placement: SideEffectPlacement<'_>, chunk: Vec<AdmittedBatchEntity>) -> Result<BatchResult, EntityError> {
        // Clause 5.6.20 is the batch form of Merge Entity, whose fragments carry
        // the NGSI-LD Null to arbitrary depth (clause 5.5.12).
        let prepared = prepared_chunk(chunk);
        let mut errors = Vec::new();
        let rewrite = MergeRewrite {
            service: self,
            target: placement.target(),
        };
        let outcome = self.commit_chunk_read_modify_write(placement, &rewrite, prepared).await?;
        errors.extend(outcome.errors);
        Ok(BatchResult {
            success: outcome.success,
            errors,
        })
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        admission_fixture::admitted_batch,
        batch::BatchResult,
        building_subscription_fixture::BUILDING_TYPE_IRI,
        compare_and_set_fixture::{ContendedService, Watchers, building_id, contended_service, request_building, term, tombstoned_terms},
        side_effect::SideEffectPlacement,
        write_kind::BatchMerge,
    };
    use andromeda_context_space::context_target::ContextTarget;
    use andromeda_jsonld::context_spec::ContextSpec;
    use andromeda_ngsi_ld_entity::entity_state::EntityState;
    use andromeda_ngsi_ld_lexicon::identifier::Identifier;
    use pretty_assertions::assert_eq;
    use serde_json::{Value, json};

    /// The named dataset the stored `speed` holds beside its default instance.
    const DATASET_A: &str = "urn:ngsi-ld:Dataset:A";

    /// One expanded Property instance holding `value`, in `dataset` when named.
    fn property_instance(value: i64, dataset: Option<&str>) -> Value {
        let mut instance = json!({
            "@type": ["https://uri.etsi.org/ngsi-ld/Property"],
            "https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": value}]
        });
        if let Some(dataset) = dataset {
            instance["https://uri.etsi.org/ngsi-ld/datasetId"] = json!([{"@id": dataset}]);
        }
        instance
    }

    /// A service whose store holds the Building `a`, whose `speed` holds a
    /// default instance and a [`DATASET_A`] instance, beside a `brand`.
    fn seeded(contended: &ContendedService) -> Identifier {
        let id = building_id("a");
        let mut node = serde_json::Map::new();
        node.insert("@id".to_owned(), json!(id.as_str()));
        node.insert("@type".to_owned(), json!([BUILDING_TYPE_IRI]));
        node.insert(term("speed"), json!([property_instance(1, None), property_instance(2, Some(DATASET_A))]));
        node.insert(term("brand"), json!([property_instance(3, None)]));
        let payload = serde_json::to_vec(&Value::Array(vec![Value::Object(node)])).expect("JSON");
        contended
            .store
            .seed(&id, EntityState::from_slice(&payload).expect("the fixture is a stored entity"));
        id
    }

    /// Merge the Building `id` carrying `speed` in one batch, inline.
    async fn batch_merge_speed(contended: &ContendedService, id: &Identifier, speed: &Value) -> BatchResult {
        let entities = vec![request_building(id, &json!({"speed": speed}))];
        let admitted = admitted_batch(&contended.service, BatchMerge, entities, &ContextSpec::core_only()).await;
        contended
            .service
            .batch_merge(SideEffectPlacement::Inline(&ContextTarget::Live), admitted)
            .await
            .expect("the batch completes")
    }

    /// The `datasetId` of every instance the stored `speed` holds, `None` for
    /// the default instance, or nothing when the attribute is gone.
    fn stored_speed_datasets(contended: &ContendedService, id: &Identifier) -> Vec<Option<String>> {
        let (state, _) = contended.store.stored(id).expect("the Building is stored");
        state
            .attributes()
            .get(term("speed").as_str())
            .map(|held| held.iter().map(|(dataset, _)| dataset.map(ToString::to_string)).collect())
            .unwrap_or_default()
    }

    /// Clause 5.6.20.4 runs the Merge Entity of clause 5.6.17, and clause
    /// 5.5.12 removes "the target member" a fragment sets to the NGSI-LD Null:
    /// every instance goes, the named one with the default one.
    #[tokio::test]
    async fn a_batch_merge_removes_every_instance_of_a_member_set_to_the_null() {
        let contended = contended_service(Watchers::None).await;
        let id = seeded(&contended);

        let result = batch_merge_speed(&contended, &id, &json!("urn:ngsi-ld:null")).await;

        assert_eq!(result.success, vec![id.clone()]);
        assert!(result.errors.is_empty(), "{:?}", result.errors);
        assert_eq!(
            stored_speed_datasets(&contended, &id),
            Vec::<Option<String>>::new(),
            "no instance of speed is left"
        );
        let writes = contended.store.submitted_for(&id);
        let landed = writes.last().expect("the merge landed one write");
        assert_eq!(tombstoned_terms(landed), vec!["speed".to_owned(), "speed".to_owned()], "both instances end");
    }

    /// Clause 5.6.17.4: a reified instance without a `datasetId` targets the
    /// default instance, so the NGSI-LD Null as its value deletes that one and
    /// leaves the named instance standing.
    #[tokio::test]
    async fn a_batch_merge_of_a_reified_null_deletes_only_the_default_instance() {
        let contended = contended_service(Watchers::None).await;
        let id = seeded(&contended);

        let result = batch_merge_speed(&contended, &id, &json!({"type": "Property", "value": "urn:ngsi-ld:null"})).await;

        assert_eq!(result.success, vec![id.clone()]);
        assert_eq!(stored_speed_datasets(&contended, &id), vec![Some(DATASET_A.to_owned())]);
    }
}
