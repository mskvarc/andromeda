//! JSON-LD expansion of registration information entries.
//!
//! The reverse direction is `crate::information_compaction`, which every
//! rendering of a registration shares.

use crate::{RegistrationService, error::RegistrationError, registration_validation_reason::RegistrationValidationReason};
use andromeda_jsonld::{context_spec::ContextSpec, engine::term_lookup::PreparedTermContext};
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, entity_type_name::EntityTypeName, reference::NgsiLdReferenceError};
use andromeda_ngsi_ld_model::{entity_info::EntityTypeSelection, registration::info::RegistrationInfo};
use std::str::FromStr;

impl RegistrationService {
    /// Expand the entity types and attribute names of `information` under
    /// `context`, keeping the shape each `type` was written in.
    ///
    /// # Errors
    ///
    /// Returns [`RegistrationError::JsonLd`] when the context cannot be
    /// processed, and [`RegistrationValidationReason::ExpandedName`] when a term
    /// expands to a name NGSI-LD cannot carry.
    pub(super) async fn expand_information(&self, information: &mut [RegistrationInfo], context: &ContextSpec) -> Result<(), RegistrationError> {
        let prepared_context = self.engine.prepare_term_context(context);
        for entry in information {
            for entity in entry.entities.iter_mut().flatten() {
                self.expand_entity_type(&mut entity.type_, &prepared_context).await?;
            }
            for names in [entry.property_names.as_mut(), entry.relationship_names.as_mut()].into_iter().flatten() {
                for name in names {
                    let expanded = self.engine.expand_term(name.as_str(), &prepared_context).await?;
                    *name = AttributeName::from_str(expanded.as_str()).map_err(expanded_name)?;
                }
            }
        }
        Ok(())
    }

    /// Expand an `EntityInfo` `type`, which Table 5.2.8-1 lets be one name or
    /// an array of names; the shape is kept so it reads back as written.
    async fn expand_entity_type(&self, selection: &mut EntityTypeSelection, context: &PreparedTermContext) -> Result<(), RegistrationError> {
        match selection {
            EntityTypeSelection::Single(name) => self.expand_type_name(name, context).await,
            EntityTypeSelection::Several(names) => {
                for name in names {
                    self.expand_type_name(name, context).await?;
                }
                Ok(())
            }
        }
    }

    /// Expand one entity type name in place.
    async fn expand_type_name(&self, name: &mut EntityTypeName, context: &PreparedTermContext) -> Result<(), RegistrationError> {
        let expanded = self.engine.expand_type(name.as_str(), context).await?;
        *name = EntityTypeName::from_str(expanded.as_str()).map_err(expanded_name)?;
        Ok(())
    }
}

/// The refusal of a term that expands to a name NGSI-LD cannot carry.
const fn expanded_name(source: NgsiLdReferenceError) -> RegistrationError {
    RegistrationError::Validation(RegistrationValidationReason::ExpandedName { source })
}

#[cfg(test)]
mod tests {
    use crate::{RegistrationService, entity_probe::EntityPresence, fixed_probe::FixedProbe, fixed_store::FixedStore, store::RegistrationStore};
    use andromeda_json_literal::JsonLiteral;
    use andromeda_jsonld::{context_spec::ContextSpec, engine::processor::JsonLdEngine};
    use andromeda_ngsi_ld_geometry::geometry_census::{GeometryCensusLimits, GeometryPositionLimit};
    use andromeda_ngsi_ld_model::registration::info::RegistrationInfo;
    use andromeda_ngsi_ld_number::json_value::value_census::JsonValueLimit;
    use andromeda_ngsi_ld_query::geo::{admission::GeoQueryAdmission, near_distance_band::NearDistanceBand};
    use serde_json::json;
    use std::{num::NonZeroUsize, sync::Arc};

    fn service() -> RegistrationService {
        let engine = JsonLdEngine::builder().allow_remote(false).build().expect("engine builds");
        RegistrationService::new(
            Arc::new(FixedStore::new(Vec::new())) as Arc<dyn RegistrationStore>,
            Arc::new(FixedProbe::new(EntityPresence::Absent, EntityPresence::Absent)),
            engine,
            None,
            GeoQueryAdmission::new(
                GeometryCensusLimits {
                    positions: GeometryPositionLimit::new(NonZeroUsize::MIN.saturating_add(999_999)),
                    values: JsonValueLimit::new(NonZeroUsize::MIN.saturating_add(999_999)),
                },
                NearDistanceBand::Refused,
            ),
        )
    }

    /// Clause 5.5.7: the terms of `information` expand under the request's
    /// context, a `type` keeping the shape it was written in.
    #[tokio::test]
    async fn the_terms_of_information_expand_under_the_request_context_keeping_the_type_shape() {
        let context = ContextSpec::from_body(
            JsonLiteral::admit(
                &json!({
                    "Vehicle": "https://example.org/Vehicle",
                    "Car": "https://example.org/Car",
                    "speed": "https://example.org/speed",
                    "owner": "https://example.org/owner"
                })
                .to_string(),
            )
            .expect("an admissible literal"),
        );
        let mut information: Vec<RegistrationInfo> = serde_json::from_value(json!([
            {"entities": [{"type": "Vehicle"}, {"type": ["Vehicle", "Car"], "id": "urn:ngsi-ld:Car:1"}], "propertyNames": ["speed"], "relationshipNames": ["owner"]}
        ]))
        .expect("information");

        service().expand_information(&mut information, &context).await.expect("the terms expand");

        pretty_assertions::assert_eq!(
            serde_json::to_value(&information).expect("information writes"),
            json!([{
                "entities": [
                    {"type": "https://example.org/Vehicle"},
                    {"type": ["https://example.org/Vehicle", "https://example.org/Car"], "id": "urn:ngsi-ld:Car:1"}
                ],
                "propertyNames": ["https://example.org/speed"],
                "relationshipNames": ["https://example.org/owner"]
            }])
        );
    }
}
