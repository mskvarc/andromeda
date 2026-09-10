//! Partially updating one attribute instance (clause 5.6.4).

use crate::{
    EntityService,
    attrs::attribute_write::{MutatedEntity, payload_write},
    error::EntityError,
    expanded_member::unwrap_expanded_ref,
    notification_plan::attribute_change_groups,
    write_admission::{AttributeWrite, admit_expanded_attribute},
};
use andromeda_context_commit::history_record::HistoryRecord;
use andromeda_context_space::context_target::ContextTarget;
use andromeda_jsonld::{attribute_fragment::expand_attribute_fragment, context_spec::ContextSpec};
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, uri::Uri};
use andromeda_ngsi_ld_model::expanded::{
    entity_member::extract_types,
    mutation::attribute::partial_update_attr,
    removed_instance::removed_attribute_instances,
};
use andromeda_ngsi_ld_representation::normalization::normalize_attribute_node;
use andromeda_ngsi_ld_vocabulary::expanded::properties as expanded_properties;
use serde_json::Value;
use std::sync::Arc;

impl EntityService {
    /// Apply a partial update to one attribute instance (clause 5.6.4).
    ///
    /// The update is computed from a payload read outside the writing
    /// transaction, so the write states the `modifiedAt` it assumed and is
    /// recomputed against a fresh read if a concurrent writer got in first.
    ///
    /// # Errors
    ///
    /// Reports [`EntityError::JsonLd`] when `attr_name` or `fragment` cannot be
    /// expanded against `context`, [`EntityError::Validation`] when `attr_name`
    /// names `scope` (clause 5.6.4.4), the fragment carries a malformed
    /// attribute instance or it sets a `datasetId` to the NGSI-LD Null (clause
    /// 5.5.8), [`EntityError::NotFound`] when no entity carries
    /// `id`, [`EntityError::MemberNotFound`] when the entity carries neither
    /// the attribute nor the selected instance, [`EntityError::Contention`]
    /// when concurrent writers superseded the computed write on every attempt,
    /// and [`EntityError::Persistence`] when the store could not apply it.
    pub async fn partial_update_attr(
        &self,
        target: &ContextTarget,
        id: &Uri,
        attr_name: &AttributeName,
        dataset_id: Option<&Uri>,
        mut fragment: Value,
        context: &ContextSpec,
    ) -> Result<(), EntityError> {
        let entity_uri = id.clone(); // clone: notification and computed write own the identifier

        let prepared_context = self.engine.prepare_term_context(context);
        let attr_iri = self.engine.expand_term(attr_name.as_str(), &prepared_context).await?;

        // Clause 5.6.4.4: `scope` is not an attribute and cannot be a partial
        // update target, exactly as Replace Attribute rejects it.
        if attr_iri.as_str() == expanded_properties::SCOPE.as_str() {
            return Err(EntityError::validation("Scope is not a partially updatable attribute"));
        }

        // Clauses 5.6.4 and 5.6.19 take a bare Attribute Fragment, so the
        // entity-level normalization other write paths run does not reach it.
        // Normalizing here is what applies the concise form and discards the
        // output-only `previous...` member clauses 5.2.5 to 5.2.38 require an
        // implementation to ignore when a Context Producer supplies one. The
        // expansion puts the Fragment back under the name the request URI
        // carries, because a term's own definition decides what the members
        // below it mean -- see `andromeda_jsonld::attribute_fragment`.
        normalize_attribute_node(attr_name.as_str(), &mut fragment);
        let expanded = Arc::new(expand_attribute_fragment(&self.engine, attr_name.as_str(), fragment, context).await?);
        // Clause 5.5.8 makes this fragment's NGSI-LD Nulls deletions, with one
        // exception it states outright: "A datasetId cannot be deleted by
        // setting it to the value urn:ngsi-ld:null".
        admit_expanded_attribute(&expanded, attr_name, AttributeWrite::PartialUpdate.null_sentinel_admission())?;

        let MutatedEntity { events } = self
            .commit_computed_write(target, &entity_uri, |previous| {
                let mut existing = previous.clone(); // clone: the update mutates its own copy of the stored payload
                partial_update_attr(&mut existing, &attr_iri, dataset_id, unwrap_expanded_ref(&expanded))?;

                let merged = Arc::new(existing);
                let types = extract_types(&merged);
                let groups = attribute_change_groups(previous, &merged);
                let plan = self.plan_notifications(target, &entity_uri, &types, &groups, Some(&merged), Some(previous));
                let removed = removed_attribute_instances(previous, &merged);
                let history = if removed.is_empty() {
                    vec![HistoryRecord::Observations(Arc::clone(&expanded))]
                } else {
                    removed.into_iter().map(HistoryRecord::Tombstone).collect()
                };

                let write = payload_write(&entity_uri, &merged, history, plan.matches);
                Ok((write, MutatedEntity { events: plan.events }))
            })
            .await?;

        for event in events {
            self.emit_change(target, event);
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        error::EntityError,
        notification_recording_harness::{entity_uri, live_target, recording_service},
    };
    use andromeda_jsonld::context_spec::ContextSpec;
    use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
    use serde_json::json;

    #[tokio::test]
    async fn partial_update_attr_rejects_the_scope_target() {
        // ETSI GS CIM 009 v1.9.1 clause 5.6.4.4: `scope` is not a partial-update
        // target and must be refused as BadRequestData, as Replace Attribute is.
        let (service, _recorded) = recording_service().await;
        let scope: AttributeName = "scope".parse().expect("scope is a valid attribute name");
        let error = service
            .partial_update_attr(
                &live_target(),
                &entity_uri(),
                &scope,
                None,
                json!({"type": "Property", "value": "/Madrid"}),
                &ContextSpec::core_only(),
            )
            .await
            .expect_err("scope cannot be partially updated");
        assert!(matches!(error, EntityError::Validation { .. }), "{error}");
    }
}
