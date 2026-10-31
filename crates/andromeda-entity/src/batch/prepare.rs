//! Expanding and screening the entities of a batch that name one context.
//!
//! The entities prepared together all name one context (the batch groups them
//! by it before they reach here), so the context is processed once and every
//! entity is expanded against it by the typed walk. A large batch spreads the
//! walk across the blocking pool, because expanding a document whose context is
//! already processed is CPU-bound. An entity using something the walk does not
//! model is expanded by the JSON-LD processor instead, on the calling task.
//! Every entity carries its position so per-entity results line up with the
//! caller's order whichever way it was expanded.

use crate::{
    EntityService,
    batch::{
        BatchMember,
        execution::{BatchExecution, decide_batch_execution},
    },
    compact_geometry_admission::admit_compact_geometries,
    entity_invariant::EntityInvariant,
    error::EntityError,
    port::blocking_executor::BlockingTask,
    write_admission::{BatchWrite, admit_written_entity},
};
use andromeda_jsonld::{context_spec::ContextSpec, engine::expansion_context::ExpansionContext};
use andromeda_ngsi_ld_entity::{document::EntityDocument, entity_state::EntityState};
use andromeda_ngsi_ld_expansion::{entity_expansion::EntityExpansion, typed_expander::TypedExpander, typed_expansion::TypedExpansion};
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_ngsi_ld_model::entity::Entity;
use andromeda_ngsi_ld_representation::payload_budget::EntityPayloadBudget;
use std::{
    mem::{replace, take},
    sync::{Arc, Mutex},
};

/// One entity of the local phase: the write's identifier and the document its
/// preparation expanded and screened.
pub(super) struct PreparedEntity {
    /// Entity identifier.
    pub id: Identifier,
    /// The expanded, screened document the write applies or stores.
    pub document: EntityDocument,
}

impl PreparedEntity {
    /// The entity the document creates or replaces whole (clauses 5.6.1 and
    /// 5.6.18), moved out of the document rather than copied.
    ///
    /// # Errors
    ///
    /// Answers the entity's identifier with the refusal of a document no
    /// entity can hold, boxed because a failure is the rare outcome.
    pub(super) fn into_state(self) -> Result<(Identifier, EntityState), Box<(Identifier, EntityError)>> {
        let PreparedEntity { id, document } = self;
        match EntityState::try_from(document) {
            Ok(state) => Ok((id, state)),
            Err(refusal) => Err(Box::new((id, EntityError::from(refusal)))),
        }
    }
}

/// One entity waiting for its preparation, at its position in the batch.
pub(super) struct PendingEntity<M> {
    /// The entity's position in the batch.
    pub(super) idx: usize,
    /// The entity as submitted.
    pub(super) entity: Entity<M>,
}

/// What preparing one entity produced: the entity handed back with its
/// expanded document, or the failure against its identifier.
///
/// The failure is boxed so a prepared slot is not sized for the much larger
/// broker error.
pub(super) type PreparationOutcome<M> = Result<(Entity<M>, EntityDocument), Box<(Identifier, EntityError)>>;

/// What the typed walk made of one entity.
///
/// Both outcomes are boxed: they are collected per task and moved between
/// threads, and an entity with its document is large.
enum WalkOutcome<M> {
    /// The walk expanded and screened the entity, or refused it.
    Done {
        /// The entity's position in the batch.
        idx: usize,
        /// The prepared entity or its failure.
        result: Box<PreparationOutcome<M>>,
    },
    /// The entity uses something the walk does not model; the JSON-LD
    /// processor expands it.
    Declined(Box<PendingEntity<M>>),
}

/// The expansion and screening every entity of one batch write goes through.
#[derive(Clone, Copy)]
pub(super) struct Screening {
    /// The payload budget the expanded document is counted against.
    pub(super) budget: EntityPayloadBudget,
    /// The batch operation, which decides both the concise rewrite before
    /// expansion and what the write admits of the NGSI-LD Null.
    pub(super) write: BatchWrite,
}

