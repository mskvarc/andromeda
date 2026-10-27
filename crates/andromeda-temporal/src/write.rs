//! Temporal mutation policy.

use crate::{
    error::TemporalError,
    port::{mutator::TemporalDeleteScope, read_scope::ScopeReach},
    service::{CreateOrUpdateResult, TemporalService, iri_short_name},
    temporal_not_found_reason::TemporalNotFoundReason,
    temporal_validation_reason::TemporalValidationReason,
    written_evolution::WrittenEvolution,
};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_jsonld::context_spec::ContextSpec;
use andromeda_ngsi_ld_expansion::{entity_expansion::EntityExpansion, prefixed_names::path_attribute};
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, identifier::Identifier};
use andromeda_ngsi_ld_model::temporal::{temporal_document::TemporalDocument, written_temporal_entity::WrittenTemporalEntity};
use andromeda_ngsi_ld_query::dataset::DatasetSelector;
use andromeda_telemetry::temporal::{self, TemporalOperation};
use std::slice::from_ref;

impl TemporalService {
    /// Create a temporal entity or append the supplied observations when it already exists.
    ///
    /// # Errors
    ///
    /// Returns [`TemporalError::Validation`] when the payload is no Temporal
    /// Evolution the model admits, carries no `id` or no `type` (clause
    /// 4.5.1), puts a `unitCode` on an attribute family clause 4.5.2 defines
    /// without one, or carries a `GeoProperty` whose geometry is unusable
    /// (clause 4.7); [`TemporalError::PayloadBudget`] when it exceeds the
    /// budget an entity is admitted under; [`TemporalError::JsonLd`] when the
    /// payload cannot be expanded against `context`; and
    /// [`TemporalError::Commit`] or [`TemporalError::Persistence`] when the
    /// atomic write cannot be landed.
    pub async fn create_or_update(
        &self,
        target: &ContextTarget,
        payload: WrittenTemporalEntity,
        context: &ContextSpec,
    ) -> Result<CreateOrUpdateResult, TemporalError> {
        let timer = temporal::start(TemporalOperation::CreateOrUpdate);
        let result = self.create_or_update_inner(target, payload, context).await;
        timer.finish_result(&result);
        result
    }

    /// The measured body of [`TemporalService::create_or_update`].
    async fn create_or_update_inner(
        &self,
        target: &ContextTarget,
        payload: WrittenTemporalEntity,
        context: &ContextSpec,
    ) -> Result<CreateOrUpdateResult, TemporalError> {
        let expansion = EntityExpansion::prepare(&self.engine, context).await?;
        let evolution = WrittenEvolution::expand(&expansion, payload).await?;
        self.record_evolution(target, evolution).await
    }

    /// Record the Temporal Evolution `history`, already read into the model,
    /// as a client's write of it would: created, or appended to the evolution
    /// that exists.
    ///
    /// # Errors
    ///
    /// As [`TemporalService::create_or_update`], except that nothing is
    /// expanded.
    pub async fn record_history(&self, target: &ContextTarget, history: TemporalDocument) -> Result<CreateOrUpdateResult, TemporalError> {
        let timer = temporal::start(TemporalOperation::CreateOrUpdate);
        let result = match WrittenEvolution::of_history(history) {
            Ok(evolution) => self.record_evolution(target, evolution).await,
            Err(error) => Err(error),
        };
        timer.finish_result(&result);
        result
    }

