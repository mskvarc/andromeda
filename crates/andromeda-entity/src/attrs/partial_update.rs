//! Partially updating one attribute instance (clause 5.6.4).

use crate::{
    EntityService,
    attribute_envelope::validate_attribute_envelopes,
    attrs::attribute_write::{MutatedEntity, payload_write},
    error::EntityError,
    expanded_member::{attr_name_of, extract_attribute_keys, unwrap_expanded_ref},
    geo_property_geometry::validate_geo_property_geometries,
};
use andromeda_context_commit::history_record::{HistoryRecord, tombstone};
use andromeda_jsonld::context_spec::ContextSpec;
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, uri::Uri};
use andromeda_ngsi_ld_model::{
    expanded::{entity_member::extract_types, mutation::attribute::partial_update_attr},
    notification::NotificationTrigger,
};
use serde_json::Value;
use std::{collections::HashSet, sync::Arc};

impl EntityService {
    /// Apply a partial update to one attribute instance (clause 5.6.4).
    ///
    /// The update is computed from a payload read outside the writing
    /// transaction, so the write states the `modifiedAt` it assumed and is
    /// recomputed against a fresh read if a concurrent writer got in first.
    pub async fn partial_update_attr(
        &self,
        id: &Uri,
        attr_name: &AttributeName,
        dataset_id: Option<&Uri>,
        fragment: Value,
        context: &ContextSpec,
    ) -> Result<(), EntityError> {
        let entity_uri = id.clone(); // clone: notification and computed write own the identifier

        let prepared_context = self.engine.prepare_term_context(context);
        let attr_iri = self.engine.expand_term(attr_name.as_str(), &prepared_context).await?;
        let expanded_attr_name = attr_name_of(attr_iri.as_str())?;

        let expanded = Arc::new(self.engine.expand(fragment, context).await?);
        validate_attribute_envelopes(&expanded)?;
        validate_geo_property_geometries(&expanded)?;

        let MutatedEntity {
            merged,
            previous: previous_attributes,
            trigger,
        } = self
            .commit_computed_write(&entity_uri, |previous| {
                let previous_attributes = self.change_sink.as_ref().map(|_| previous.clone()); // clone: snapshot before mutation for showChanges diff
                let attrs_before: HashSet<AttributeName> = extract_attribute_keys(previous).into_iter().collect();

                let mut existing = previous.clone(); // clone: the update mutates its own copy of the stored payload
                partial_update_attr(&mut existing, &attr_iri, dataset_id, unwrap_expanded_ref(&expanded))?;

                // A null sentinel in the fragment deletes the attribute rather
                // than updating it, which the diff is the only way to tell.
                let attrs_after: HashSet<AttributeName> = extract_attribute_keys(&existing).into_iter().collect();
                let was_deleted = attrs_before.contains(attr_iri.as_str()) && !attrs_after.contains(attr_iri.as_str());

                let (history, trigger) = if was_deleted {
                    (vec![tombstone(&expanded_attr_name, dataset_id)], NotificationTrigger::AttributeDeleted)
                } else {
                    (
                        vec![HistoryRecord::Observations(Arc::clone(&expanded))], // clone: Arc ref-count bump
                        NotificationTrigger::AttributeUpdated,
                    )
                };

                let merged = Arc::new(existing);
                let write = payload_write(&entity_uri, &merged, history);
                Ok((
                    write,
                    MutatedEntity {
                        merged,
                        previous: previous_attributes,
                        trigger,
                    },
                ))
            })
            .await?;

        let entity_types = extract_types(&merged);
        self.emit_attr_change(
            entity_uri,
            entity_types,
            trigger,
            vec![attr_name_of(attr_iri.as_str())?],
            previous_attributes,
            Some(merged),
        );

        Ok(())
    }
}
