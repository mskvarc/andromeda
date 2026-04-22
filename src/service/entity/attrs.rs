use crate::{
    core::error::AndromedaError,
    domain::{
        notification::{ChangeEvent, TriggerReason},
        uri::Uri,
    },
    i18n::collation::augment_expanded_with_collation_metadata,
    jsonld::{ContextSpec, vocab},
    query::compiled::PurgeMode,
    service::entity::{
        EntityService,
        extract::save_entity,
        extract_attribute_keys,
        extract_entity_types,
        normalize_entity_attributes,
        unwrap_expanded_ref,
        validate_attribute_envelopes,
    },
};
use serde_json::Value;
use urn_rs::Urn;

impl EntityService {
    /// 5.6.3 Append Entity Attributes
    ///
    /// Returns `(all_attr_iris, not_appended_iris)`:
    /// - all attribute IRIs from the fragment
    /// - attribute IRIs that were NOT appended (because `noOverwrite` was set
    ///   and the instance already existed)
    pub async fn append_attrs(
        &self,
        id: &Urn,
        mut fragment: Value,
        no_overwrite: bool,
        context: &ContextSpec,
    ) -> Result<(Vec<String>, Vec<String>), AndromedaError> {
        let entity_uri = Uri::from(id.clone()); // clone: owned Urn for Uri conversion; id is &Urn
        let id = id.to_string();

        normalize_entity_attributes(&mut fragment);
        let mut expanded = self.engine.expand(fragment, context).await?;
        augment_expanded_with_collation_metadata(&mut expanded);
        validate_attribute_envelopes(&expanded)?;
        let attrs = unwrap_expanded_ref(&expanded);

        // Load existing entity, apply append, save back
        let mut existing = self.repo.get(&id).await?.ok_or_else(|| AndromedaError::entity_not_found(&id))?;
        let not_appended = crate::domain::expanded::append_entity_attrs(&mut existing, attrs, no_overwrite).map_err(AndromedaError::entity_validation)?;
        save_entity(&self.repo, &id, &existing).await?;

        if let Err(e) = self.temporal_recorder.record_entity_attributes(&id, &expanded).await {
            tracing::warn!(error = %e, entity_id = %id, event = "temporal_recording_failed");
        }

        let entity_types = extract_entity_types(&existing);
        self.emit_change(
            ChangeEvent::new(entity_uri, entity_types, TriggerReason::AttributeCreated).with_changed_attributes(extract_attribute_keys(&expanded)),
        );

        let all_attr_keys = extract_attribute_keys(&expanded);
        Ok((all_attr_keys, not_appended))
    }

    /// 5.6.2 Update Entity Attributes
    ///
    /// Returns a list of attribute IRIs that were NOT updated (because they
    /// did not already exist on the entity). The handler uses this to decide
    /// between 204 and 207.
    pub async fn update_attrs(&self, id: &Urn, mut fragment: Value, context: &ContextSpec) -> Result<Vec<String>, AndromedaError> {
        let entity_uri = Uri::from(id.clone()); // clone: owned Urn for Uri conversion; id is &Urn
        let id = id.to_string();

        normalize_entity_attributes(&mut fragment);
        let mut expanded = self.engine.expand(fragment, context).await?;
        augment_expanded_with_collation_metadata(&mut expanded);
        validate_attribute_envelopes(&expanded)?;
        let attrs = unwrap_expanded_ref(&expanded);

        // Single fetch: used for deletion detection, showChanges, and update
        let mut existing = self.repo.get(&id).await?.ok_or_else(|| AndromedaError::entity_not_found(&id))?;
        let previous_attributes = self.change_tx.as_ref().map(|_| existing.clone()); // clone: snapshot before mutation for showChanges diff
        let attrs_before: std::collections::HashSet<String> = extract_attribute_keys(&existing).into_iter().collect();
        let not_updated = crate::domain::expanded::update_entity_attrs(&mut existing, attrs).map_err(AndromedaError::entity_validation)?;
        save_entity(&self.repo, &id, &existing).await?;

        if let Err(e) = self.temporal_recorder.record_entity_attributes(&id, &expanded).await {
            tracing::warn!(error = %e, entity_id = %id, event = "temporal_recording_failed");
        }

        let entity_types = self.fetch_entity_types(&id).await;

        // Detect which attributes were deleted
        let attrs_after: std::collections::HashSet<String> = extract_attribute_keys(&existing).into_iter().collect();
        let deleted_attrs: Vec<String> = attrs_before.difference(&attrs_after).cloned().collect(); // clone: owned strings from set difference

        // Record temporal tombstones for deleted attributes
        for deleted_attr in &deleted_attrs {
            let _ = self.temporal_recorder.record_attribute_deletion(&id, deleted_attr, None).await;
        }

        for deleted_attr in &deleted_attrs {
            self.emit_attr_change(
                entity_uri.clone(),   // clone: value needed by subsequent loop iterations and final emit
                entity_types.clone(), // clone: value needed by subsequent loop iterations and final emit
                TriggerReason::AttributeDeleted,
                vec![deleted_attr.clone()],  // clone: owned copy from borrowed iterator
                previous_attributes.clone(), // clone: value needed by subsequent loop iterations and final emit
            );
        }

        let all_changed = extract_attribute_keys(&expanded);
        let created_attrs: Vec<String> = all_changed.iter().filter(|a| !attrs_before.contains(a.as_str())).cloned().collect(); // clone: owned strings from filtered references
        let updated_attrs: Vec<String> = all_changed.iter().filter(|a| attrs_before.contains(a.as_str())).cloned().collect(); // clone: owned strings from filtered references

        if !created_attrs.is_empty() {
            self.emit_attr_change(
                entity_uri.clone(),   // clone: value also needed by subsequent AttributeUpdated emit
                entity_types.clone(), // clone: value also needed by subsequent AttributeUpdated emit
                TriggerReason::AttributeCreated,
                created_attrs,
                previous_attributes.clone(), // clone: value also needed by subsequent AttributeUpdated emit
            );
        }

        if !updated_attrs.is_empty() {
            self.emit_attr_change(entity_uri, entity_types, TriggerReason::AttributeUpdated, updated_attrs, previous_attributes);
        }

        Ok(not_updated)
    }

