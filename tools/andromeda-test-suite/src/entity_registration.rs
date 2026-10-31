//! The registration most context source tests start from: one information element naming an entity type and an
//! `idPattern`, and an endpoint the test chooses.

use ngsi_ld::{
    model::{BuildError, EntityTypeName, Input},
    query::IdPattern,
    registration::{CSourceRegistration, EntityInfo, Problem, RegistrationId, RegistrationInfo},
};

/// The registration `id` whose one information element names `entity_type` and `id_pattern`, served at `endpoint`.
///
/// # Errors
///
/// Returns [`BuildError`] when the endpoint is not a URL or the information is not valid.
pub fn entity_registration(
    id: &RegistrationId,
    entity_type: &EntityTypeName,
    id_pattern: IdPattern,
    endpoint: &str,
) -> Result<CSourceRegistration<Input>, BuildError<Problem>> {
    let information = RegistrationInfo::builder()
        .entities([EntityInfo::new(entity_type.clone()).with_id_pattern(id_pattern)])
        .build()?;
    CSourceRegistration::builder(endpoint).id(id.clone()).information([information]).build()
}

#[cfg(test)]
mod tests {
    use crate::{entity_registration::entity_registration, namespace::Namespace};
    use googletest::prelude::*;

    #[gtest]
    fn the_registration_names_the_type_the_pattern_and_the_endpoint() -> Result<()> {
        let namespace = Namespace::fresh();
        let id = namespace.registration_id(1)?;
        let entity_type = namespace.entity_type("Building")?;
        let registration = entity_registration(&id, &entity_type, namespace.id_pattern("Building")?, "http://source.test/")?;
        verify_that!(registration.information(), len(eq(1)))
    }
}
