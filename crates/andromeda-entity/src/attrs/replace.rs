//! Replacing one attribute wholesale (clause 5.6.19).

use crate::{
    EntityService,
    attrs::attribute_write::{MutatedEntity, payload_write},
    entity_validation_reason::EntityValidationReason,
    error::EntityError,
    expanded_member::unwrap_expanded_ref,
    notification_plan::attribute_change_groups,
    write_admission::{AttributeWrite, admit_expanded_attribute},
};
use andromeda_context_commit::history_record::HistoryRecord;
use andromeda_context_space::context_target::ContextTarget;
use andromeda_jsonld::{attribute_fragment::expand_attribute_fragment, context_spec::ContextSpec};
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, uri::Uri};
use andromeda_ngsi_ld_model::expanded::{attribute_system_time::stamp_attribute_instants, entity_member::extract_types, mutation::attribute::replace_attr};
use andromeda_ngsi_ld_representation::normalization::normalize_attribute_node;
use andromeda_ngsi_ld_vocabulary::expanded::properties as expanded_properties;
use chrono::Utc;
use serde_json::Value;
use std::sync::Arc;

impl EntityService {
    /// Replace one attribute of an entity wholesale (clause 5.6.19).
    ///
    /// The replacement is computed from a payload read outside the writing
    /// transaction, so the write states the `modifiedAt` it assumed and is
    /// recomputed against a fresh read if a concurrent writer got in first.
    ///
    /// # Errors
    ///
    /// Reports [`EntityError::Validation`] when `attr_name` is a JSON-LD
    /// keyword or names `scope`, or when `fragment` carries a malformed
    /// attribute instance, [`EntityError::JsonLd`] when `attr_name` or
    /// `fragment` cannot be expanded against `context`,
    /// [`EntityError::NotFound`] when no entity carries `id`,
    /// [`EntityError::MemberNotFound`] when the entity carries neither the
    /// attribute nor the selected instance, [`EntityError::Contention`] when
    /// concurrent writers superseded the computed write on every attempt, and
    /// [`EntityError::Persistence`] when the store could not apply it.
    pub async fn replace_attr(
        &self,
        target: &ContextTarget,
        id: &Uri,
        attr_name: &AttributeName,
        dataset_id: Option<&Uri>,
        mut fragment: Value,
        context: &ContextSpec,
    ) -> Result<(), EntityError> {
        let entity_uri = id.clone(); // clone: notification and computed write own the identifier

        if attr_name.as_str().starts_with('@') {
            return Err(EntityError::Validation(EntityValidationReason::ReservedAttributeName {
                name: attr_name.clone(),
            }));
        }

        let prepared_context = self.engine.prepare_term_context(context);
        let attr_iri = self.engine.expand_term(attr_name.as_str(), &prepared_context).await?;

        if attr_iri.as_str() == expanded_properties::SCOPE.as_str() {
            return Err(EntityError::Validation(EntityValidationReason::ScopeNotReplaceable));
        }

        // Clauses 5.6.4 and 5.6.19 take a bare Attribute Fragment, so the
        // entity-level normalization other write paths run does not reach it.
        // Normalizing here is what applies the concise form and discards the
        // output-only `previous...` member clauses 5.2.5 to 5.2.38 require an
        // implementation to ignore when a Context Producer supplies one.
        // The expansion puts the Fragment back under the name the request URI
        // carries, because a term's own definition decides what the members
        // below it mean -- see `andromeda_jsonld::attribute_fragment`.
        normalize_attribute_node(attr_name.as_str(), &mut fragment);
        let expanded = Arc::new(expand_attribute_fragment(&self.engine, attr_name.as_str(), fragment, context).await?);
        admit_expanded_attribute(&expanded, attr_name, AttributeWrite::Replace.null_sentinel_admission())?;

        let MutatedEntity { events } = self
            .commit_computed_write(target, &entity_uri, |previous| {
                let now = Utc::now();
                let mut existing = previous.clone(); // clone: the replacement mutates its own copy of the stored payload
                replace_attr(&mut existing, &attr_iri, dataset_id, unwrap_expanded_ref(&expanded))?;

                // Clause 4.8: a touched instance takes `now` for modifiedAt and keeps

                // its createdAt across the read-modify-write.

                stamp_attribute_instants(&mut existing, previous, now);

                let merged = Arc::new(existing);
                let types = extract_types(&merged);

                let groups = attribute_change_groups(previous, &merged);
                let plan = self.plan_notifications(target, &entity_uri, &types, &groups, Some(&merged), Some(previous));

                let write = payload_write(&entity_uri, &merged, vec![HistoryRecord::Observations(Arc::clone(&expanded))], plan.matches); // clone: Arc ref-count bump
                Ok((write, MutatedEntity { events: plan.events }))
            })
            .await?;

        for event in events {
            self.emit_change(target, event);
        }

        Ok(())
    }
}
