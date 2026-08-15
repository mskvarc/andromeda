//! Deleting an attribute from an entity (clause 5.6.5).

use crate::{
    EntityService,
    attrs::attribute_write::{MutatedEntity, payload_write},
    error::EntityError,
    expanded_member::attr_name_of,
};
use andromeda_context_commit::history_record::tombstone;
use andromeda_jsonld::context_spec::ContextSpec;
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, attribute_type::AttributeType, uri::Uri};
use andromeda_ngsi_ld_model::{
    expanded::{
        entity_member::extract_types,
        instance_key::attribute_instance_family,
        mutation::{attribute::delete_attr, selection::DatasetSelection},
    },
    notification::NotificationTrigger,
};
use std::{slice::from_ref, sync::Arc};

impl EntityService {
    /// Delete one attribute, or one of its instances, from an entity
    /// (clause 5.6.5).
    ///
    /// The deletion is computed from a payload read outside the writing
    /// transaction, so the write states the `modifiedAt` it assumed and is
    /// recomputed against a fresh read if a concurrent writer got in first.
    pub async fn delete_attr(&self, id: &Uri, attr_name: &AttributeName, selection: DatasetSelection<'_>, context: &ContextSpec) -> Result<(), EntityError> {
        let entity_uri = id.clone(); // clone: notification and computed write own the identifier
        let id = id.as_str();

        let prepared_context = self.engine.prepare_term_context(context);
        let attr_iri = self.engine.expand_term(attr_name.as_str(), &prepared_context).await?;
        let expanded_attr_name = attr_name_of(attr_iri.as_str())?;

        // Resolve target dataset ids for tombstones before mutating the entity
        // payload. The catalogue outlives the attributes it describes, so this
        // does not have to be re-read per attempt.
        let tombstone_targets: Vec<Option<Uri>> = match selection {
            DatasetSelection::All => match self.attribute_history.dataset_identifiers(&entity_uri, &expanded_attr_name).await {
                Ok(dataset_identifiers) => dataset_identifiers,
                Err(e) => {
                    tracing::warn!(error = %e, entity_id = %id, event = "tea_lookup_failed_for_deletion");
                    Vec::new()
                }
            },
            DatasetSelection::Single(dataset_id) => vec![Some(dataset_id.clone())], // clone: the tombstone target survives the borrowed selection.
            DatasetSelection::Default => vec![None],
        };

        let MutatedEntity {
            merged,
            types: entity_types,
            previous: previous_attributes,
            trigger,
        } = self
            .commit_computed_write(&entity_uri, |previous| {
                let previous_attributes = self.change_sink.as_ref().map(|_| previous.clone()); // clone: snapshot before mutation for showChanges diff

                let mut existing = previous.clone(); // clone: the deletion mutates its own copy of the stored payload
                delete_attr(&mut existing, &attr_iri, selection)?;

                let mut history = Vec::with_capacity(tombstone_targets.len());
                for target in &tombstone_targets {
                    // The family is read from the payload the deletion is
                    // computed against, so the tombstone renders the null member
                    // its family prescribes (clauses 4.5.7 and 4.5.8). A target
                    // the catalogue knows and the payload does not is a Property
                    // by clause 4.5.2.
                    let attribute_type = attribute_instance_family(previous, &expanded_attr_name, target.as_ref()).unwrap_or(AttributeType::Property);
                    history.push(tombstone(&expanded_attr_name, target.as_ref(), attribute_type));
                }

                let merged = Arc::new(existing);
                let types = extract_types(&merged);

                // The durable match mirrors the single Live `emit_attr_change`
                // below: `AttributeDeleted` for the one attribute this call
                // removed, with the same before-snapshot.
                let subscription_matches = self.build_subscription_matches(
                    &entity_uri,
                    &types,
                    NotificationTrigger::AttributeDeleted,
                    from_ref(&expanded_attr_name),
                    Some(&merged),
                    previous_attributes.as_ref(),
                );

                let write = payload_write(&entity_uri, &merged, history, subscription_matches);
                Ok((
                    write,
                    MutatedEntity {
                        merged,
                        types,
                        previous: previous_attributes,
                        trigger: NotificationTrigger::AttributeDeleted,
                    },
                ))
            })
            .await?;

        self.emit_attr_change(entity_uri, entity_types, trigger, vec![expanded_attr_name], previous_attributes, Some(merged));

        Ok(())
    }
}
