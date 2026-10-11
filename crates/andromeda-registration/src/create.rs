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
    /// [`RegistrationError::JsonLd`] when the request context cannot be
    /// processed or the body cannot be expanded, [`RegistrationError::Internal`]
    /// when no identifier could be minted for an anonymous registration,
    /// and [`RegistrationError::Persistence`] when the store rejects the
    /// write.
    pub async fn create(&self, body: RegistrationBody, context: &AdmittedContext) -> Result<Identifier, RegistrationError> {
        // Counted before the body is expanded.
        admit_registration_geometries(body.geometries(), self.geo_admission.limits)?;
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

        // Clause 5.5.7: a context that does not process refuses the
        // registration before any of its names is expanded under it.
        self.engine.expansion_context(context).await?;

        let properties = expand_context_source_properties(&self.engine, take(&mut body.additional_properties), context).await?;
        if let Some(information) = body.information.as_set_mut() {
            self.expand_information(information, context).await?;
        }

        let mut params = body.into_create_params()?;
        params.additional_properties = properties;
        validate_shape(&RegistrationShape {
            mode: params.mode,
            information: &params.information,
            operations: &params.operations,
            context_source_info: params.context_source_info.as_deref(),
            expires_at: params.expires_at,
        })?;
        check_conflicts(self.repo.as_ref(), self.entity_probe.as_ref(), params.mode, &params.information, None).await?;
        let written = self.repo.create(&id, &params).await?;
        self.emit_csr_change(CsrChangeEvent {
            csr_id: id.clone(), // clone: the change event and response each own the identifier.
            change: CsrChange::Created {
                written: Box::new(written.registration),
            },
            revision: written.revision,
        });
        Ok(id)
    }
}