    /// Land a written evolution: held to the entity budget, then every
    /// instance to history and the standing ones to current state.
    async fn record_evolution(&self, target: &ContextTarget, evolution: WrittenEvolution) -> Result<CreateOrUpdateResult, TemporalError> {
        evolution.admit_budget(self.limits.payload_budget)?;
        let document = evolution.document();
        let entity_id = document
            .id()
            .cloned() // clone: the identifier outlives the evolution it is read from
            .ok_or(TemporalError::Validation(TemporalValidationReason::EntityMissingId))?;
        if document.types().is_empty() {
            return Err(TemporalError::Validation(TemporalValidationReason::EntityMissingType));
        }

        // Clause 5.6.11.4: an existing Temporal Evolution gets the instances
        // added "as mandated by clause 5.6.12"; otherwise "implementations
        // shall create the provided Temporal Evolution of an Entity". Both are
        // the same write -- every instance to history, the standing ones to
        // current state -- and the answer differs only in what it reports.
        let scope = self.reader.read_scope(target, ScopeReach::Entity(&entity_id)).await?;
        let exists = self.reader.entity_exists(&scope, &entity_id).await?;
        self.commit_evolution(target, &entity_id, &evolution).await?;
        if exists {
            Ok(CreateOrUpdateResult::Updated)
        } else {
            Ok(CreateOrUpdateResult::Created(entity_id))
        }
    }

    /// Add temporal attributes to an existing temporal entity.
    ///
    /// # Errors
    ///
    /// Returns [`TemporalError::NotFound`] when the entity has no Temporal
    /// Evolution, [`TemporalError::Validation`] when the payload is no
    /// fragment the model admits, puts a `unitCode` on an attribute family
    /// clause 4.5.2 defines without one or carries a `GeoProperty` whose
    /// geometry is unusable (clause 4.7), [`TemporalError::PayloadBudget`]
    /// when it exceeds the budget an entity is admitted under,
    /// [`TemporalError::JsonLd`] when the payload cannot be expanded against
    /// `context`, and [`TemporalError::Commit`] or
    /// [`TemporalError::Persistence`] when the append cannot be landed.
    pub async fn add_attrs(
        &self,
        target: &ContextTarget,
        entity_id: &Identifier,
        payload: WrittenTemporalEntity,
        context: &ContextSpec,
    ) -> Result<(), TemporalError> {
        let timer = temporal::start(TemporalOperation::AddAttributes);
        let result = self.add_attrs_inner(target, entity_id, payload, context).await;
        timer.finish_result(&result);
        result
    }

    /// The measured body of [`TemporalService::add_attrs`].
    async fn add_attrs_inner(
        &self,
        target: &ContextTarget,
        entity_id: &Identifier,
        payload: WrittenTemporalEntity,
        context: &ContextSpec,
    ) -> Result<(), TemporalError> {
        // Clause 5.6.12 addresses the Temporal Evolution by the request path, and
        // clause 4.5.6 makes it the Entity with the types its evolution records:
        // the fragment need name neither, yet the instances it adds may have to
        // bring current state back into being under the path's identifier. An
        // entity the catalogue records no evolution of has none to add to.
        let scope = self.reader.read_scope(target, ScopeReach::Entity(entity_id)).await?;
        let Some(recorded_types) = self.reader.entity_types(&scope, from_ref(entity_id)).await?.remove(entity_id) else {
            // clone: the refusal owns the identifier it names.
            return Err(TemporalError::NotFound(TemporalNotFoundReason::Entity { id: entity_id.clone() }));
        };

        let expansion = EntityExpansion::prepare(&self.engine, context).await?;
        let evolution = WrittenEvolution::expand(&expansion, payload).await?;
        evolution.admit_budget(self.limits.payload_budget)?;
        let evolution = evolution.addressed_to(entity_id, recorded_types);
        self.commit_evolution(target, entity_id, &evolution).await?;
        Ok(())
    }