/// One share of a batch's typed walk, run on a blocking worker.
struct WalkTask<M> {
    /// The entities this task walks.
    entries: Vec<PendingEntity<M>>,
    /// The processed context they are expanded against.
    context: Arc<ExpansionContext>,
    /// What every expanded entity is screened under.
    screening: Screening,
    /// Where every task leaves what it made of its entities.
    outputs: Arc<Mutex<Vec<WalkOutcome<M>>>>,
}

impl<M: BatchMember> BlockingTask for WalkTask<M> {
    fn run(self: Box<Self>) -> Result<(), EntityError> {
        let WalkTask {
            entries,
            context,
            screening,
            outputs,
        } = *self;
        let walked = entries.into_iter().map(|entry| walk(entry, &context, screening)).collect::<Vec<_>>();
        outputs
            .lock()
            .map_err(|_poisoned| EntityError::from(EntityInvariant::BatchOutputPoisoned))?
            .extend(walked);
        Ok(())
    }
}

/// Expand one entity by the typed walk and screen what it expanded to.
///
/// The concise rewrite is the batch operation's own: a batch merge runs the
/// Merge Entity of clause 5.6.17 per entity (clause 5.6.20.4), so it keeps a
/// first-level NGSI-LD Null member as the deletion of that member, which every
/// other batch write reads as a concise value.
fn walk<M: BatchMember>(entry: PendingEntity<M>, context: &ExpansionContext, screening: Screening) -> WalkOutcome<M> {
    let PendingEntity { idx, entity } = entry;
    // Clause 6.3.2: a geometry past the budget is refused before the walk
    // copies it into the written document.
    if let Err(exceeded) = admit_compact_geometries(screening.budget.geometry_positions, entity.attribute_members()) {
        return WalkOutcome::Done {
            idx,
            result: Box::new(Err(Box::new((entity.id, EntityError::from(exceeded))))),
        };
    }
    match TypedExpander::new(context)
        .bounded_by(screening.budget.json_values)
        .entity(&entity, screening.write.normalization())
    {
        Ok(TypedExpansion::Expanded(document)) => WalkOutcome::Done {
            idx,
            result: Box::new(screened(entity, document, screening)),
        },
        Ok(TypedExpansion::Declined(_)) => WalkOutcome::Declined(Box::new(PendingEntity { idx, entity })),
        Err(failure) => WalkOutcome::Done {
            idx,
            result: Box::new(Err(Box::new((entity.id, EntityError::from(failure))))),
        },
    }
}

/// Screen an expanded entity under the payload budget and the clause 5.5.4
/// rules of its write.
///
/// Clauses 5.6.7.4, 5.6.8.4, 5.6.9.4 and 5.6.20.4 each open with "execute the
/// behaviour defined in clause 5.5.4 on JSON-LD validation". The failure
/// attributes itself to this one entity, so the multi-status answer of clause
/// 5.6.7 reports it without failing its siblings.
fn screened<M>(entity: Entity<M>, document: EntityDocument, screening: Screening) -> PreparationOutcome<M> {
    match admit_written_entity(screening.budget, &document, screening.write.null_sentinel_admission()) {
        Ok(()) => Ok((entity, document)),
        Err(error) => Err(Box::new((entity.id, error))),
    }
}

impl EntityService {
    pub(super) fn batch_execution(&self, len: usize) -> BatchExecution {
        decide_batch_execution(len, self.batch_execution.prepare_blocking_threshold, self.batch_execution.chunk_concurrency)
    }

    /// The expansion and screening the batch operation `write` puts every
    /// entity through.
    pub(super) const fn screening(&self, write: BatchWrite) -> Screening {
        Screening {
            budget: self.payload_budget,
            write,
        }
    }

    /// Expand and screen the `entries` naming `context`, answering each with
    /// its position.
    pub(super) async fn prepare_group<M: BatchMember>(
        &self,
        entries: Vec<PendingEntity<M>>,
        context: &ContextSpec,
        execution: BatchExecution,
        screening: Screening,
    ) -> Vec<(usize, PreparationOutcome<M>)> {
        let (processed, entries, mut prepared) = self.process_group_context(entries, context).await;
        let Some(processed) = processed else {
            return prepared;
        };
        let mut declined = Vec::new();
        for outcome in self.run_walk(entries, &processed, execution, screening).await {
            match outcome {
                WalkOutcome::Done { idx, result } => prepared.push((idx, *result)),
                WalkOutcome::Declined(entry) => declined.push(*entry),
            }
        }
        prepared.extend(self.expand_declined(declined, processed, screening).await);
        prepared
    }

