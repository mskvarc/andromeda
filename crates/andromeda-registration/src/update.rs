//! Context-source registration update policy (ETSI GS CIM 009 clause 5.9.3).

use crate::{
    RegistrationService,
    context_source_properties::expand_context_source_properties,
    error::RegistrationError,
    geometry::admit_registration_geometries,
    mode_rules::check_conflicts,
    registration_body::RegistrationBody,
    resolved_update::apply_fragment,
    validation::{RegistrationShape, validate_shape},
};
use andromeda_jsonld::user_context_admission::AdmittedContext;
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_ngsi_ld_model::notification::{CsrChangeEvent, CsrTriggerReason};
use std::mem::take;

impl RegistrationService {
    /// Merge an update fragment into an existing registration.
    ///
    /// # Errors
    ///
    /// Returns [`RegistrationError::Validation`] when the fragment nulls a
    /// member clause 5.5.8 does not allow to be removed, or leaves the merged
    /// registration outside Table 5.2.9-1,
    /// [`RegistrationError::GeometryTooLarge`] when a geometry holds more than
    /// the broker accepts, [`RegistrationError::NotFound`] when no
    /// registration carries `id`, [`RegistrationError::Conflict`] when the
    /// merged footprint overlaps an existing registration or local context
    /// data (clause 5.9.3.4), [`RegistrationError::JsonLd`] when the fragment
    /// cannot be expanded, and [`RegistrationError::Persistence`] when the
    /// store rejects the read or the write.
    pub async fn update(&self, id: &Identifier, body: RegistrationBody, context: &AdmittedContext) -> Result<(), RegistrationError> {
        // Counted before the fragment is expanded.
        admit_registration_geometries(body.geometries(), self.geometry_limits)?;
        let mut body = body;
        let properties = expand_context_source_properties(&self.engine, take(&mut body.additional_properties), context).await?;
        if let Some(information) = body.information.as_set_mut() {
            self.expand_information(information, context).await?;
        }

        let mut fragment = body.into_update_fragment()?;
        fragment.additional_properties = properties;
        let existing = self.repo.get(id).await?.ok_or_else(|| RegistrationError::not_found(id))?;
        // Nothing is modified, so `modifiedAt` stays and no change is announced (clause 4.8).
        if fragment.is_empty() {
            return Ok(());
        }
        // clone: the change event carries the registration as it stood, and the merge consumes it
        let pre_update_snapshot = Box::new(existing.clone());
        let resolved = apply_fragment(existing, &fragment);
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
        let revision = self.repo.update(id, &resolved).await?.require_applied(id)?;

        self.emit_csr_change(CsrChangeEvent {
            csr_id: id.clone(), // clone: the event owns the identifier borrowed from the request path.
            trigger_reason: CsrTriggerReason::Updated,
            csr_snapshot: Some(pre_update_snapshot),
            revision,
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
        registration_body::RegistrationBody,
        store::RegistrationStore,
    };
    use andromeda_json_literal::JsonLiteral;
    use andromeda_jsonld::{
        context_spec::ContextSpec,
        engine::processor::JsonLdEngine,
        user_context_admission::{AdmittedContext, SubmittedContext, admit_user_context},
    };
    use andromeda_ngsi_ld_geometry::geometry_census::{GeometryCensusLimits, GeometryPositionLimit};
    use andromeda_ngsi_ld_lexicon::identifier::Identifier;
    use andromeda_ngsi_ld_model::request_body::RequestBody;
    use andromeda_ngsi_ld_number::json_value::value_census::JsonValueLimit;
    use serde_json::{Value, json};
    use std::{num::NonZeroUsize, sync::Arc};

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
            GeometryCensusLimits {
                positions: GeometryPositionLimit::new(NonZeroUsize::MIN.saturating_add(999_999)),
                values: JsonValueLimit::new(NonZeroUsize::MIN.saturating_add(999_999)),
            },
        )
    }

    /// `context` as the request handler admits it.
    async fn admitted(service: &RegistrationService, context: ContextSpec) -> AdmittedContext {
        admit_user_context(service.engine(), SubmittedContext::from(context))
            .await
            .expect("the context is admitted")
    }

    fn registration_id() -> Identifier {
        REGISTRATION_ID.parse().expect("valid URI")
    }

    /// The body a request carrying `document` is decoded to.
    fn body_of(document: &Value) -> RegistrationBody {
        RegistrationBody::from_request_text(document.to_string().as_bytes()).expect("a registration body")
    }

    fn baseline_store() -> Arc<FixedStore> {
        Arc::new(FixedStore::new(vec![registration(
            REGISTRATION_ID,
            "inclusive",
            &json!([{"entities": [{"type": "https://example.org/Vehicle"}]}]),
        )]))
    }

    /// Clause 5.9.3 with clause 5.5.7: the short names of an update fragment
    /// expand under the request's context and the expanded IRIs are what the
    /// store is asked to write.
    #[tokio::test]
    async fn shorthand_names_in_the_fragment_expand_under_the_request_context() {
        let store = baseline_store();
        let service = service(&store);
        let context = ContextSpec::from_body(
            JsonLiteral::admit(
                &json!({
                    "Vehicle": "https://example.org/Vehicle",
                    "speed": "https://example.org/speed"
                })
                .to_string(),
            )
            .expect("an admissible literal"),
        );
        let fragment = json!({ "information": [ { "entities": [ { "type": "Vehicle" } ], "propertyNames": ["speed"] } ] });
        let context = admitted(&service, context).await;

        service
            .update(&registration_id(), body_of(&fragment), &context)
            .await
            .expect("the fragment applies");

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

    /// The fragment is expanded before the registration is looked up, so an
    /// unknown identifier under a loadable context is a plain not-found.
    #[tokio::test]
    async fn an_unknown_registration_under_a_loadable_context_is_not_found() {
        let service = service(&Arc::new(FixedStore::new(Vec::new())));
        let fragment = json!({ "information": [ { "entities": [ { "type": "Vehicle" } ], "propertyNames": ["speed"] } ] });

        let context = admitted(&service, ContextSpec::none()).await;
        let error = service
            .update(&registration_id(), body_of(&fragment), &context)
            .await
            .expect_err("the registration is unknown");

        assert!(matches!(error, RegistrationError::NotFound { .. }), "{error:?}");
    }

    /// Clause 5.5.8 acts only for each member of the Fragment, so the empty
    /// Fragment modifies nothing and the store is asked to write nothing.
    #[tokio::test]
    async fn the_empty_fragment_writes_nothing() {
        let store = baseline_store();
        let service = service(&store);
        let context = admitted(&service, ContextSpec::none()).await;

        service
            .update(&registration_id(), body_of(&json!({})), &context)
            .await
            .expect("the empty fragment applies");

        assert!(store.updated_information().is_empty());
    }

    #[tokio::test]
    async fn the_empty_fragment_on_an_unknown_registration_is_not_found() {
        let service = service(&Arc::new(FixedStore::new(Vec::new())));
        let context = admitted(&service, ContextSpec::none()).await;

        let error = service
            .update(&registration_id(), body_of(&json!({})), &context)
            .await
            .expect_err("the registration is unknown");

        assert!(matches!(error, RegistrationError::NotFound { .. }), "{error:?}");
    }
}