    /// Resolve a compact attribute name while retaining compatibility with already-expanded names.
    ///
    /// # Errors
    ///
    /// Returns [`TemporalError::Validation`] for a name `context` reads as a
    /// prefixed name that breaks the clause 4.6.2 grammar after its colon, or
    /// whose expansion is no attribute name, and the failure of a context that
    /// does not resolve or of a store read.
    pub(crate) async fn resolve_attr_iri_for_entity(
        &self,
        target: &ContextTarget,
        entity_id: &Identifier,
        attr_id: &AttributeName,
        context: &ContextSpec,
    ) -> Result<AttributeName, TemporalError> {
        let prepared_context = self.engine.prepare_term_context(context);
        path_attribute(prepared_context.expansion(&self.engine).await?, attr_id)?;
        let expanded_attr = self
            .engine
            .expand_term(attr_id.as_str(), &prepared_context)
            .await?
            .to_string()
            .parse::<AttributeName>()
            .map_err(|source| TemporalError::Validation(TemporalValidationReason::AttributeName { source }))?;

        if self.mutator.attribute_exists(target, entity_id, &expanded_attr).await? {
            return Ok(expanded_attr);
        }

        if expanded_attr != *attr_id && self.mutator.attribute_exists(target, entity_id, attr_id).await? {
            return Ok(attr_id.clone()); // clone: the resolved name is returned after borrowing the request value.
        }

        let attr_names = self.mutator.attribute_names_for_entity(target, entity_id).await?;
        let mut matches = attr_names
            .into_iter()
            .filter(|name| iri_short_name(name.as_str()).is_some_and(|short| short == attr_id.as_str()))
            .collect::<Vec<_>>();

        if matches.len() == 1
            && let Some(name) = matches.pop()
        {
            return Ok(name);
        }

        Ok(expanded_attr)
    }

    /// Delete all temporal data for an entity.
    ///
    /// # Errors
    ///
    /// Returns [`TemporalError::NotFound`] when the entity has no Temporal
    /// Evolution; [`TemporalError::HistoryUnsettled`] when another delivery
    /// owner still holds the entity's queued history once the mutation
    /// barrier's wait runs out; the failure of a queued intent that does not
    /// apply; and [`TemporalError::Persistence`] when the deletion cannot be
    /// applied.
    pub async fn delete(&self, target: &ContextTarget, entity_id: &Identifier) -> Result<(), TemporalError> {
        let timer = temporal::start(TemporalOperation::Delete);
        let result = self.delete_inner(target, entity_id).await;
        timer.finish_result(&result);
        result
    }

    /// The measured body of [`TemporalService::delete`].
    async fn delete_inner(&self, target: &ContextTarget, entity_id: &Identifier) -> Result<(), TemporalError> {
        self.mutation_barrier.synchronize(target, entity_id).await?;
        if !self.mutator.entity_exists(target, entity_id).await? {
            return Err(TemporalError::NotFound(TemporalNotFoundReason::Entity { id: entity_id.clone() }));
        }
        self.mutator.delete_all_for_entity(target, entity_id).await?;
        Ok(())
    }

    /// Delete a specific temporal attribute (optionally filtered by dataset ID).
    ///
    /// # Errors
    ///
    /// Returns [`TemporalError::NotFound`] when the entity has no Temporal
    /// Evolution, when it carries no such attribute, or when the named
    /// `datasetId` -- or the default instance -- matches nothing;
    /// [`TemporalError::Validation`] when the attribute name does not expand
    /// to a valid one; [`TemporalError::HistoryUnsettled`] when another
    /// delivery owner still holds the entity's queued history once the
    /// mutation barrier's wait runs out; the failure of a queued intent that
    /// does not apply; and [`TemporalError::Persistence`] when the deletion
    /// cannot be applied.
    pub async fn delete_attr(
        &self,
        target: &ContextTarget,
        entity_id: &Identifier,
        attr_id: &AttributeName,
        dataset_id: Option<&DatasetSelector>,
        scope: TemporalDeleteScope,
        context: &ContextSpec,
    ) -> Result<(), TemporalError> {
        let timer = temporal::start(TemporalOperation::DeleteAttribute);
        let result = self.delete_attr_inner(target, entity_id, attr_id, dataset_id, scope, context).await;
        timer.finish_result(&result);
        result
    }