    /// The processed context of a group, the entries still to expand against
    /// it, and the failures of those that could not be.
    ///
    /// A context that does not process is processed again for the next entity
    /// rather than reported once for the group, so each entity carries the
    /// failure of its own expansion.
    async fn process_group_context<M>(
        &self,
        entries: Vec<PendingEntity<M>>,
        context: &ContextSpec,
    ) -> (Option<Arc<ExpansionContext>>, Vec<PendingEntity<M>>, Vec<(usize, PreparationOutcome<M>)>) {
        let mut failures = Vec::new();
        let mut entries = entries.into_iter();
        while let Some(entry) = entries.next() {
            match self.engine.expansion_context(context).await {
                Ok(processed) => return (Some(processed), [entry].into_iter().chain(entries).collect(), failures),
                Err(error) => failures.push((entry.idx, Err(Box::new((entry.entity.id, EntityError::from(error)))))),
            }
        }
        (None, Vec::new(), failures)
    }

    /// Walk `entries` on the calling task, or spread across the blocking pool
    /// when `execution` says so.
    async fn run_walk<M: BatchMember>(
        &self,
        entries: Vec<PendingEntity<M>>,
        context: &Arc<ExpansionContext>,
        execution: BatchExecution,
        screening: Screening,
    ) -> Vec<WalkOutcome<M>> {
        match execution {
            BatchExecution::Inline => entries.into_iter().map(|entry| walk(entry, context, screening)).collect(),
            BatchExecution::Offload { concurrency } => {
                let chunk_size = self.batch_execution.chunk_size.max(1);
                let outputs = Arc::new(Mutex::new(Vec::with_capacity(entries.len())));
                let mut owners = Vec::with_capacity(entries.len());
                let mut tasks: Vec<Box<dyn BlockingTask>> = Vec::new();
                let mut chunk: Vec<PendingEntity<M>> = Vec::with_capacity(chunk_size);
                let walk_task = |entries: Vec<PendingEntity<M>>| -> Box<dyn BlockingTask> {
                    Box::new(WalkTask {
                        entries,
                        context: Arc::clone(context), // clone: every task shares the processed context.
                        screening,
                        outputs: Arc::clone(&outputs), // clone: every task writes to the same result collection.
                    })
                };
                for entry in entries {
                    owners.push((entry.idx, entry.entity.id.clone())); // clone: failure attribution survives moving the entity into a task.
                    chunk.push(entry);
                    if chunk.len() == chunk_size {
                        tasks.push(walk_task(replace(&mut chunk, Vec::with_capacity(chunk_size))));
                    }
                }
                if !chunk.is_empty() {
                    tasks.push(walk_task(chunk));
                }
                if let Err(error) = self.blocking_executor.execute(tasks, concurrency).await {
                    tracing::error!(%error, "the batch preparation executor failed");
                    // Clause 5.2.17 requires a real entity URI for each batch error, so
                    // the failure is attributed to every entity the executor owned.
                    return owners
                        .into_iter()
                        .map(|(idx, id)| WalkOutcome::Done {
                            idx,
                            result: Box::new(Err(Box::new((id, EntityInvariant::BatchPreparationFailed.into())))),
                        })
                        .collect();
                }
                match outputs.lock() {
                    Ok(mut outputs) => take(&mut *outputs),
                    Err(poisoned) => take(&mut *poisoned.into_inner()),
                }
            }
        }
    }