    /// 5.6.4 Partial Attribute Update
    pub async fn partial_update_attr(
        &self,
        id: &Urn,
        attr_name: &str,
        dataset_id: Option<&str>,
        fragment: Value,
        context: &ContextSpec,
    ) -> Result<(), AndromedaError> {
        let entity_uri = Uri::from(id.clone()); // clone: owned Urn for Uri conversion; id is &Urn
        let id = id.to_string();

        let attr_iri = self.engine.expand_term(attr_name, context).await?;

        let mut expanded = self.engine.expand(fragment, context).await?;
        augment_expanded_with_collation_metadata(&mut expanded);
        validate_attribute_envelopes(&expanded)?;
        let attr_value = unwrap_expanded_ref(&expanded);

        // Single fetch: used for showChanges, deletion detection, and partial update
        let mut existing = self.repo.get(&id).await?.ok_or_else(|| AndromedaError::entity_not_found(&id))?;
        let previous_attributes = self.change_tx.as_ref().map(|_| existing.clone()); // clone: snapshot before mutation for showChanges diff
        let attrs_before: std::collections::HashSet<String> = extract_attribute_keys(&existing).into_iter().collect();

        crate::domain::expanded::partial_update_attr(&mut existing, &attr_iri, dataset_id, attr_value).map_err(|msg| {
            if msg.contains("type mismatch") {
                AndromedaError::entity_validation(msg)
            } else {
                AndromedaError::entity_not_found(msg)
            }
        })?;
        save_entity(&self.repo, &id, &existing).await?;

        // Detect if the attribute was actually deleted (null sentinel)
        let attrs_after: std::collections::HashSet<String> = extract_attribute_keys(&existing).into_iter().collect();
        let was_deleted = attrs_before.contains(&attr_iri) && !attrs_after.contains(&attr_iri);

        if was_deleted {
            let _ = self.temporal_recorder.record_attribute_deletion(&id, &attr_iri, dataset_id).await;
        } else if let Err(e) = self.temporal_recorder.record_entity_attributes(&id, &expanded).await {
            tracing::warn!(error = %e, entity_id = %id, event = "temporal_recording_failed");
        }

        let trigger = if was_deleted {
            TriggerReason::AttributeDeleted
        } else {
            TriggerReason::AttributeUpdated
        };

        let entity_types = self.fetch_entity_types(&id).await;
        self.emit_attr_change(entity_uri, entity_types, trigger, vec![attr_iri], previous_attributes);

        Ok(())
    }

    /// 5.6.5 Delete Entity Attribute
    pub async fn delete_attr(
        &self,
        id: &Urn,
        attr_name: &str,
        dataset_id: Option<&str>,
        delete_all: bool,
        context: &ContextSpec,
    ) -> Result<(), AndromedaError> {
        let entity_uri = Uri::from(id.clone()); // clone: owned Urn for Uri conversion; id is &Urn
        let id = id.to_string();

        let attr_iri = self.engine.expand_term(attr_name, context).await?;

        // Single fetch: used for showChanges and deletion
        let mut existing = self.repo.get(&id).await?.ok_or_else(|| AndromedaError::entity_not_found(&id))?;
        let previous_attributes = self.change_tx.as_ref().map(|_| existing.clone()); // clone: snapshot before mutation for showChanges diff

        // Record temporal tombstones before the actual deletion
        if delete_all {
            // Record tombstones for all dataset instances of this attribute
            if let Ok(teas) = self.temporal_recorder.get_teas_for_entity_attr(&id, &attr_iri).await {
                for tea in &teas {
                    if let Some(dataset_id) = tea.dataset_id.as_ref() {
                        let dataset_id = dataset_id.to_string();
                        let _ = self.temporal_recorder.record_attribute_deletion(&id, &attr_iri, Some(&dataset_id)).await;
                    } else {
                        let _ = self.temporal_recorder.record_attribute_deletion(&id, &attr_iri, None).await;
                    }
                }
            }
        } else {
            let _ = self.temporal_recorder.record_attribute_deletion(&id, &attr_iri, dataset_id).await;
        }

        crate::domain::expanded::delete_attr(&mut existing, &attr_iri, dataset_id, delete_all).map_err(AndromedaError::entity_not_found)?;
        save_entity(&self.repo, &id, &existing).await?;

        let entity_types = self.fetch_entity_types(&id).await;
        self.emit_attr_change(entity_uri, entity_types, TriggerReason::AttributeDeleted, vec![attr_iri], previous_attributes);

        Ok(())
    }

