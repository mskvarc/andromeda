//! Partially updating one attribute instance (clause 5.6.4).

use crate::{
    core::error::AndromedaError,
    service::entity::{
        EntityService,
        attribute_envelope::validate_attribute_envelopes,
        attrs::attribute_write::{MutatedEntity, payload_write},
        expanded_member::{attr_name_of, extract_attribute_keys, unwrap_expanded_ref},
        history_record::{HistoryRecord, tombstone},
    },
};
use andromeda_jsonld::context_spec::ContextSpec;
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, uri::Uri};
use andromeda_ngsi_ld_model::{
    expanded::{entity_member::extract_types, mutation::attribute::partial_update_attr},
    notification::NotificationTrigger,
};
use serde_json::Value;
use std::{collections::HashSet, sync::Arc};
use urn_rs::Urn;

impl EntityService {
    /// Apply a partial update to one attribute instance (clause 5.6.4).
    ///
    /// The update is computed from a payload read outside the writing
    /// transaction, so the write states the `modifiedAt` it assumed and is
    /// recomputed against a fresh read if a concurrent writer got in first.
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

        let expanded = Arc::new(self.engine.expand(fragment, context).await?);
        validate_attribute_envelopes(&expanded)?;

        let MutatedEntity {
            merged,
            previous: previous_attributes,
            trigger,
        } = self
            .commit_computed_write(&id, |previous| {
                let previous_attributes = self.change_tx.as_ref().map(|_| previous.clone()); // clone: snapshot before mutation for showChanges diff
                let attrs_before: HashSet<AttributeName> = extract_attribute_keys(previous).into_iter().collect();

                let mut existing = previous.clone(); // clone: the update mutates its own copy of the stored payload
                partial_update_attr(&mut existing, &attr_iri, dataset_id, unwrap_expanded_ref(&expanded))?;

                // A null sentinel in the fragment deletes the attribute rather
                // than updating it, which the diff is the only way to tell.
                let attrs_after: HashSet<AttributeName> = extract_attribute_keys(&existing).into_iter().collect();
                let was_deleted = attrs_before.contains(attr_iri.as_str()) && !attrs_after.contains(attr_iri.as_str());

                let (history, trigger) = if was_deleted {
                    (vec![tombstone(&attr_iri, dataset_id)?], NotificationTrigger::AttributeDeleted)
                } else {
                    (
                        vec![HistoryRecord::Observations(Arc::clone(&expanded))], // clone: Arc ref-count bump
                        NotificationTrigger::AttributeUpdated,
                    )
                };

                let merged = Arc::new(existing);
                let write = payload_write(&id, &merged, history);
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
            vec![attr_name_of(&attr_iri)?],
            previous_attributes,
            Some(merged),
        );

        Ok(())
    }
}
