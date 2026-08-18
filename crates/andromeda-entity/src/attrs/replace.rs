//! Replacing one attribute wholesale (clause 5.6.19).

use crate::{
    EntityService,
    attribute_envelope::validate_attribute_envelopes,
    attrs::attribute_write::{MutatedEntity, payload_write},
    error::EntityError,
    expanded_member::unwrap_expanded_ref,
    geo_property_geometry::validate_geo_property_geometries,
    notification_plan::attribute_change_groups,
};
use andromeda_context_commit::history_record::HistoryRecord;
use andromeda_jsonld::context_spec::ContextSpec;
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, uri::Uri};
use andromeda_ngsi_ld_model::expanded::{entity_member::extract_types, mutation::attribute::replace_attr};
use andromeda_ngsi_ld_vocabulary::expanded::properties as expanded_properties;
use serde_json::Value;
use std::sync::Arc;

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

        let MutatedEntity { events } = self
            .commit_computed_write(&entity_uri, |previous| {
                let mut existing = previous.clone(); // clone: the replacement mutates its own copy of the stored payload
                replace_attr(&mut existing, &attr_iri, dataset_id, unwrap_expanded_ref(&expanded))?;

                let merged = Arc::new(existing);
                let types = extract_types(&merged);

                let groups = attribute_change_groups(previous, &merged);
                let plan = self.plan_notifications(&entity_uri, &types, &groups, Some(&merged), Some(previous));

                let write = payload_write(&entity_uri, &merged, vec![HistoryRecord::Observations(Arc::clone(&expanded))], plan.matches); // clone: Arc ref-count bump
                Ok((write, MutatedEntity { events: plan.events }))
            })
            .await?;

        events.into_iter().for_each(|event| self.emit_change(event));

        Ok(())
    }
}
