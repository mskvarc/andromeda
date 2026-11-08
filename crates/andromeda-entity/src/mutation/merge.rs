//! Merge Entity (clause 5.6.17, behaviour in clause 5.5.12).

use crate::{
    EntityService,
    admitted_write::AdmittedEntity,
    entity_validation_reason::EntityValidationReason,
    error::EntityError,
    merge_fragment::{adapt_member, held_attribute_family},
    merge_options::MergeOptions,
    read_modify_write::ComputedLanding,
    write_kind::MergeEntity,
    write_target::WriteTarget,
};
use andromeda_context_commit::history_record::mutation_history;
use andromeda_context_space::context_target::ContextTarget;
use andromeda_jsonld::{engine::term_lookup::PreparedTermContext, user_context_admission::AdmittedContext};
use andromeda_ngsi_ld_entity::entity_state::EntityState;
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, identifier::Identifier};
use andromeda_ngsi_ld_model::{attribute::merge_member::MergeMember, entity::EntityPatch};
use andromeda_telemetry::entity::{self, EntityOperation};
use std::sync::Arc;

impl EntityService {
    /// Merge a partial update into an existing entity (NGSI-LD 5.6.17) under
    /// the Table 6.5.3.4-1 options the request named.
    ///
    /// `admitted` is `patch` as the clause 5.5.4 admission expanded it. That is
    /// the fragment the merge applies unless `format` or `lang` asks for it to
    /// be rewritten against the stored entity first: the rewritten fragment is
    /// a different document, so `patch` is admitted again once rewritten, and
    /// the attributes `options.claimed` names are taken out of it.
    ///
    /// The merge is computed from a state read outside the writing
    /// transaction, so the write states the `modifiedAt` it assumed and is
    /// recomputed against a fresh read if a concurrent writer got in first.
    ///
    /// # Errors
    ///
    /// Reports [`EntityError::Validation`] when a rewritten patch cannot be
    /// adapted to the attribute families the target
    /// holds, or sets a `datasetId` to the NGSI-LD Null (clause 5.5.12),
    /// [`EntityError::JsonLd`] when the patch cannot be expanded against
    /// `context`, [`EntityError::NotFound`] when no entity carries the
    /// identifier of `write_target` or its type selector excludes it (clause
    /// 5.6.17.4),
    /// [`EntityError::Contention`] when concurrent writers superseded the
    /// computed write on every attempt, and [`EntityError::Persistence`] when
    /// the store could not apply it.
    pub async fn merge(
        &self,
        target: &ContextTarget,
        write_target: WriteTarget<'_>,
        patch: &EntityPatch<MergeMember>,
        admitted: AdmittedEntity<MergeEntity>,
        context: &AdmittedContext,
        options: MergeOptions<'_>,
    ) -> Result<(), EntityError> {
        let id = write_target.id;
        let timer = entity::start(EntityOperation::Merge);
        let result: Result<(), EntityError> = async {
            let prepared_context = self.engine.prepare_term_context(context);
            let fragment = if options.rewrites_fragment() {
                let mut adapted = patch.clone(); // clone: the caller keeps the fragment as written, for the registrations it forwards it to
                self.adapt_merge_fragment(target, id, &mut adapted, &prepared_context, options).await?;
                // Clause 5.5.12 lets a merge fragment carry the NGSI-LD Null at
                // any depth, but "a datasetId cannot be deleted by setting it to
                // the value urn:ngsi-ld:null". Clause 4.18 admits the sentinel as
                // the whole value of `scope`, which clause 5.5.12 then reads as
                // the deletion of that member.
                let rewritten = self.admit_entity_write::<MergeEntity>(&adapted, context).await?;
                // Clause 4.3.6.3: the fragment as written still names the
                // attributes proxied registrations took, and the rewrite must
                // not bring them back into this merge.
                match rewritten.local_remainder(options.claimed) {
                    Some(remainder) => remainder.into_document(),
                    None => return Ok(()),
                }
            } else {
                admitted.into_document()
            };

            let ComputedLanding { value: events, committed_at } = self
                .commit_computed_write(target, write_target, |read, now| {
                    let previous = &read.state;
                    let mut merged = EntityState::clone(previous);
                    merged.merge(&fragment, options.observed_at)?;
                    // Clause 4.8: a merged instance keeps its createdAt and, when its
                    // content changed, takes `now` for modifiedAt.
                    merged.settle(Some(previous), now);
                    let merged = Arc::new(merged);
                    let history = mutation_history(previous, &merged);
                    let (write, events) = self.write_if_modified(target, id, read, merged, history, now)?;
                    Ok((write, events))
                })
                .await?;

            if let Some(committed_at) = committed_at {
                self.notifier.publish(target, committed_at, events).await;
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
        fragment: &mut EntityPatch<MergeMember>,
        prepared_context: &PreparedTermContext,
        options: MergeOptions<'_>,
    ) -> Result<(), EntityError> {
        let Some(existing) = self.reader.get(target, id).await? else {
            return Ok(());
        };
        for (member, value) in fragment.attribute_members_mut() {
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