    /// 5.6.19 Replace Entity Attribute
    pub async fn replace_attr(
        &self,
        id: &Urn,
        attr_name: &str,
        dataset_id: Option<&str>,
        fragment: Value,
        context: &ContextSpec,
    ) -> Result<(), AndromedaError> {
        let entity_uri = Uri::from(id.clone()); // clone: owned Urn for Uri conversion; id is &Urn
        let id = id.to_string();

        if attr_name.starts_with('@') {
            return Err(AndromedaError::entity_validation("Invalid attribute name"));
        }

        let attr_iri = self.engine.expand_term(attr_name, context).await?;

        if attr_iri == vocab::expanded::SCOPE.as_str() {
            return Err(AndromedaError::entity_validation("Scope is not a replaceable attribute"));
        }

        let mut expanded = self.engine.expand(fragment, context).await?;
        augment_expanded_with_collation_metadata(&mut expanded);
        validate_attribute_envelopes(&expanded)?;
        let attr_value = unwrap_expanded_ref(&expanded);

        // Single fetch: used for showChanges and replacement
        let mut existing = self.repo.get(&id).await?.ok_or_else(|| AndromedaError::entity_not_found(&id))?;
        let previous_attributes = self.change_tx.as_ref().map(|_| existing.clone()); // clone: snapshot before mutation for showChanges diff
        crate::domain::expanded::replace_attr(&mut existing, &attr_iri, dataset_id, attr_value).map_err(AndromedaError::entity_not_found)?;
        save_entity(&self.repo, &id, &existing).await?;

        if let Err(e) = self.temporal_recorder.record_entity_attributes(&id, &expanded).await {
            tracing::warn!(error = %e, entity_id = %id, event = "temporal_recording_failed");
        }

        let entity_types = self.fetch_entity_types(&id).await;
        self.emit_attr_change(entity_uri, entity_types, TriggerReason::AttributeUpdated, vec![attr_iri], previous_attributes);

        Ok(())
    }

    /// Purge attributes from entities according to the given mode (S5.6.21).
    ///
    /// - `Drop`: remove the listed attributes from each entity
    /// - `Keep`: retain only the listed attributes, removing all others
    ///
    /// Returns the number of entities modified.
    pub async fn purge_attrs(&self, entity_ids: &[String], mode: &PurgeMode, context: &ContextSpec) -> Result<u32, AndromedaError> {
        let attr_iris: Vec<String> = match mode {
            PurgeMode::DeleteEntities => return Ok(0),
            PurgeMode::Drop(attrs) | PurgeMode::Keep(attrs) => {
                let mut expanded = Vec::with_capacity(attrs.len());
                for attr in attrs {
                    expanded.push(self.engine.expand_term(attr, context).await?);
                }
                expanded
            }
        };

        let mut modified_count = 0u32;

        for entity_id in entity_ids {
            let Some(mut existing) = self.repo.get(entity_id).await? else {
                continue;
            };

            let obj = match &mut existing {
                Value::Array(arr) => arr.first_mut().and_then(|v| v.as_object_mut()),
                Value::Object(obj) => Some(obj),
                _other => None,
            };

            let Some(obj) = obj else {
                continue;
            };

            let keys_to_remove: Vec<String> = match mode {
                PurgeMode::Drop(_) => {
                    attr_iris.iter().filter(|iri| obj.contains_key(iri.as_str())).cloned().collect() // clone: owned keys for removal
                }
                PurgeMode::Keep(_) => obj
                    .keys()
                    .filter(|k| {
                        // Preserve JSON-LD structural keys and NGSI-LD system attrs
                        !k.starts_with('@') && !attr_iris.iter().any(|iri| k.as_str() == iri)
                    })
                    .cloned() // clone: owned keys for removal
                    .collect(),
                PurgeMode::DeleteEntities => Vec::new(),
            };

            if keys_to_remove.is_empty() {
                continue;
            }

            for key in &keys_to_remove {
                obj.remove(key);
            }

            save_entity(&self.repo, entity_id, &existing).await?;
            modified_count += 1;
        }

        Ok(modified_count)
    }
}
