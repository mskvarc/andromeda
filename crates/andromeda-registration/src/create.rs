//! Context-source registration creation policy (ETSI GS CIM 009 clause 5.9.2).

use crate::{
    RegistrationService,
    context_source_properties::expand_context_source_properties,
    error::RegistrationError,
    geometry::admit_registration_geometries,
    identifier::generate_registration_id,
    mode_rules::check_conflicts,
    registration_body::RegistrationBody,
    validation::{RegistrationShape, validate_shape},
};
use andromeda_jsonld::user_context_admission::AdmittedContext;
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_ngsi_ld_model::notification::{CsrChange, CsrChangeEvent};
use std::mem::take;

impl RegistrationService {
    /// Create a context-source registration from its request body.
    ///
    /// # Errors
    ///
    /// Returns [`RegistrationError::Validation`] when a member of the body
    /// falls outside Table 5.2.9-1, [`RegistrationError::AlreadyExists`] when
    /// the body names an identifier the store already holds,
    /// [`RegistrationError::GeometryTooLarge`] when a geometry holds more than
    /// the broker accepts, [`RegistrationError::Conflict`]
    /// when a proxied footprint overlaps an existing registration or context
    /// data the broker already holds (clause 5.9.2.4),
    /// [`RegistrationError::JsonLd`] when the body cannot be expanded or the
    /// `contextSourceInfo` `jsonldContext` cannot be retrieved or processed,
    /// [`RegistrationError::Internal`]
    /// when no identifier could be minted for an anonymous registration,
    /// and [`RegistrationError::Persistence`] when the store rejects the
    /// write.
    pub async fn create(&self, body: RegistrationBody, context: &AdmittedContext) -> Result<Identifier, RegistrationError> {
        // Counted before the body is expanded.
        admit_registration_geometries(body.geometries(), self.geo_admission.limits)?;
        body.refuse_ngsi_ld_null_properties()?;
        let mut body = body;
        let id = match body.id.take() {
            Some(id) => {
                if self.repo.exists(&id).await? {
                    return Err(RegistrationError::already_exists(&id));
                }
                id
            }
            None => generate_registration_id()?,
        };

        let properties = expand_context_source_properties(&self.engine, take(&mut body.additional_properties), context).await?;
        if let Some(information) = body.information.as_set_mut() {
            self.expand_information(information, context).await?;
        }

        let mut params = body.into_create_params()?;
        params.additional_properties = properties;
        let source_info = validate_shape(&RegistrationShape {
            mode: params.mode,
            information: &params.information,
            operations: &params.operations,
            context_source_info: params.context_source_info.as_deref(),
            expires_at: params.expires_at,
        })?;
        self.admit_forwarding_context(&source_info).await?;
        check_conflicts(self.repo.as_ref(), self.entity_probe.as_ref(), params.mode, &params.information, None).await?;
        // Counted before the write is sent, so a registration is never held
        // uncounted. A duplicate is refused before anything is written, so its
        // count is given back; any other failure may have committed.
        self.census.admit();
        let written = self.repo.create(&id, &params).await.inspect_err(|error| {
            if let RegistrationError::AlreadyExists { .. } = error {
                self.census.release(1);
            }
        })?;
        self.emit_csr_change(CsrChangeEvent {
            csr_id: id.clone(),
            change: CsrChange::Created {
                written: Box::new(written.registration),
            },
            revision: written.revision,
        })
        .await;
        Ok(id)
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        RegistrationService,
        error::RegistrationError,
        fixed_store::{CreationAnswer, FixedStore},
        registration_body::RegistrationBody,
        registration_census::RegistrationCensus,
        service_fixture::{offline_engine, service_counting},
        store::RegistrationStore,
    };
    use andromeda_jsonld::{
        context_spec::ContextSpec,
        user_context_admission::{SubmittedContext, admit_user_context},
    };
    use andromeda_ngsi_ld_model::request_body::RequestBody;
    use andromeda_ngsi_ld_representation::compact_node::context_reading::ContextReading;
    use pretty_assertions::assert_eq;
    use serde_json::json;
    use std::sync::Arc;

    /// A service over a store answering creations as `creation` says, counting
    /// its registrations in `census`.
    fn service(creation: CreationAnswer, census: &RegistrationCensus) -> RegistrationService {
        let store = FixedStore::new(Vec::new()).answering_creations(creation);
        service_counting(Arc::new(store) as Arc<dyn RegistrationStore>, census.clone(), offline_engine(), None)
    }

    /// Create a registration of Vehicles through `service`.
    async fn create(service: &RegistrationService) -> Result<(), RegistrationError> {
        let document = json!({
            "id": "urn:ngsi-ld:ContextSourceRegistration:counted",
            "type": "ContextSourceRegistration",
            "endpoint": "https://peer.example.org",
            "information": [{"entities": [{"type": "https://example.org/Vehicle"}]}]
        });
        let body = RegistrationBody::from_request_text(document.to_string().as_bytes(), ContextReading::keyword_spellings()).expect("a registration body");
        let context = admit_user_context(service.engine(), SubmittedContext::from(ContextSpec::none()))
            .await
            .expect("the core context is admitted");
        service.create(body, &context).await.map(drop)
    }

    #[tokio::test]
    async fn a_created_registration_is_counted() {
        let census = RegistrationCensus::new(0);
        create(&service(CreationAnswer::Written, &census)).await.expect("the registration is created");
        assert!(!census.holds_none());
    }

    /// A lost uniqueness race refuses the insert, so nothing was written and
    /// the count goes back to what the store holds.
    #[tokio::test]
    async fn a_creation_the_store_refuses_as_a_duplicate_is_not_counted() {
        let census = RegistrationCensus::new(0);
        let outcome = create(&service(CreationAnswer::Duplicate, &census)).await;
        assert!(matches!(outcome, Err(RegistrationError::AlreadyExists { .. })), "{outcome:?}");
        assert!(census.holds_none());
    }

    /// The write may have committed before the store became unreachable, so it
    /// stays counted. The count may exceed what the store holds, never fall
    /// short of it.
    #[tokio::test]
    async fn a_creation_whose_outcome_is_unknown_stays_counted() {
        let census = RegistrationCensus::new(0);
        let outcome = create(&service(CreationAnswer::Unavailable, &census)).await;
        assert!(outcome.is_err(), "{outcome:?}");
        assert_eq!(census.release(0), 1);
    }
}
