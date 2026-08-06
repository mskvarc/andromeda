//! Appending attributes to an entity (clause 5.6.3).

use crate::{
    core::error::AndromedaError,
    service::entity::{
        EntityService,
        attribute_envelope::validate_attribute_envelopes,
        attrs::attribute_write::payload_write,
        expanded_member::{extract_attribute_keys, unwrap_expanded_ref},
        history_record::HistoryRecord,
    },
};
use andromeda_jsonld::context_spec::ContextSpec;
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, uri::Uri};
use andromeda_ngsi_ld_model::{
    expanded::{
        entity_member::extract_types,
        mutation::{entity::append_entity_attrs, selection::OverwritePolicy},
    },
    notification::{ChangeEvent, NotificationTrigger},
};
use andromeda_ngsi_ld_representation::normalization::normalize_entity_attributes;
use serde_json::Value;
use std::sync::Arc;
use urn_rs::Urn;

impl EntityService {
    /// Append attributes to an existing entity (clause 5.6.3).
    ///
    /// Returns `(all_attr_iris, not_appended_iris)`:
    /// - all attribute IRIs the entity carries once the append has landed
    /// - attribute IRIs that were NOT appended, because `noOverwrite` was set
    ///   and the instance already existed
    ///
    /// The append is computed from a payload read outside the writing
    /// transaction, so the write states the `modifiedAt` it assumed and is
    /// recomputed against a fresh read if a concurrent writer got in first.
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
        let expanded = Arc::new(self.engine.expand(fragment, context).await?);
        validate_attribute_envelopes(&expanded)?;

        let (merged, not_appended) = self
            .commit_computed_write(&id, |previous| {
                let mut existing = previous.clone(); // clone: the append mutates its own copy of the stored payload
                let not_appended = append_entity_attrs(&mut existing, unwrap_expanded_ref(&expanded), overwrite)?;
                let merged = Arc::new(existing);
                let write = payload_write(&id, &merged, vec![HistoryRecord::Observations(Arc::clone(&expanded))]); // clone: Arc ref-count bump
                Ok((write, (merged, not_appended)))
            })
            .await?;

        let entity_types = extract_types(&merged);
        let attr_keys = extract_attribute_keys(&merged);
        self.emit_change(
            ChangeEvent::new(entity_uri, entity_types, NotificationTrigger::AttributeCreated)
                .with_changed_attributes(attr_keys.clone()) // clone: the same names are also this operation's return value
                .with_current_payload(merged),
        );

        Ok((attr_keys, not_appended))
    }
}