    /// The measured body of [`TemporalService::delete_attr`].
    async fn delete_attr_inner(
        &self,
        target: &ContextTarget,
        entity_id: &Identifier,
        attr_id: &AttributeName,
        dataset_id: Option<&DatasetSelector>,
        scope: TemporalDeleteScope,
        context: &ContextSpec,
    ) -> Result<(), TemporalError> {
        self.mutation_barrier.synchronize(target, entity_id).await?;
        let attr_iri = self.resolve_attr_iri_for_entity(target, entity_id, attr_id, context).await?;

        if !self.mutator.entity_exists(target, entity_id).await? {
            return Err(TemporalError::NotFound(TemporalNotFoundReason::Entity { id: entity_id.clone() }));
        }

        if !self.mutator.attribute_exists(target, entity_id, &attr_iri).await? {
            return Err(TemporalError::NotFound(TemporalNotFoundReason::AttributeOnEntity {
                attribute: attr_iri.clone(),
                entity: entity_id.clone(),
            }));
        }

        let dataset_uri = match dataset_id {
            Some(DatasetSelector::Id(uri)) => Some(uri),
            Some(DatasetSelector::Default) | None => None,
        };
        let deleted = self.mutator.delete_attribute(target, entity_id, &attr_iri, dataset_uri, scope).await?;
        if deleted == 0 {
            // clone: the error owns the borrowed request attribute and identifier.
            let missing = match dataset_id {
                Some(DatasetSelector::Id(ds_id)) => TemporalNotFoundReason::AttributeDatasetOnEntity {
                    attribute: attr_iri.clone(),
                    dataset_id: Box::new(ds_id.clone()),
                    entity: entity_id.clone(),
                },
                Some(DatasetSelector::Default) | None => TemporalNotFoundReason::AttributeDefaultInstanceOnEntity {
                    attribute: attr_iri.clone(),
                    entity: entity_id.clone(),
                },
            };
            return Err(TemporalError::NotFound(missing));
        }
        Ok(())
    }
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::{
        error::TemporalError,
        reified_scope_fixture::{evolution_with_scope, refused_scopes},
        stub_broker::{StubBroker, live_target, service, uri},
        temporal_validation_reason::TemporalValidationReason,
    };
    use andromeda_jsonld::context_spec::ContextSpec;
    use andromeda_ngsi_ld_entity::{refusal::EntityRefusal, scope_refusal::ScopeRefusal};

    /// Whether `refused` is the clause 5.5.4 `BadRequestData` naming `expected`.
    fn is_scope_refusal(refused: &TemporalError, expected: &ScopeRefusal) -> bool {
        matches!(refused, TemporalError::Validation(TemporalValidationReason::Representation(EntityRefusal::Scope(refusal))) if refusal == expected)
    }

    /// Clauses 4.5.6 and 5.6.11.3: a temporal create whose reified Scope
    /// carries a member beyond the four Temporal Properties is refused before
    /// anything is recorded.
    #[tokio::test]
    async fn a_temporal_create_refuses_a_reified_scope_carrying_a_forbidden_member() {
        let service = service(StubBroker::new([]));
        for (scope, expected) in refused_scopes() {
            let refused = service
                .create_or_update(&live_target(), evolution_with_scope(&scope), &ContextSpec::core_only())
                .await
                .expect_err("the reified Scope carries a forbidden member");
            assert!(is_scope_refusal(&refused, &expected), "{scope}: {refused:?}");
        }
    }

    /// Clauses 4.5.6 and 5.6.12.3: appending to an existing Temporal
    /// Evolution holds its reified Scope to the same members.
    #[tokio::test]
    async fn a_temporal_append_refuses_a_reified_scope_carrying_a_forbidden_member() {
        let entity_id = uri("urn:ngsi-ld:Vehicle:1");
        let service = service(StubBroker::new([(uri("urn:ngsi-ld:Vehicle:1"), Vec::new())]));
        for (scope, expected) in refused_scopes() {
            let refused = service
                .add_attrs(&live_target(), &entity_id, evolution_with_scope(&scope), &ContextSpec::core_only())
                .await
                .expect_err("the reified Scope carries a forbidden member");
            assert!(is_scope_refusal(&refused, &expected), "{scope}: {refused:?}");
        }
    }
}
