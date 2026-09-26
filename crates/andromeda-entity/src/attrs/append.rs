//! Appending attributes to an entity (clause 5.6.3).

use crate::{
    EntityService,
    attrs::attribute_write::payload_write,
    error::EntityError,
    expanded_member::{unwrap_expanded_ref, written_attribute_names, written_entity_types},
    notification_plan::attribute_change_groups,
    write_admission::{EntityWrite, admit_expanded_entity},
};
use andromeda_context_commit::history_record::mutation_history;
use andromeda_context_space::context_target::ContextTarget;
use andromeda_jsonld::context_spec::ContextSpec;
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, uri::Uri};
use andromeda_ngsi_ld_model::expanded::{
    attribute_system_time::stamp_attribute_instants,
    mutation::{entity::append_entity_attrs, error::ExpandedEntityError, selection::OverwritePolicy},
    written_observation::AddressedInstances,
};
use andromeda_ngsi_ld_representation::normalization::normalize_entity_attributes;
use chrono::Utc;
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
        // Clause 5.6.3.4 folds the fragment's `scope` into the target's; it is
        // not one of the partial update or merge fragments clause 5.5.4 excepts
        // from its first-level NGSI-LD Null prohibition, so a deletion sentinel
        // has no reading here.
        admit_expanded_entity(&expanded, EntityWrite::AppendAttributes.null_sentinel_admission())?;

        // Clause 5.6.3.5 reports "the list of Attributes actually appended", so
        // the reported set starts from the fragment's own attributes. Reading it
        // from the merged entity instead would report every attribute the entity
        // already held as if this request had appended it.
        let requested = written_attribute_names(unwrap_expanded_ref(&expanded))?;
        let (not_appended, events) = self
            .commit_computed_write(target, &entity_uri, |previous| {
                let now = Utc::now();
                let mut existing = previous.clone(); // clone: the append mutates its own copy of the stored payload
                let not_appended = append_entity_attrs(&mut existing, unwrap_expanded_ref(&expanded), overwrite)?;
                // Clause 4.8: a touched instance takes `now` for modifiedAt and keeps
                // its createdAt across the read-modify-write.
                stamp_attribute_instants(&mut existing, previous, now).map_err(ExpandedEntityError::from)?;
                let merged = Arc::new(existing);

                // The attributes actually appended are those the fragment
                // carried minus the instances `noOverwrite` skipped. This is the
                // set the change describes to `watchedAttributes` matching and to
                // the notification's `changedAttributes`; the entity's other
                // attributes did not change.
                let types = written_entity_types(&merged)?;
                let groups = attribute_change_groups(previous, &merged)?;
                let plan = self.plan_notifications(target, &entity_uri, &types, &groups, Some(&merged), Some(previous));

                let addressed = AddressedInstances::EntityFragment {
                    fragment: &expanded,
                    overwrite,
                };
                let history = mutation_history(addressed, previous, &merged)?;
                let write = payload_write(&entity_uri, &merged, history, plan.matches)?;
                Ok((write, (not_appended, plan.events)))
            })
            .await?;

        for event in events {
            self.emit_change(target, event);
        }

        Ok((requested, not_appended))
    }
}
