//! Partially updating one attribute instance (clause 5.6.4).

use crate::{
    EntityService,
    attribute_envelope::validate_attribute_envelopes,
    attrs::attribute_write::{MutatedEntity, payload_write},
    error::EntityError,
    expanded_member::unwrap_expanded_ref,
    geo_property_geometry::validate_geo_property_geometries,
    notification_plan::attribute_change_groups,
};
use andromeda_context_commit::history_record::HistoryRecord;
use andromeda_jsonld::context_spec::ContextSpec;
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, uri::Uri};
use andromeda_ngsi_ld_model::expanded::{
    entity_member::extract_types,
    mutation::attribute::partial_update_attr,
    removed_instance::removed_attribute_instances,
};
use serde_json::Value;
use std::sync::Arc;

impl EntityService {
    /// Apply a partial update to one attribute instance (clause 5.6.4).
    ///
    /// The update is computed from a payload read outside the writing
    /// transaction, so the write states the `modifiedAt` it assumed and is
    /// recomputed against a fresh read if a concurrent writer got in first.
    pub async fn partial_update_attr(
        &self,
        id: &Uri,
        attr_name: &AttributeName,
        dataset_id: Option<&Uri>,
        fragment: Value,
        context: &ContextSpec,
    ) -> Result<(), EntityError> {
        let entity_uri = id.clone(); // clone: notification and computed write own the identifier

        let prepared_context = self.engine.prepare_term_context(context);
        let attr_iri = self.engine.expand_term(attr_name.as_str(), &prepared_context).await?;
        let expanded = Arc::new(self.engine.expand(fragment, context).await?);
        validate_attribute_envelopes(&expanded)?;
        validate_geo_property_geometries(&expanded)?;

        let MutatedEntity { events } = self
            .commit_computed_write(&entity_uri, |previous| {
                let mut existing = previous.clone(); // clone: the update mutates its own copy of the stored payload
                partial_update_attr(&mut existing, &attr_iri, dataset_id, unwrap_expanded_ref(&expanded))?;

                let merged = Arc::new(existing);
                let types = extract_types(&merged);
                let groups = attribute_change_groups(previous, &merged);
                let plan = self.plan_notifications(&entity_uri, &types, &groups, Some(&merged), Some(previous));
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

        events.into_iter().for_each(|event| self.emit_change(event));

        Ok(())
    }
}
