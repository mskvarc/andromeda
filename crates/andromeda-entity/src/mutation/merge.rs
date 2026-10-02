//! Merge Entity (clause 5.6.17, behaviour in clause 5.5.12).

use crate::{
    EntityService,
    admitted_write::AdmittedEntity,
    entity_type_match::entity_satisfies,
    entity_validation_reason::EntityValidationReason,
    error::EntityError,
    merge_fragment::{adapt_member, held_attribute_family},
    merge_options::MergeOptions,
    modification_write::ModificationWrite,
    write_kind::MergeEntity,
};
use andromeda_context_commit::history_record::mutation_history;
use andromeda_context_space::context_target::ContextTarget;
use andromeda_jsonld::{context_spec::ContextSpec, engine::term_lookup::PreparedTermContext, term_expansion::ContextualExpander};
use andromeda_ngsi_ld_entity::{
    entity_state::EntityState,
    history::written_observation::AddressedInstances,
    operation::selection::OverwritePolicy,
    system_instant::SystemInstant,
};
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, identifier::Identifier};
use andromeda_ngsi_ld_model::entity::EntityPatch;
use andromeda_ngsi_ld_query::type_selection::expansion::expand_type_expression;
use andromeda_ngsi_ld_representation::entity_members::ENTITY_MEMBERS;
use andromeda_telemetry::entity::{self, EntityOperation};
use chrono::Utc;
use serde_json::Value;
use std::sync::Arc;

impl EntityService {
    /// Merge a partial update into an existing entity (NGSI-LD 5.6.17) under
    /// the Table 6.5.3.4-1 options the request named.
    ///
    /// `admitted` is `patch` as the clause 5.5.4 admission expanded it. That is
    /// the fragment the merge applies unless `format` or `lang` asks for it to
    /// be rewritten against the stored entity first: the rewritten fragment is
    /// a different document, so `patch` is admitted again once rewritten.
    ///
    /// The merge is computed from a state read outside the writing
    /// transaction, so the write states the `modifiedAt` it assumed and is
    /// recomputed against a fresh read if a concurrent writer got in first.
    ///
    /// # Errors
    ///
    /// Reports [`EntityError::Validation`] when a rewritten patch cannot be
    /// serialised, cannot be adapted to the attribute families the target
    /// holds, or sets a `datasetId` to the NGSI-LD Null (clause 5.5.12),
    /// [`EntityError::JsonLd`] when the patch or the type expression cannot be
    /// expanded against `context`, [`EntityError::NotFound`] when no entity
    /// carries `id` or the type selector excludes it,
    /// [`EntityError::Contention`] when concurrent writers superseded the
    /// computed write on every attempt, and [`EntityError::Persistence`] when
    /// the store could not apply it.
    pub async fn merge(
        &self,
        target: &ContextTarget,
        id: &Identifier,
        patch: &EntityPatch,
        admitted: AdmittedEntity<MergeEntity>,
        context: &ContextSpec,
        options: MergeOptions<'_>,
    ) -> Result<(), EntityError> {
        let timer = entity::start(EntityOperation::Merge);
        let result: Result<(), EntityError> = async {
            let prepared_context = self.engine.prepare_term_context(context);
            let type_selector = match options.type_selector {
                Some(raw) => Some(expand_type_expression(raw, &ContextualExpander::new(&self.engine, &prepared_context)).await?),
                None => None,
            };
            let fragment = if options.rewrites_fragment() {
                let mut patch_json = serde_json::to_value(patch).map_err(|err| EntityError::Validation(EntityValidationReason::CompactDocument(err)))?;
                self.adapt_merge_fragment(target, id, &mut patch_json, &prepared_context, options).await?;
                // Clause 5.5.12 lets a merge fragment carry the NGSI-LD Null at
                // any depth, but "a datasetId cannot be deleted by setting it to
                // the value urn:ngsi-ld:null". Clause 4.18 admits the sentinel as
                // the whole value of `scope`, which clause 5.5.12 then reads as
                // the deletion of that member.
                self.admit_remaining_entity::<MergeEntity>(patch_json, context).await?.into_document()
            } else {
                admitted.into_document()
            };

            let now = SystemInstant::new(Utc::now());
            let events = self
                .commit_computed_write(target, id, |previous| {
                    // Clause 5.6.17.4: the target is the entity with this id "and,
                    // where specified, type"; one outside the selector is not it.
                    if type_selector.as_ref().is_some_and(|selector| !entity_satisfies(previous, selector)) {
                        return Err(EntityError::not_found(id));
                    }
                    let mut merged = EntityState::clone(previous); // clone: shares every attribute the merge leaves alone
                    merged.merge(&fragment, options.observed_at)?;
                    // Clause 4.8: a merged instance keeps its createdAt and, when its
                    // content changed, takes `now` for modifiedAt.
                    merged.stamp_attribute_instants(Some(previous), now);
                    let merged = Arc::new(merged);
                    let addressed = AddressedInstances::EntityFragment {
                        fragment: &fragment,
                        overwrite: OverwritePolicy::Overwrite,
                    };
                    let history = mutation_history(addressed, previous, &merged);
                    let ModificationWrite { write, events } = self.modification_write(target, id, previous, merged, history)?;
                    Ok((write, events))
                })
                .await?;

            for event in events {
                self.notifier.publish(target, event);
            }

            Ok(())
        }
        .await;
        timer.finish_result(&result);
        result
    }

    /// Rewrite a merge fragment for the attribute types the target already
    /// holds (Table 6.5.3.4-1 `format=simplified` and `lang`).
    ///
    /// Only reached for options that ask for a rewrite; a target that does not
    /// exist is left for the write to report as not found.
    async fn adapt_merge_fragment(
        &self,
        target: &ContextTarget,
        id: &Identifier,
        fragment: &mut Value,
        prepared_context: &PreparedTermContext,
        options: MergeOptions<'_>,
    ) -> Result<(), EntityError> {
        let Some(members) = fragment.as_object_mut() else {
            return Ok(());
        };
        let Some(existing) = self.reader.get(target, id).await? else {
            return Ok(());
        };
        for (member, value) in members.iter_mut() {
            if ENTITY_MEMBERS.contains(&member.as_str()) {
                continue;
            }
            let iri = self.engine.expand_term(member, prepared_context).await?;
            let attribute = member
                .parse::<AttributeName>()
                .map_err(|source| EntityError::Validation(EntityValidationReason::AttributeMemberName(source)))?;
            adapt_member(
                &attribute,
                value,
                held_attribute_family(&existing, iri.as_str()),
                options.representation,
                options.lang,
            )
            .map_err(|error| EntityError::Validation(EntityValidationReason::MemberAdaptation(error)))?;
        }
        Ok(())
    }
}
