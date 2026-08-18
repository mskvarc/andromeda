//! Updating the attributes an entity already has (clause 5.6.2).

use crate::{
    EntityService,
    attribute_envelope::validate_attribute_envelopes,
    attrs::attribute_write::{UpdatedAttrs, payload_write},
    error::EntityError,
    expanded_member::unwrap_expanded_ref,
    geo_property_geometry::validate_geo_property_geometries,
};
use andromeda_context_commit::history_record::mutation_history;
use andromeda_jsonld::context_spec::ContextSpec;
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, uri::Uri};
use andromeda_ngsi_ld_model::{
    expanded::{
        entity_member::{extract_attribute_names, extract_types},
        mutation::entity::update_entity_attrs,
    },
    notification::NotificationTrigger,
};
use andromeda_ngsi_ld_representation::normalization::normalize_entity_attributes;
use serde_json::Value;
use std::{collections::HashSet, slice::from_ref, sync::Arc};

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
    pub async fn update_attrs(&self, id: &Uri, mut fragment: Value, context: &ContextSpec) -> Result<Vec<AttributeName>, EntityError> {
        let entity_uri = id.clone(); // clone: notification and computed write own the identifier

        normalize_entity_attributes(&mut fragment);
        let expanded = Arc::new(self.engine.expand(fragment, context).await?);
        validate_attribute_envelopes(&expanded)?;
        validate_geo_property_geometries(&expanded)?;
        let all_changed = extract_attribute_names(&expanded);

        let UpdatedAttrs {
            merged,
            types: entity_types,
            previous: previous_attributes,
            created: created_attrs,
            updated: updated_attrs,
            deleted: deleted_attrs,
            not_updated,
        } = self
            .commit_computed_write(&entity_uri, |previous| {
                let previous_attributes = self.change_sink.as_ref().map(|_| previous.clone()); // clone: snapshot before mutation for showChanges diff
                let attrs_before: HashSet<AttributeName> = extract_attribute_names(previous).into_iter().collect();

                let mut existing = previous.clone(); // clone: the update mutates its own copy of the stored payload
                let not_updated = update_entity_attrs(&mut existing, unwrap_expanded_ref(&expanded))?;

                let attrs_after: HashSet<AttributeName> = extract_attribute_names(&existing).into_iter().collect();
                let deleted: Vec<AttributeName> = attrs_before.difference(&attrs_after).cloned().collect(); // clone: owned names from set difference

                let merged = Arc::new(existing);
                let types = extract_types(&merged);
                // clone: owned names from filtered references
                let created: Vec<AttributeName> = all_changed.iter().filter(|name| !attrs_before.contains(name.as_str())).cloned().collect();
                let updated: Vec<AttributeName> = all_changed.iter().filter(|name| attrs_before.contains(name.as_str())).cloned().collect();

                // The durable matches mirror the three Live emits below exactly:
                // one `AttributeDeleted` per removed instance, then a single
                // `AttributeCreated` and `AttributeUpdated` when their groups are
                // non-empty. Each group carries only its own attribute names.
                let mut subscription_matches = Vec::new();
                for deleted_attr in &deleted {
                    subscription_matches.extend(self.build_subscription_matches(
                        &entity_uri,
                        &types,
                        NotificationTrigger::AttributeDeleted,
                        from_ref(deleted_attr),
                        Some(&merged),
                        previous_attributes.as_ref(),
                    ));
                }
                if !created.is_empty() {
                    subscription_matches.extend(self.build_subscription_matches(
                        &entity_uri,
                        &types,
                        NotificationTrigger::AttributeCreated,
                        &created,
                        Some(&merged),
                        previous_attributes.as_ref(),
                    ));
                }
                if !updated.is_empty() {
                    subscription_matches.extend(self.build_subscription_matches(
                        &entity_uri,
                        &types,
                        NotificationTrigger::AttributeUpdated,
                        &updated,
                        Some(&merged),
                        previous_attributes.as_ref(),
                    ));
                }

                let history = mutation_history(&expanded, previous, &merged);
                let write = payload_write(&entity_uri, &merged, history, subscription_matches);
                Ok((
                    write,
                    UpdatedAttrs {
                        merged,
                        types,
                        previous: previous_attributes,
                        created,
                        updated,
                        deleted,
                        not_updated,
                    },
                ))
            })
            .await?;

        for deleted_attr in &deleted_attrs {
            self.emit_attr_change(
                entity_uri.clone(),   // clone: value needed by subsequent loop iterations and final emit
                entity_types.clone(), // clone: value needed by subsequent loop iterations and final emit
                NotificationTrigger::AttributeDeleted,
                vec![deleted_attr.clone()],  // clone: owned copy from borrowed iterator
                previous_attributes.clone(), // clone: value needed by subsequent loop iterations and final emit
                Some(Arc::clone(&merged)),   // clone: Arc ref-count bump
            );
        }

        if !created_attrs.is_empty() {
            self.emit_attr_change(
                entity_uri.clone(),   // clone: value also needed by subsequent AttributeUpdated emit
                entity_types.clone(), // clone: value also needed by subsequent AttributeUpdated emit
                NotificationTrigger::AttributeCreated,
                created_attrs,
                previous_attributes.clone(), // clone: value also needed by subsequent AttributeUpdated emit
                Some(Arc::clone(&merged)),   // clone: Arc ref-count bump
            );
        }

        if !updated_attrs.is_empty() {
            self.emit_attr_change(
                entity_uri,
                entity_types,
                NotificationTrigger::AttributeUpdated,
                updated_attrs,
                previous_attributes,
                Some(merged),
            );
        }

        Ok(not_updated)
    }
}
