use crate::{
    core::error::AndromedaError,
    jsonld::ContextSpec,
    service::entity::{
        EntityService,
        extract::build_save_params,
        extract_attribute_keys,
        normalize_entity_attributes,
        unwrap_expanded_ref,
        validate_attribute_envelopes,
    },
};
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, uri::Uri};
use andromeda_ngsi_ld_model::{
    expanded::{
        entity_member::extract_types,
        instance_key::removed_attribute_instances,
        mutation::{
            attribute::{delete_attr, partial_update_attr, replace_attr},
            entity::{append_entity_attrs, update_entity_attrs},
            error::ExpandedEntityError,
            selection::{DatasetSelection, OverwritePolicy},
        },
    },
    notification::{ChangeEvent, TriggerReason},
};
use andromeda_ngsi_ld_query::projection::purge::PurgeMode;
use andromeda_ngsi_ld_vocabulary::expanded::properties as expanded_properties;
use serde_json::Value;
use std::{collections::HashSet, sync::Arc};
use urn_rs::Urn;

// Lives here rather than beside `AndromedaError` because the domain crate must
// not know the broker's transport-aware error type, and this module is the one
// that owns attribute mutation at the service layer.
impl From<ExpandedEntityError> for AndromedaError {
    /// Map each mutation failure onto the NGSI-LD ProblemDetails type clause
    /// 5.5.2 requires: a malformed request body is `BadRequestData` (400),
    /// while a well-formed request naming an attribute, dataset or instance
    /// the entity does not have is `ResourceNotFound` (404).
    fn from(error: ExpandedEntityError) -> AndromedaError {
        match error {
            ExpandedEntityError::EntityNotExpanded
            | ExpandedEntityError::PatchNotExpanded
            | ExpandedEntityError::AttributesNotObject
            | ExpandedEntityError::AttributeTypeMismatch { .. } => AndromedaError::entity_validation(error.to_string()),
            ExpandedEntityError::AttributeNotFound { .. }
            | ExpandedEntityError::AttributeHasNoInstances { .. }
            | ExpandedEntityError::DatasetNotFound { .. }
            | ExpandedEntityError::DefaultInstanceNotFound { .. } => AndromedaError::entity_not_found(error.to_string()),
        }
    }
}

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
        overwrite: OverwritePolicy,
        context: &ContextSpec,
    ) -> Result<(Vec<AttributeName>, Vec<String>), AndromedaError> {
        let entity_uri = Uri::from(id.clone()); // clone: owned Urn for Uri conversion; id is &Urn
        let id = id.to_string();

        normalize_entity_attributes(&mut fragment);
        let expanded = self.engine.expand(fragment, context).await?;
        validate_attribute_envelopes(&expanded)?;
        let attrs = unwrap_expanded_ref(&expanded);

        // Load existing entity, apply append, save back
        let mut existing = self.repo.get(&id).await?.ok_or_else(|| AndromedaError::entity_not_found(&id))?;
        let not_appended = append_entity_attrs(&mut existing, attrs, overwrite)?;

        let mut uow = self.uow.begin().await?;
        uow.save_entity(build_save_params(&id, &existing)).await?;
        uow.record_entity_attributes(&id, &expanded).await?;
        uow.commit().await?;

        let entity_types = extract_types(&existing);
        let all_attr_keys = extract_attribute_keys(&expanded);
        let payload_arc = Arc::new(existing);
        self.emit_change(
            ChangeEvent::new(entity_uri, entity_types, TriggerReason::AttributeCreated)
                .with_changed_attributes(extract_attribute_keys(&expanded))
                .with_current_payload(payload_arc),
        );

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
        let expanded = self.engine.expand(fragment, context).await?;
        validate_attribute_envelopes(&expanded)?;
        let attrs = unwrap_expanded_ref(&expanded);

        // Single fetch: used for deletion detection, showChanges, and update
        let mut existing = self.repo.get(&id).await?.ok_or_else(|| AndromedaError::entity_not_found(&id))?;
        let previous_attributes = self.change_tx.as_ref().map(|_| existing.clone()); // clone: snapshot before mutation for showChanges diff
        let attrs_before: HashSet<AttributeName> = extract_attribute_keys(&existing).into_iter().collect();
        let not_updated = update_entity_attrs(&mut existing, attrs)?;

        // Detect which attributes were deleted by the update
        let attrs_after: HashSet<AttributeName> = extract_attribute_keys(&existing).into_iter().collect();
        let deleted_attrs: Vec<AttributeName> = attrs_before.difference(&attrs_after).cloned().collect(); // clone: owned names from set difference

        let mut uow = self.uow.begin().await?;
        uow.save_entity(build_save_params(&id, &existing)).await?;
        uow.record_entity_attributes(&id, &expanded).await?;
        for deleted_attr in &deleted_attrs {
            uow.record_attribute_deletion(&id, deleted_attr.as_str(), None).await?;
        }
        uow.commit().await?;

        let entity_types = self.fetch_entity_types(&id).await;
        let all_changed = extract_attribute_keys(&expanded);
        let created_attrs: Vec<AttributeName> = all_changed.iter().filter(|a| !attrs_before.contains(a.as_str())).cloned().collect(); // clone: owned names from filtered references
        let updated_attrs: Vec<AttributeName> = all_changed.iter().filter(|a| attrs_before.contains(a.as_str())).cloned().collect(); // clone: owned names from filtered references
        let payload_arc = Arc::new(existing);

        for deleted_attr in &deleted_attrs {
            self.emit_attr_change(
                entity_uri.clone(),   // clone: value needed by subsequent loop iterations and final emit
                entity_types.clone(), // clone: value needed by subsequent loop iterations and final emit
                TriggerReason::AttributeDeleted,
                vec![deleted_attr.clone()],  // clone: owned copy from borrowed iterator
                previous_attributes.clone(), // clone: value needed by subsequent loop iterations and final emit
                Some(Arc::clone(&payload_arc)),
            );
        }

        if !created_attrs.is_empty() {
            self.emit_attr_change(
                entity_uri.clone(),   // clone: value also needed by subsequent AttributeUpdated emit
                entity_types.clone(), // clone: value also needed by subsequent AttributeUpdated emit
                TriggerReason::AttributeCreated,
                created_attrs,
                previous_attributes.clone(), // clone: value also needed by subsequent AttributeUpdated emit
                Some(Arc::clone(&payload_arc)),
            );
        }

        if !updated_attrs.is_empty() {
            self.emit_attr_change(
                entity_uri,
                entity_types,
                TriggerReason::AttributeUpdated,
                updated_attrs,
                previous_attributes,
                Some(payload_arc),
            );
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

        let expanded = self.engine.expand(fragment, context).await?;
        validate_attribute_envelopes(&expanded)?;
        let attr_value = unwrap_expanded_ref(&expanded);

        // Single fetch: used for showChanges, deletion detection, and partial update
        let mut existing = self.repo.get(&id).await?.ok_or_else(|| AndromedaError::entity_not_found(&id))?;
        let previous_attributes = self.change_tx.as_ref().map(|_| existing.clone()); // clone: snapshot before mutation for showChanges diff
        let attrs_before: HashSet<AttributeName> = extract_attribute_keys(&existing).into_iter().collect();

        partial_update_attr(&mut existing, &attr_iri, dataset_id, attr_value)?;

        // Detect if the attribute was actually deleted (null sentinel)
        let attrs_after: HashSet<AttributeName> = extract_attribute_keys(&existing).into_iter().collect();
        let was_deleted = attrs_before.contains(attr_iri.as_str()) && !attrs_after.contains(attr_iri.as_str());

        let mut uow = self.uow.begin().await?;
        uow.save_entity(build_save_params(&id, &existing)).await?;
        if was_deleted {
            uow.record_attribute_deletion(&id, &attr_iri, dataset_id).await?;
        } else {
            uow.record_entity_attributes(&id, &expanded).await?;
        }
        uow.commit().await?;

        let trigger = if was_deleted {
            TriggerReason::AttributeDeleted
        } else {
            TriggerReason::AttributeUpdated
        };

        let entity_types = self.fetch_entity_types(&id).await;
        let payload_arc = Arc::new(existing);
        self.emit_attr_change(
            entity_uri,
            entity_types,
            trigger,
            vec![attr_name_of(&attr_iri)?],
            previous_attributes,
            Some(payload_arc),
        );

        Ok(())
    }

    /// 5.6.5 Delete Entity Attribute
    pub async fn delete_attr(&self, id: &Urn, attr_name: &str, selection: DatasetSelection<'_>, context: &ContextSpec) -> Result<(), AndromedaError> {
        let entity_uri = Uri::from(id.clone()); // clone: owned Urn for Uri conversion; id is &Urn
        let id = id.to_string();

        let attr_iri = self.engine.expand_term(attr_name, context).await?;

        // Single fetch: used for showChanges and deletion
        let mut existing = self.repo.get(&id).await?.ok_or_else(|| AndromedaError::entity_not_found(&id))?;
        let previous_attributes = self.change_tx.as_ref().map(|_| existing.clone()); // clone: snapshot before mutation for showChanges diff

        // Resolve target dataset ids for tombstones before mutating the entity payload.
        let tombstone_targets: Vec<Option<String>> = match selection {
            DatasetSelection::All => match self.temporal_recorder.get_teas_for_entity_attr(&id, &attr_iri).await {
                Ok(teas) => teas.iter().map(|tea| tea.dataset_id.as_ref().map(ToString::to_string)).collect(),
                Err(e) => {
                    tracing::warn!(error = %e, entity_id = %id, event = "tea_lookup_failed_for_deletion");
                    Vec::new()
                }
            },
            DatasetSelection::Single(dataset_id) => vec![Some(dataset_id.to_string())],
            DatasetSelection::Default => vec![None],
        };

        delete_attr(&mut existing, &attr_iri, selection)?;

        let mut uow = self.uow.begin().await?;
        uow.save_entity(build_save_params(&id, &existing)).await?;
        for target in &tombstone_targets {
            uow.record_attribute_deletion(&id, &attr_iri, target.as_deref()).await?;
        }
        uow.commit().await?;

        let entity_types = self.fetch_entity_types(&id).await;
        let payload_arc = Arc::new(existing);
        self.emit_attr_change(
            entity_uri,
            entity_types,
            TriggerReason::AttributeDeleted,
            vec![attr_name_of(&attr_iri)?],
            previous_attributes,
            Some(payload_arc),
        );

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

        if attr_iri == expanded_properties::SCOPE.as_str() {
            return Err(AndromedaError::entity_validation("Scope is not a replaceable attribute"));
        }

        let expanded = self.engine.expand(fragment, context).await?;
        validate_attribute_envelopes(&expanded)?;
        let attr_value = unwrap_expanded_ref(&expanded);

        // Single fetch: used for showChanges and replacement
        let mut existing = self.repo.get(&id).await?.ok_or_else(|| AndromedaError::entity_not_found(&id))?;
        let previous_attributes = self.change_tx.as_ref().map(|_| existing.clone()); // clone: snapshot before mutation for showChanges diff
        replace_attr(&mut existing, &attr_iri, dataset_id, attr_value)?;

        let mut uow = self.uow.begin().await?;
        uow.save_entity(build_save_params(&id, &existing)).await?;
        uow.record_entity_attributes(&id, &expanded).await?;
        uow.commit().await?;

        let entity_types = self.fetch_entity_types(&id).await;
        let payload_arc = Arc::new(existing);
        self.emit_attr_change(
            entity_uri,
            entity_types,
            TriggerReason::AttributeUpdated,
            vec![attr_name_of(&attr_iri)?],
            previous_attributes,
            Some(payload_arc),
        );

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
                    expanded.push(self.engine.expand_term(attr.as_str(), context).await?);
                }
                expanded
            }
        };

        let mut modified_count = 0u32;

        for entity_id in entity_ids {
            // Each entity is its own unit of work: the payload rewrite and the
            // tombstones for the purged instances must commit together, and a
            // single entity failing must not roll back the ones already purged.
            let mut uow = self.uow.begin().await?;
            let Some(mut existing) = uow.get_entity(entity_id).await? else {
                continue;
            };
            let previous = existing.clone(); // clone: "before" snapshot for the removal diff; `existing` is mutated in place

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

            let removed = removed_attribute_instances(&previous, &existing);
            uow.save_entity(build_save_params(entity_id, &existing)).await?;
            uow.record_removed_attributes(entity_id, &removed).await?;
            uow.commit().await?;
            modified_count += 1;
        }

        Ok(modified_count)
    }
}

/// Parse an expanded attribute IRI into the domain's attribute-name type.
///
/// The IRI comes straight out of JSON-LD term expansion, so a value that fails
/// the NGSI-LD name grammar means the caller asked for something that cannot
/// name an attribute at all.
fn attr_name_of(attr_iri: &str) -> Result<AttributeName, AndromedaError> {
    attr_iri
        .parse()
        .map_err(|err| AndromedaError::entity_validation(format!("Invalid attribute name '{attr_iri}': {err}")))
}
