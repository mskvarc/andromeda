//! Updating the attributes an entity already has (clause 5.6.2).

use crate::{
    core::error::AndromedaError,
    service::entity::{
        EntityService,
        attribute_envelope::validate_attribute_envelopes,
        attrs::attribute_write::{UpdatedAttrs, payload_write},
        expanded_member::{extract_attribute_keys, unwrap_expanded_ref},
        history_record::{HistoryRecord, tombstone},
    },
};
use andromeda_jsonld::context_spec::ContextSpec;
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, uri::Uri};
use andromeda_ngsi_ld_model::{
    expanded::{entity_member::extract_types, mutation::entity::update_entity_attrs},
    notification::TriggerReason,
};
use andromeda_ngsi_ld_representation::normalization::normalize_entity_attributes;
use serde_json::Value;
use std::{collections::HashSet, sync::Arc};
use urn_rs::Urn;

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
    pub async fn update_attrs(&self, id: &Urn, mut fragment: Value, context: &ContextSpec) -> Result<Vec<String>, AndromedaError> {
        let entity_uri = Uri::from(id.clone()); // clone: owned Urn for Uri conversion; id is &Urn
        let id = id.to_string();

        normalize_entity_attributes(&mut fragment);
        let expanded = Arc::new(self.engine.expand(fragment, context).await?);
        validate_attribute_envelopes(&expanded)?;
        let all_changed = extract_attribute_keys(&expanded);

        let UpdatedAttrs {
            merged,
            previous: previous_attributes,
            created: created_attrs,
            updated: updated_attrs,
            deleted: deleted_attrs,
            not_updated,
        } = self
            .commit_computed_write(&id, |previous| {
                let previous_attributes = self.change_tx.as_ref().map(|_| previous.clone()); // clone: snapshot before mutation for showChanges diff
                let attrs_before: HashSet<AttributeName> = extract_attribute_keys(previous).into_iter().collect();

                let mut existing = previous.clone(); // clone: the update mutates its own copy of the stored payload
                let not_updated = update_entity_attrs(&mut existing, unwrap_expanded_ref(&expanded))?;

                let attrs_after: HashSet<AttributeName> = extract_attribute_keys(&existing).into_iter().collect();
                let deleted: Vec<AttributeName> = attrs_before.difference(&attrs_after).cloned().collect(); // clone: owned names from set difference

                let mut history = vec![HistoryRecord::Observations(Arc::clone(&expanded))]; // clone: Arc ref-count bump
                for deleted_attr in &deleted {
                    history.push(tombstone(deleted_attr.as_str(), None)?);
                }

                let merged = Arc::new(existing);
                let write = payload_write(&id, &merged, history);
                Ok((
                    write,
                    UpdatedAttrs {
                        // clone: owned names from filtered references
                        created: all_changed.iter().filter(|name| !attrs_before.contains(name.as_str())).cloned().collect(),
                        updated: all_changed.iter().filter(|name| attrs_before.contains(name.as_str())).cloned().collect(),
                        merged,
                        previous: previous_attributes,
                        deleted,
                        not_updated,
                    },
                ))
            })
            .await?;

        let entity_types = extract_types(&merged);

        for deleted_attr in &deleted_attrs {
            self.emit_attr_change(
                entity_uri.clone(),   // clone: value needed by subsequent loop iterations and final emit
                entity_types.clone(), // clone: value needed by subsequent loop iterations and final emit
                TriggerReason::AttributeDeleted,
                vec![deleted_attr.clone()],  // clone: owned copy from borrowed iterator
                previous_attributes.clone(), // clone: value needed by subsequent loop iterations and final emit
                Some(Arc::clone(&merged)),   // clone: Arc ref-count bump
            );
        }

        if !created_attrs.is_empty() {
            self.emit_attr_change(
                entity_uri.clone(),   // clone: value also needed by subsequent AttributeUpdated emit
                entity_types.clone(), // clone: value also needed by subsequent AttributeUpdated emit
                TriggerReason::AttributeCreated,
                created_attrs,
                previous_attributes.clone(), // clone: value also needed by subsequent AttributeUpdated emit
                Some(Arc::clone(&merged)),   // clone: Arc ref-count bump
            );
        }

        if !updated_attrs.is_empty() {
            self.emit_attr_change(
                entity_uri,
                entity_types,
                TriggerReason::AttributeUpdated,
                updated_attrs,
                previous_attributes,
                Some(merged),
            );
        }

        Ok(not_updated)
    }
}
