//! Appending attributes to an entity (clause 5.6.3).

use crate::{
    EntityService,
    attribute_envelope::validate_attribute_envelopes,
    attrs::attribute_write::payload_write,
    error::EntityError,
    expanded_member::unwrap_expanded_ref,
    geo_property_geometry::validate_geo_property_geometries,
    notification_plan::attribute_change_groups,
    null_content::reject_null_content,
};
use andromeda_context_commit::history_record::HistoryRecord;
use andromeda_context_space::context_target::ContextTarget;
use andromeda_jsonld::context_spec::ContextSpec;
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, uri::Uri};
use andromeda_ngsi_ld_model::expanded::{
    entity_member::{extract_attribute_names, extract_types},
    mutation::{entity::append_entity_attrs, selection::OverwritePolicy},
};
use andromeda_ngsi_ld_representation::normalization::normalize_entity_attributes;
use serde_json::Value;
use std::sync::Arc;

impl EntityService {
    /// Append attributes to an existing entity (clause 5.6.3).
    ///
    /// Returns `(all_attr_iris, not_appended_iris)`:
    /// - all attribute IRIs the entity carries once the append has landed
    /// - attribute IRIs that were NOT appended, because `noOverwrite` was set
    ///   and the instance already existed
    ///
    /// The append is computed from a payload read outside the writing
    /// transaction, so the write states the `modifiedAt` it assumed and is
    /// recomputed against a fresh read if a concurrent writer got in first.
    ///
    /// # Errors
    ///
    /// Reports [`EntityError::JsonLd`] when `fragment` cannot be expanded
    /// against `context`, [`EntityError::Validation`] when an attribute
    /// instance omits the member its family requires, carries a `unitCode` the
    /// family does not define, repeats an instance identity or holds a
    /// malformed `GeoProperty` geometry, [`EntityError::NotFound`] when no
    /// entity carries `id`, [`EntityError::Contention`] when concurrent writers
    /// superseded the computed write on every attempt, and
    /// [`EntityError::Persistence`] when the store could not apply it.
    pub async fn append_attrs(
        &self,
        target: &ContextTarget,
        id: &Uri,
        mut fragment: Value,
        overwrite: OverwritePolicy,
        context: &ContextSpec,
    ) -> Result<(Vec<AttributeName>, Vec<AttributeName>), EntityError> {
        let entity_uri = id.clone(); // clone: notification and computed write own the identifier

        normalize_entity_attributes(&mut fragment);
        let expanded = Arc::new(self.engine.expand(fragment, context).await?);
        validate_attribute_envelopes(&expanded)?;
        reject_null_content(&expanded)?;
        validate_geo_property_geometries(&expanded)?;

        let (merged, not_appended, events) = self
            .commit_computed_write(target, &entity_uri, |previous| {
                let mut existing = previous.clone(); // clone: the append mutates its own copy of the stored payload
                let not_appended = append_entity_attrs(&mut existing, unwrap_expanded_ref(&expanded), overwrite)?;
                let merged = Arc::new(existing);

                // The attributes actually appended are those the fragment
                // carried minus the instances `noOverwrite` skipped. This is the
                // set the change describes to `watchedAttributes` matching and to
                // the notification's `changedAttributes`; the entity's other
                // attributes did not change.
                let types = extract_types(&merged);
                let groups = attribute_change_groups(previous, &merged);
                let plan = self.plan_notifications(target, &entity_uri, &types, &groups, Some(&merged), Some(previous));

                let write = payload_write(&entity_uri, &merged, vec![HistoryRecord::Observations(Arc::clone(&expanded))], plan.matches); // clone: Arc ref-count bump
                Ok((write, (merged, not_appended, plan.events)))
            })
            .await?;

        let attr_keys = extract_attribute_names(&merged);
        for event in events {
            self.emit_change(target, event);
        }

        Ok((attr_keys, not_appended))
    }
}
