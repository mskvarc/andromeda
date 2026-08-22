//! Context-source registration update policy (ETSI GS CIM 009 clause 5.9.3).

use crate::{
    RegistrationService,
    context_source_properties::expand_context_source_properties,
    error::RegistrationError,
    mode_rules::check_conflicts,
    payload::extract_update_fragment,
    resolved_update::apply_fragment,
    validation::{RegistrationShape, validate_shape},
};
use andromeda_jsonld::context_spec::ContextSpec;
use andromeda_ngsi_ld_lexicon::uri::Uri;
use andromeda_ngsi_ld_model::{
    notification::{CsrChangeEvent, CsrTriggerReason},
    registration::ContextSourceRegistrationType,
};
use serde_json::Value;

impl RegistrationService {
    /// Merge an update fragment into an existing registration.
    pub async fn update(&self, id: &Uri, payload: Value, context: &ContextSpec) -> Result<(), RegistrationError> {
        // A fragment may omit `type`, but one it carries has to be the
        // registration type (clause 5.2.9).
        if let Some(type_value) = payload.get("type") {
            serde_json::from_value::<ContextSourceRegistrationType>(type_value.clone()) // clone: serde owns the member while the fragment remains borrowed.
                .map_err(|_| RegistrationError::csr_validation("type must be 'ContextSourceRegistration'"))?;
        }
        if payload.get("information").is_some_and(Value::is_null) {
            return Err(RegistrationError::csr_validation("information must not be null"));
        }
        if payload.get("endpoint").is_some_and(Value::is_null) {
            return Err(RegistrationError::csr_validation("endpoint must not be null"));
        }

        let expanded_properties = expand_context_source_properties(&self.engine, &payload, context).await?;
        let mut payload = payload;
        if let Some(information) = payload.get_mut("information") {
            self.expand_information(information, context).await?;
        }

        let existing = self.repo.get(id).await?.ok_or_else(|| RegistrationError::not_found(id))?;
        let pre_update_snapshot =
            serde_json::to_value(&existing).map_err(|error| RegistrationError::internal(format!("Failed to serialize the pre-update CSR: {error}")))?;
        let mut fragment = extract_update_fragment(&payload)?;
        fragment.additional_properties = expanded_properties;
        let resolved = apply_fragment(existing, &fragment)?;
        // Clause 5.9.3.4 evaluates the rules against the registration as it
        // will be after the merge, not against the fragment alone.
        validate_shape(&RegistrationShape {
            mode: resolved.mode,
            information: &resolved.information,
            operations: &resolved.operations,
            context_source_info: resolved.context_source_info.as_deref(),
            expires_at: resolved.expires_at,
        })?;
        check_conflicts(self.repo.as_ref(), self.entity_probe.as_ref(), resolved.mode, &resolved.information, Some(id)).await?;
        self.repo.update(id, &resolved).await?.require_applied(id)?;

        self.emit_csr_change(CsrChangeEvent {
            csr_id: id.clone(), // clone: the event owns the identifier borrowed from the request path.
            trigger_reason: CsrTriggerReason::Updated,
            csr_snapshot: Some(pre_update_snapshot),
        });
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        RegistrationService,
        entity_probe::EntityPresence,
        error::RegistrationError,
        fixed_probe::FixedProbe,
        fixed_store::{FixedStore, registration},
        store::RegistrationStore,
    };
    use andromeda_jsonld::{context_spec::ContextSpec, engine::processor::JsonLdEngine};
    use andromeda_ngsi_ld_lexicon::uri::Uri;
    use iri_rs::IriRefBuf;
    use serde_json::json;
    use std::sync::Arc;

    const REGISTRATION_ID: &str = "urn:ngsi-ld:ContextSourceRegistration:update";

    /// A service over `store`, with remote context fetching disabled so a
    /// remote context can only fail to load.
    fn service(store: &Arc<FixedStore>) -> RegistrationService {
        let engine = JsonLdEngine::builder().allow_remote(false).build().expect("engine builds");
        RegistrationService::new(
            Arc::clone(store) as Arc<dyn RegistrationStore>, // clone: the test keeps its own handle to read what was written
            Arc::new(FixedProbe::new(EntityPresence::Absent, EntityPresence::Absent)),
            engine,
            None,
        )
    }

    fn registration_id() -> Uri {
        REGISTRATION_ID.parse().expect("valid URI")
    }

    fn baseline_store() -> Arc<FixedStore> {
        Arc::new(FixedStore::new(vec![registration(
            REGISTRATION_ID,
            "inclusive",
            json!([{"entities": [{"type": "https://example.org/Vehicle"}]}]),
        )]))
    }

    /// Clause 5.9.3 with clause 5.5.7: the short names of an update fragment
    /// expand under the request's context and the expanded IRIs are what the
    /// store is asked to write.
    #[tokio::test]
    async fn shorthand_names_in_the_fragment_expand_under_the_request_context() {
        let store = baseline_store();
        let service = service(&store);
        let context = ContextSpec::none().with_body_context(Some(json!({
            "Vehicle": "https://example.org/Vehicle",
            "speed": "https://example.org/speed"
        })));
        let fragment = json!({ "information": [ { "entities": [ { "type": "Vehicle" } ], "propertyNames": ["speed"] } ] });

        service.update(&registration_id(), fragment, &context).await.expect("the fragment applies");

        let written = store.updated_information();
        pretty_assertions::assert_eq!(written.len(), 1);
        let names: Vec<String> = written[0][0].property_names.iter().flatten().map(ToString::to_string).collect();
        pretty_assertions::assert_eq!(names, vec!["https://example.org/speed".to_string()]);
        let types: Vec<String> = written[0][0]
            .entities
            .iter()
            .flatten()
            .map(|entity| entity.type_.as_type_expression().to_string())
            .collect();
        pretty_assertions::assert_eq!(types, vec!["https://example.org/Vehicle".to_string()]);
    }

    /// Clause 5.5.2: a request context that cannot be loaded is reported as
    /// `LdContextNotAvailable`; the fragment is never expanded against the
    /// core context alone and nothing is written.
    #[tokio::test]
    async fn an_unloadable_request_context_is_reported_rather_than_replaced_by_the_core_context() {
        let store = baseline_store();
        let service = service(&store);
        let context = ContextSpec::from_links(vec![
            IriRefBuf::new("https://contexts.example.org/unreachable.jsonld").expect("valid IRI reference"),
        ]);
        let fragment = json!({ "information": [ { "entities": [ { "type": "Vehicle" } ], "propertyNames": ["speed"] } ] });

        let error = service
            .update(&registration_id(), fragment, &context)
            .await
            .expect_err("the context load fails");

        assert!(
            matches!(&error, RegistrationError::JsonLd(source) if source.is_context_not_available()),
            "expected a context-not-available failure, got {error:?}"
        );
        assert!(store.updated_information().is_empty(), "nothing is written when the context does not load");
    }

    /// The fragment is expanded before the registration is looked up, so an
    /// unknown identifier under a loadable context is a plain not-found.
    #[tokio::test]
    async fn an_unknown_registration_under_a_loadable_context_is_not_found() {
        let service = service(&Arc::new(FixedStore::new(Vec::new())));
        let fragment = json!({ "information": [ { "entities": [ { "type": "Vehicle" } ], "propertyNames": ["speed"] } ] });

        let error = service
            .update(&registration_id(), fragment, &ContextSpec::none())
            .await
            .expect_err("the registration is unknown");

        assert!(matches!(error, RegistrationError::NotFound { .. }), "{error:?}");
    }
}
