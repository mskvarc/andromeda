//! Deleting an attribute from an entity (clause 5.6.5).

use crate::{
    EntityService,
    attrs::attribute_write::{MutatedEntity, payload_write},
    error::EntityError,
    expanded_member::{attr_name_of, written_entity_types},
    notification_plan::modification,
};
use andromeda_context_commit::history_record::{HistoryRecord, tombstone};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_jsonld::context_spec::ContextSpec;
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, attribute_type::AttributeType, identifier::Identifier};
use andromeda_ngsi_ld_model::expanded::{
    instance_key::attribute_instance_family,
    mutation::{attribute::delete_attr, error::ExpandedEntityError, selection::DatasetSelection},
};
use andromeda_ngsi_ld_vocabulary::expanded::properties as expanded_properties;
use serde_json::Value;
use std::sync::Arc;

impl EntityService {
    /// Delete one attribute, or one of its instances, from an entity
    /// (clause 5.6.5).
    ///
    /// The deletion is computed from a payload read outside the writing
    /// transaction, so the write states the `modifiedAt` it assumed and is
    /// recomputed against a fresh read if a concurrent writer got in first.
    ///
    /// # Errors
    ///
    /// Reports [`EntityError::JsonLd`] when `attr_name` cannot be expanded
    /// against `context`, [`EntityError::NotFound`] when no entity carries
    /// `id`, [`EntityError::MemberNotFound`] when the entity carries neither
    /// the attribute nor the selected instance, [`EntityError::Contention`]
    /// when concurrent writers superseded the computed write on every attempt,
    /// and [`EntityError::Persistence`] when the store could not apply it.
    pub async fn delete_attr(
        &self,
        target: &ContextTarget,
        id: &Identifier,
        attr_name: &AttributeName,
        selection: DatasetSelection<'_>,
        context: &ContextSpec,
    ) -> Result<(), EntityError> {
        let entity_uri = id.clone(); // clone: notification and computed write own the identifier
        let id = id.as_str();

        let prepared_context = self.engine.prepare_term_context(context);
        let attr_iri = self.engine.expand_term(attr_name.as_str(), &prepared_context).await?;
        let expanded_attr_name = attr_name_of(attr_iri.as_str())?;

        // Resolve target dataset ids for tombstones before mutating the entity
        // payload. The catalogue outlives the attributes it describes, so this
        // does not have to be re-read per attempt.
        let tombstone_targets: Vec<Option<Identifier>> = match selection {
            DatasetSelection::All => match self.attribute_history.dataset_identifiers(target, &entity_uri, &expanded_attr_name).await {
                Ok(dataset_identifiers) => dataset_identifiers,
                Err(e) => {
                    tracing::warn!(error = %e, entity_id = %id, event = "tea_lookup_failed_for_deletion");
                    Vec::new()
                }
            },
            DatasetSelection::Single(dataset_id) => vec![Some(dataset_id.clone())], // clone: the tombstone target survives the borrowed selection.
            DatasetSelection::Default => vec![None],
        };

        let MutatedEntity { events } = self
            .commit_computed_write(target, &entity_uri, |previous| {
                let mut existing = Value::clone(previous); // clone: the deletion mutates its own copy of the stored payload
                delete_attr(&mut existing, &attr_iri, selection)?;

                // Clause 5.6.5.4 handles the Scope before the `deleteAll` and
                // `datasetId` bullets — "If the target Attribute is scope,
                // remove the scope Attribute from the target Entity" — so the
                // removal is unconditional, and clause 4.18 ends the Scope's
                // validity at that delete operation. The Scope has no attribute
                // stream to tombstone: clause 4.5.4 gives it no dataset
                // dimension and no attribute family, so its history is the
                // scope removal and nothing else.
                let history = if expanded_attr_name.as_str() == expanded_properties::SCOPE.as_str() {
                    vec![HistoryRecord::ScopeRemoval]
                } else {
                    tombstone_targets
                        .iter()
                        .map(|target| {
                            // The family is read from the payload the deletion is
                            // computed against, so the tombstone renders the null member
                            // its family prescribes (clauses 4.5.7 and 4.5.8). A target
                            // the catalogue knows and the payload does not is a Property
                            // by clause 4.5.2.
                            let attribute_type = attribute_instance_family(previous, &expanded_attr_name, target.as_ref())
                                .map_err(ExpandedEntityError::from)?
                                .unwrap_or(AttributeType::Property);
                            Ok(tombstone(&expanded_attr_name, target.as_ref(), attribute_type))
                        })
                        .collect::<Result<Vec<HistoryRecord>, EntityError>>()?
                };

                let merged = Arc::new(existing);
                let types = written_entity_types(&merged)?;
                let change = modification(previous, &merged)?;
                let plan = self.notifier.plan(target, &entity_uri, &types, change, Some(&merged), Some(previous));

                let write = payload_write(&entity_uri, &merged, history, plan.matches)?;
                Ok((write, MutatedEntity { events: plan.events }))
            })
            .await?;

        for event in events {
            self.notifier.publish(target, event);
        }

        Ok(())
    }
}
