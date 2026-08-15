//! Replacing one attribute wholesale (clause 5.6.19).

use crate::{
    EntityService,
    attribute_envelope::validate_attribute_envelopes,
    attrs::attribute_write::{MutatedEntity, payload_write},
    error::EntityError,
    expanded_member::{attr_name_of, unwrap_expanded_ref},
    geo_property_geometry::validate_geo_property_geometries,
};
use andromeda_context_commit::history_record::HistoryRecord;
use andromeda_jsonld::context_spec::ContextSpec;
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, uri::Uri};
use andromeda_ngsi_ld_model::{
    expanded::{entity_member::extract_types, mutation::attribute::replace_attr},
    notification::NotificationTrigger,
};
use andromeda_ngsi_ld_vocabulary::expanded::properties as expanded_properties;
use serde_json::Value;
use std::{slice::from_ref, sync::Arc};

impl EntityService {
    /// Replace one attribute of an entity wholesale (clause 5.6.19).
    ///
    /// The replacement is computed from a payload read outside the writing
    /// transaction, so the write states the `modifiedAt` it assumed and is
    /// recomputed against a fresh read if a concurrent writer got in first.
    pub async fn replace_attr(
        &self,
        id: &Uri,
        attr_name: &AttributeName,
        dataset_id: Option<&Uri>,
        fragment: Value,
        context: &ContextSpec,
    ) -> Result<(), EntityError> {
        let entity_uri = id.clone(); // clone: notification and computed write own the identifier

        if attr_name.as_str().starts_with('@') {
            return Err(EntityError::validation("Invalid attribute name"));
        }

        let prepared_context = self.engine.prepare_term_context(context);
        let attr_iri = self.engine.expand_term(attr_name.as_str(), &prepared_context).await?;

        if attr_iri.as_str() == expanded_properties::SCOPE.as_str() {
            return Err(EntityError::validation("Scope is not a replaceable attribute"));
        }

        let expanded = Arc::new(self.engine.expand(fragment, context).await?);
        validate_attribute_envelopes(&expanded)?;
        validate_geo_property_geometries(&expanded)?;

        let expanded_attr_name = attr_name_of(attr_iri.as_str())?;

        let MutatedEntity {
            merged,
            types: entity_types,
            previous: previous_attributes,
            trigger,
        } = self
            .commit_computed_write(&entity_uri, |previous| {
                let previous_attributes = self.change_sink.as_ref().map(|_| previous.clone()); // clone: snapshot before mutation for showChanges diff

                let mut existing = previous.clone(); // clone: the replacement mutates its own copy of the stored payload
                replace_attr(&mut existing, &attr_iri, dataset_id, unwrap_expanded_ref(&expanded))?;

                let merged = Arc::new(existing);
                let types = extract_types(&merged);

                // The durable match mirrors the single Live `emit_attr_change`
                // below: `AttributeUpdated` for the one attribute this call
                // replaced, with the same before-snapshot.
                let subscription_matches = self.build_subscription_matches(
                    &entity_uri,
                    &types,
                    NotificationTrigger::AttributeUpdated,
                    from_ref(&expanded_attr_name),
                    Some(&merged),
                    previous_attributes.as_ref(),
                );

                let write = payload_write(
                    &entity_uri,
                    &merged,
                    vec![HistoryRecord::Observations(Arc::clone(&expanded))],
                    subscription_matches,
                ); // clone: Arc ref-count bump
                Ok((
                    write,
                    MutatedEntity {
                        merged,
                        types,
                        previous: previous_attributes,
                        trigger: NotificationTrigger::AttributeUpdated,
                    },
                ))
            })
            .await?;

        self.emit_attr_change(entity_uri, entity_types, trigger, vec![expanded_attr_name], previous_attributes, Some(merged));

        Ok(())
    }
}
