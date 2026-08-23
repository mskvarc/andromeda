//! Updating the attributes an entity already has (clause 5.6.2).

use crate::{
    EntityService,
    attribute_envelope::validate_attribute_envelopes,
    attrs::attribute_write::payload_write,
    error::EntityError,
    expanded_member::unwrap_expanded_ref,
    geo_property_geometry::validate_geo_property_geometries,
    notification_plan::attribute_change_groups,
};
use andromeda_context_commit::{change_event::ChangeEvent, history_record::mutation_history};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_jsonld::context_spec::ContextSpec;
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, uri::Uri};
use andromeda_ngsi_ld_model::expanded::{entity_member::extract_types, mutation::entity::update_entity_attrs};
use andromeda_ngsi_ld_representation::normalization::normalize_entity_attributes;
use serde_json::Value;
use std::sync::Arc;

impl EntityService {
    /// Update the attributes an entity already carries (clause 5.6.2).
    ///
    /// Returns the attribute IRIs that were NOT updated, because the entity did
    /// not already carry them. The handler uses that to decide between 204 and
    /// 207.
    ///
    /// The update is computed from a payload read outside the writing
    /// transaction, so the write states the `modifiedAt` it assumed and is
    /// recomputed against a fresh read if a concurrent writer got in first.
    pub async fn update_attrs(&self, target: &ContextTarget, id: &Uri, mut fragment: Value, context: &ContextSpec) -> Result<Vec<AttributeName>, EntityError> {
        let entity_uri = id.clone(); // clone: notification and computed write own the identifier

        normalize_entity_attributes(&mut fragment);
        let expanded = Arc::new(self.engine.expand(fragment, context).await?);
        validate_attribute_envelopes(&expanded)?;
        validate_geo_property_geometries(&expanded)?;
        let (events, not_updated): (Vec<ChangeEvent>, Vec<AttributeName>) = self
            .commit_computed_write(target, &entity_uri, |previous| {
                let mut existing = previous.clone(); // clone: the update mutates its own copy of the stored payload
                let not_updated = update_entity_attrs(&mut existing, unwrap_expanded_ref(&expanded))?;

                let merged = Arc::new(existing);
                let types = extract_types(&merged);

                // The durable matches and the in-process events are planned
                // together from one partition, so the two channels always agree
                // on the per-attribute triggers (clause 5.2.12).
                let groups = attribute_change_groups(previous, &merged);
                let plan = self.plan_notifications(target, &entity_uri, &types, &groups, Some(&merged), Some(previous));

                let history = mutation_history(&expanded, previous, &merged);
                let write = payload_write(&entity_uri, &merged, history, plan.matches);
                Ok((write, (plan.events, not_updated)))
            })
            .await?;

        for event in events {
            self.emit_change(target, event);
        }

        Ok(not_updated)
    }
}