    /// Expand the entities the typed walk declined with the JSON-LD processor,
    /// starting from the group's processed context, under the same concise
    /// rewrite the walk applies, and screen them.
    async fn expand_declined<M: BatchMember>(
        &self,
        declined: Vec<PendingEntity<M>>,
        context: Arc<ExpansionContext>,
        screening: Screening,
    ) -> Vec<(usize, PreparationOutcome<M>)> {
        let expansion = EntityExpansion::from_context(&self.engine, context).bounded_by(screening.budget.json_values);
        let mut prepared = Vec::with_capacity(declined.len());
        for PendingEntity { idx, entity } in declined {
            let result = match expansion.entity(&entity, screening.write.normalization()).await {
                Ok(document) => screened(entity, document, screening),
                Err(error) => Err(Box::new((entity.id, EntityError::from(error)))),
            };
            prepared.push((idx, result));
        }
        prepared
    }
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    clippy::panic,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::{
        EntityService,
        batch::{
            UpsertPolicy,
            execution::BatchExecution,
            prepare::{PendingEntity, PreparationOutcome},
        },
        notification_recording_harness::recording_service,
        write_admission::BatchWrite,
    };
    use andromeda_json_literal::JsonLiteral;
    use andromeda_jsonld::context_spec::ContextSpec;
    use andromeda_ngsi_ld_entity::document::{EntityDocument, attribute_member::AttributeMember, written_value::ValueSlot};
    use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
    use andromeda_ngsi_ld_model::attribute::instance::AttributeValue;
    use serde_json::{Value, json};

    fn pending(idx: usize, written: Value) -> PendingEntity<AttributeValue> {
        PendingEntity {
            idx,
            entity: serde_json::from_value(written).expect("the fixture is a valid entity"),
        }
    }

    fn vehicle(idx: usize) -> PendingEntity<AttributeValue> {
        pending(
            idx,
            json!({"id": format!("urn:ngsi-ld:Vehicle:{idx}"), "type": "Vehicle", "speed": {"type": "Property", "value": idx}}),
        )
    }

    /// The expanded attribute names of a prepared entity.
    fn attribute_names(preparation: &PreparationOutcome<AttributeValue>) -> Vec<String> {
        let (_, document) = preparation.as_ref().expect("the entity is prepared");
        document.attributes().keys().map(AttributeName::to_string).collect()
    }

    /// Every entity is answered at its own position, with the entity handed
    /// back beside its expanded document.
    #[tokio::test]
    async fn every_entity_is_answered_at_its_own_position() {
        let (service, _recorded) = recording_service().await;
        let entries = (0..5).map(vehicle).collect();

        let mut prepared = service
            .prepare_group(
                entries,
                &ContextSpec::core_only(),
                BatchExecution::Inline,
                service.screening(BatchWrite::Create),
            )
            .await;
        prepared.sort_by_key(|(idx, _)| *idx);

        pretty_assertions::assert_eq!(prepared.iter().map(|(idx, _)| *idx).collect::<Vec<_>>(), vec![0, 1, 2, 3, 4]);
        let ids: Vec<String> = prepared
            .iter()
            .map(|(_, preparation)| preparation.as_ref().map(|(entity, _)| entity.id.to_string()).expect("prepared"))
            .collect();
        pretty_assertions::assert_eq!(ids[2], "urn:ngsi-ld:Vehicle:2");
        pretty_assertions::assert_eq!(attribute_names(&prepared[0].1), vec!["https://uri.etsi.org/ngsi-ld/default-context/speed"]);
    }

    /// A type-scoped context is something the typed walk does not model: the
    /// JSON-LD processor expands the entity, under the scoped definition.
    #[tokio::test]
    async fn an_entity_the_walk_declines_is_expanded_by_the_processor() {
        let (service, _recorded) = recording_service().await;
        let context = ContextSpec::from_body(
            JsonLiteral::admit(
                &json!({
                    "Vehicle": {"@id": "https://example.org/Vehicle", "@context": {"speed": "https://typed.example.org/speed"}}
                })
                .to_string(),
            )
            .expect("an admissible literal"),
        );

        let prepared = service
            .prepare_group(vec![vehicle(0)], &context, BatchExecution::Inline, service.screening(BatchWrite::Create))
            .await;

        let [(0, preparation)] = prepared.as_slice() else {
            panic!("exactly the one entity is answered: {}", prepared.len());
        };
        pretty_assertions::assert_eq!(attribute_names(preparation), vec!["https://typed.example.org/speed"]);
    }

