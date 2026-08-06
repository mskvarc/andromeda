//! Replacing one attribute wholesale (clause 5.6.19).

use crate::{
    core::error::AndromedaError,
    service::entity::{
        EntityService,
        attribute_envelope::validate_attribute_envelopes,
        attrs::attribute_write::{MutatedEntity, payload_write},
        expanded_member::{attr_name_of, unwrap_expanded_ref},
        history_record::HistoryRecord,
    },
};
use andromeda_jsonld::context_spec::ContextSpec;
use andromeda_ngsi_ld_lexicon::uri::Uri;
use andromeda_ngsi_ld_model::{
    expanded::{entity_member::extract_types, mutation::attribute::replace_attr},
    notification::NotificationTrigger,
};
use andromeda_ngsi_ld_vocabulary::expanded::properties as expanded_properties;
use serde_json::Value;
use std::sync::Arc;
use urn_rs::Urn;

impl EntityService {
    /// Replace one attribute of an entity wholesale (clause 5.6.19).
    ///
    /// The replacement is computed from a payload read outside the writing
    /// transaction, so the write states the `modifiedAt` it assumed and is
    /// recomputed against a fresh read if a concurrent writer got in first.
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

        let expanded = Arc::new(self.engine.expand(fragment, context).await?);
        validate_attribute_envelopes(&expanded)?;

        let MutatedEntity {
            merged,
            previous: previous_attributes,
            trigger,
        } = self
            .commit_computed_write(&id, |previous| {
                let previous_attributes = self.change_tx.as_ref().map(|_| previous.clone()); // clone: snapshot before mutation for showChanges diff

                let mut existing = previous.clone(); // clone: the replacement mutates its own copy of the stored payload
                replace_attr(&mut existing, &attr_iri, dataset_id, unwrap_expanded_ref(&expanded))?;

                let merged = Arc::new(existing);
                let write = payload_write(&id, &merged, vec![HistoryRecord::Observations(Arc::clone(&expanded))]); // clone: Arc ref-count bump
                Ok((
                    write,
                    MutatedEntity {
                        merged,
                        previous: previous_attributes,
                        trigger: NotificationTrigger::AttributeUpdated,
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
