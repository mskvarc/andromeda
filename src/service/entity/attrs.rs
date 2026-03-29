use super::{EntityService, extract_attribute_keys, extract_entity_types, normalize_entity_attributes, unwrap_expanded, validate_attribute_envelopes};
use crate::{error::AndromedaError, jsonld::ContextSpec};
use ngsi_ld_types::notification::{ChangeEvent, TriggerReason};
use serde_json::Value;
use urn::Urn;

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
        let id = id.to_string();

        normalize_entity_attributes(&mut fragment);
        let expanded = self.engine.expand(fragment, context).await?;
        validate_attribute_envelopes(&expanded)?;
        let attrs = unwrap_expanded(expanded.clone());

        // Load existing entity, apply append, save back
        let mut existing = self.repo.get(&id).await?.ok_or_else(|| AndromedaError::not_found(&id))?;
        let not_appended = ngsi_ld_types::expanded::append_entity_attrs(&mut existing, &attrs, no_overwrite).map_err(AndromedaError::bad_request_data)?;
        self.repo.save(&id, &existing).await?;

        if let Err(e) = self.temporal_recorder.record_entity_attributes(&id, &expanded).await {
            tracing::warn!(error = %e, entity_id = %id, event = "temporal_recording_failed");
        }

        let entity_types = extract_entity_types(&existing);
        self.emit_change(ChangeEvent::new(id, entity_types, TriggerReason::AttributeCreated).with_changed_attributes(extract_attribute_keys(&expanded)));

        let all_attr_keys = extract_attribute_keys(&expanded);
        Ok((all_attr_keys, not_appended))
    }

    /// 5.6.2 Update Entity Attributes
    ///
    /// Returns a list of attribute IRIs that were NOT updated (because they
    /// did not already exist on the entity). The handler uses this to decide
    /// between 204 and 207.
    pub async fn update_attrs(&self, id: &Urn, mut fragment: Value, context: &ContextSpec) -> Result<Vec<String>, AndromedaError> {
        let id = id.to_string();

        normalize_entity_attributes(&mut fragment);
        let expanded = self.engine.expand(fragment, context).await?;
        validate_attribute_envelopes(&expanded)?;
        let attrs = unwrap_expanded(expanded.clone());

        // Single fetch: used for deletion detection, showChanges, and update
        let mut existing = self.repo.get(&id).await?.ok_or_else(|| AndromedaError::not_found(&id))?;
        let previous_attributes = Some(existing.clone());
        let attrs_before: std::collections::HashSet<String> = extract_attribute_keys(&existing).into_iter().collect();
        let not_updated = ngsi_ld_types::expanded::update_entity_attrs(&mut existing, &attrs).map_err(AndromedaError::bad_request_data)?;
        self.repo.save(&id, &existing).await?;

        if let Err(e) = self.temporal_recorder.record_entity_attributes(&id, &expanded).await {
            tracing::warn!(error = %e, entity_id = %id, event = "temporal_recording_failed");
        }

        let entity_types = self.fetch_entity_types(&id).await;

        // Detect which attributes were deleted
        let attrs_after: std::collections::HashSet<String> = extract_attribute_keys(&existing).into_iter().collect();
        let deleted_attrs: Vec<String> = attrs_before.difference(&attrs_after).cloned().collect();

        // Record temporal tombstones for deleted attributes
        for deleted_attr in &deleted_attrs {
            let _ = self.temporal_recorder.record_attribute_deletion(&id, deleted_attr, None).await;
        }

        for deleted_attr in &deleted_attrs {
            self.emit_attr_change(
                id.clone(),
                entity_types.clone(),
                TriggerReason::AttributeDeleted,
                vec![deleted_attr.clone()],
                previous_attributes.clone(),
            );
        }

        let all_changed = extract_attribute_keys(&expanded);
        let created_attrs: Vec<String> = all_changed.iter().filter(|a| !attrs_before.contains(a.as_str())).cloned().collect();
        let updated_attrs: Vec<String> = all_changed.iter().filter(|a| attrs_before.contains(a.as_str())).cloned().collect();

        if !created_attrs.is_empty() {
            self.emit_attr_change(
                id.clone(),
                entity_types.clone(),
                TriggerReason::AttributeCreated,
                created_attrs,
                previous_attributes.clone(),
            );
        }

        if !updated_attrs.is_empty() {
            self.emit_attr_change(id, entity_types, TriggerReason::AttributeUpdated, updated_attrs, previous_attributes);
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
        let id = id.to_string();

        let attr_iri = self.engine.expand_term(attr_name, context).await?;

        let expanded = self.engine.expand(fragment, context).await?;
        validate_attribute_envelopes(&expanded)?;
        let attr_value = unwrap_expanded(expanded.clone());

        // Single fetch: used for showChanges, deletion detection, and partial update
        let mut existing = self.repo.get(&id).await?.ok_or_else(|| AndromedaError::not_found(&id))?;
        let previous_attributes = Some(existing.clone());
        let attrs_before: std::collections::HashSet<String> = extract_attribute_keys(&existing).into_iter().collect();

        ngsi_ld_types::expanded::partial_update_attr(&mut existing, &attr_iri, dataset_id, &attr_value).map_err(|msg| {
            if msg.contains("type mismatch") {
                AndromedaError::bad_request_data(msg)
            } else {
                AndromedaError::not_found(msg)
            }
        })?;
        self.repo.save(&id, &existing).await?;

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
        self.emit_attr_change(id, entity_types, trigger, vec![attr_iri], previous_attributes);

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
        let id = id.to_string();

        let attr_iri = self.engine.expand_term(attr_name, context).await?;

        // Single fetch: used for showChanges and deletion
        let mut existing = self.repo.get(&id).await?.ok_or_else(|| AndromedaError::not_found(&id))?;
        let previous_attributes = Some(existing.clone());

        // Record temporal tombstones before the actual deletion
        if delete_all {
            // Record tombstones for all dataset instances of this attribute
            if let Ok(teas) = self.temporal_recorder.get_teas_for_entity_attr(&id, &attr_iri).await {
                for tea in &teas {
                    let _ = self
                        .temporal_recorder
                        .record_attribute_deletion(&id, &attr_iri, tea.dataset_id.as_deref())
                        .await;
                }
            }
        } else {
            let _ = self.temporal_recorder.record_attribute_deletion(&id, &attr_iri, dataset_id).await;
        }

        ngsi_ld_types::expanded::delete_attr(&mut existing, &attr_iri, dataset_id, delete_all).map_err(|msg| AndromedaError::not_found(msg))?;
        self.repo.save(&id, &existing).await?;

        let entity_types = self.fetch_entity_types(&id).await;
        self.emit_attr_change(id, entity_types, TriggerReason::AttributeDeleted, vec![attr_iri], previous_attributes);

        Ok(())
    }

    /// 5.6.19 Replace Entity Attribute
    pub async fn replace_attr(&self, id: &Urn, attr_name: &str, fragment: Value, context: &ContextSpec) -> Result<(), AndromedaError> {
        let id = id.to_string();

        if attr_name.starts_with('@') {
            return Err(AndromedaError::bad_request_data("Invalid attribute name"));
        }

        let attr_iri = self.engine.expand_term(attr_name, context).await?;

        if attr_iri == "https://uri.etsi.org/ngsi-ld/scope" {
            return Err(AndromedaError::bad_request_data("Scope is not a replaceable attribute"));
        }

        let expanded = self.engine.expand(fragment, context).await?;
        validate_attribute_envelopes(&expanded)?;
        let attr_value = unwrap_expanded(expanded.clone());

        // Single fetch: used for showChanges and replacement
        let mut existing = self.repo.get(&id).await?.ok_or_else(|| AndromedaError::not_found(&id))?;
        let previous_attributes = Some(existing.clone());
        ngsi_ld_types::expanded::replace_attr(&mut existing, &attr_iri, &attr_value).map_err(|msg| AndromedaError::not_found(msg))?;
        self.repo.save(&id, &existing).await?;

        if let Err(e) = self.temporal_recorder.record_entity_attributes(&id, &expanded).await {
            tracing::warn!(error = %e, entity_id = %id, event = "temporal_recording_failed");
        }

        let entity_types = self.fetch_entity_types(&id).await;
        self.emit_attr_change(id, entity_types, TriggerReason::AttributeUpdated, vec![attr_iri], previous_attributes);

        Ok(())
    }
}