    /// The screening of the write runs on every expanded entity: a create
    /// refuses the NGSI-LD Null, and the refusal names the entity.
    #[tokio::test]
    async fn the_screening_refuses_an_entity_against_its_own_identifier() {
        let (service, _recorded) = recording_service().await;
        let entries = vec![
            pending(
                0,
                json!({"id": "urn:ngsi-ld:Vehicle:0", "type": "Vehicle", "speed": {"type": "Property", "value": "urn:ngsi-ld:null"}}),
            ),
            vehicle(1),
        ];

        let mut prepared = service
            .prepare_group(
                entries,
                &ContextSpec::core_only(),
                BatchExecution::Inline,
                service.screening(BatchWrite::Create),
            )
            .await;
        prepared.sort_by_key(|(idx, _)| *idx);

        let Err(failure) = &prepared[0].1 else {
            panic!("the Null is refused");
        };
        pretty_assertions::assert_eq!(failure.0.as_str(), "urn:ngsi-ld:Vehicle:0");
        assert!(failure.1.is_validation(), "{}", failure.1);
        assert!(prepared[1].1.is_ok());
    }

    /// The core context, which the typed walk models, and a type-scoped one,
    /// which it declines and the JSON-LD processor expands.
    fn both_expansion_paths() -> [(&'static str, ContextSpec); 2] {
        [
            ("typed walk", ContextSpec::core_only()),
            (
                "processor",
                ContextSpec::from_body(
                    JsonLiteral::admit(
                        &json!({
                            "Vehicle": {"@id": "https://example.org/Vehicle", "@context": {"brand": "https://typed.example.org/brand"}}
                        })
                        .to_string(),
                    )
                    .expect("an admissible literal"),
                ),
            ),
        ]
    }

    /// The document `write` prepares for a Vehicle whose `speed` is written as
    /// the bare NGSI-LD Null, under `context`.
    async fn prepared_null_speed(service: &EntityService, write: BatchWrite, context: &ContextSpec) -> EntityDocument {
        let entry = pending(0, json!({"id": "urn:ngsi-ld:Vehicle:0", "type": "Vehicle", "speed": "urn:ngsi-ld:null"}));
        let mut prepared = service
            .prepare_group(vec![entry], context, BatchExecution::Inline, service.screening(write))
            .await;
        let Some((0, Ok((_, document)))) = prepared.pop() else {
            panic!("the entity is prepared");
        };
        assert!(prepared.is_empty(), "one entity was submitted");
        document
    }

    const SPEED_IRI: &str = "https://uri.etsi.org/ngsi-ld/default-context/speed";

    /// Clause 5.6.20.4 runs the Merge Entity of clause 5.6.17 per entity, and
    /// clause 5.5.12 removes "the target member" a fragment sets to the
    /// NGSI-LD Null: the bare Null is the deletion of the whole member, on the
    /// typed walk and on the processor fallback alike.
    #[tokio::test]
    async fn a_batch_merge_reads_a_first_level_null_member_as_the_deletion_of_the_member_on_both_paths() {
        let (service, _recorded) = recording_service().await;
        for (path, context) in both_expansion_paths() {
            let document = prepared_null_speed(&service, BatchWrite::Merge, &context).await;
            pretty_assertions::assert_eq!(document.attributes().get(SPEED_IRI), Some(&AttributeMember::Remove), "{path}");
        }
    }

    /// Clause 5.6.8.4 runs the update of clause 5.6.2 in update mode, which
    /// reads the same member as a concise Property whose value is the Null:
    /// the deletion of the default instance alone.
    #[tokio::test]
    async fn a_batch_upsert_in_update_mode_reads_a_first_level_null_as_a_default_instance_deletion() {
        let (service, _recorded) = recording_service().await;
        for (path, context) in both_expansion_paths() {
            let document = prepared_null_speed(&service, BatchWrite::Upsert(UpsertPolicy::Update), &context).await;
            let Some(AttributeMember::Instances(instances)) = document.attributes().get(SPEED_IRI) else {
                panic!("{path}: one written instance");
            };
            assert!(
                matches!(instances.as_slice(), [instance] if matches!(instance.value(), ValueSlot::Remove(_)) && instance.dataset_id().is_none()),
                "{path}: {instances:?}"
            );
        }
    }
}
