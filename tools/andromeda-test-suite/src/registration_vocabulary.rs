//! The `@context` the registration tests store before they register: the twelve terms of the Vehicle, parking and
//! Building vocabulary under the test's own IRIs, followed by the Core `@context`.

use crate::{
    core_context::{CoreContextError, core_entry},
    namespace::{Namespace, NamespaceError},
};
use ngsi_ld::{
    model::{BuildError, Context, ContextEntry, DateTime, InlineContext, InlineContextError, Input},
    query::{IdPattern, RegularExpressionError},
    registration::{CSourceRegistration, EntityInfo, Problem, RegistrationId, RegistrationInfo},
};

/// Why the vocabulary could not be built.
#[derive(Debug, thiserror::Error)]
pub enum VocabularyError {
    /// The inline definition is not a valid `@context`.
    #[error("the vocabulary is not a valid inline context")]
    Inline(#[from] InlineContextError),
    /// The Core `@context` is not configured.
    #[error("the Core context is not available")]
    Core(#[from] CoreContextError),
    /// An identifier or type of the registration is not valid.
    #[error("an identifier of the registration is not valid")]
    Namespace(#[from] NamespaceError),
    /// An `idPattern` of the registration is not a regular expression.
    #[error("an idPattern is not a regular expression")]
    Pattern(#[from] RegularExpressionError),
    /// The registration is not valid.
    #[error("the registration is not valid")]
    Registration(#[from] BuildError<Problem>),
}

/// The terms of the vocabulary: each is the IRI `https://test.andromeda/<token>/<name>` of the test, and the types
/// carry the token as their suffix.
const TERMS: [(&str, &str); 12] = [
    ("Vehicle", "Vehicle"),
    ("brandName", "brandName"),
    ("speed", "speed"),
    ("isParked", "isParked"),
    ("OffStreetParking", "OffStreetParking"),
    ("availableSpotNumber", "availableSpotNumber"),
    ("totalSpotNumber", "totalSpotNumber"),
    ("isNextToBuilding", "isNextToBuilding"),
    ("Building", "Building"),
    ("name", "name"),
    ("subCategory", "subCategory"),
    ("locatedAt", "locatedAt"),
];

/// The vocabulary of `namespace` as a context of two entries: the inline terms, then the Core `@context`.
///
/// # Errors
///
/// Returns [`VocabularyError`] when the definition is not valid or the Core `@context` is not configured.
pub fn registration_vocabulary(namespace: &Namespace) -> Result<Context, VocabularyError> {
    let members = TERMS
        .iter()
        .map(|(term, name)| {
            let key = if matches!(*term, "Vehicle" | "OffStreetParking" | "Building") {
                format!("{term}_{namespace}")
            } else {
                (*term).to_owned()
            };
            format!(r#""{key}": "https://test.andromeda/{namespace}/{name}""#)
        })
        .collect::<Vec<_>>()
        .join(", ");
    let inline = format!("{{{members}}}").parse::<InlineContext>()?;
    Ok(Context::Multiple(vec![ContextEntry::Inline(inline), core_entry()?]))
}

/// The registration `id` the update tests start from: a Vehicle named by identifier and two parking selectors by
/// `idPattern`, each with its attribute names, served at `http://my.csource.org:1026/`, expiring at `expires_at` when
/// one is given.
///
/// # Errors
///
/// Returns [`VocabularyError`] when a part of the registration is not valid.
pub fn vehicle_and_parking_registration(
    namespace: &Namespace,
    id: &RegistrationId,
    expires_at: Option<DateTime>,
) -> Result<CSourceRegistration<Input>, VocabularyError> {
    let parking_type = namespace.entity_type("OffStreetParking")?;
    let vehicle = RegistrationInfo::builder()
        .entities([EntityInfo::new(namespace.entity_type("Vehicle")?).with_id(namespace.urn("Vehicle", "A456")?)])
        .property_names(["brandName", "speed"])
        .relationship_names(["isParked"])
        .build()?;
    let parking = RegistrationInfo::builder()
        .entities([
            EntityInfo::new(parking_type.clone()).with_id_pattern(".*downtown$".parse::<IdPattern>()?),
            EntityInfo::new(parking_type).with_id_pattern(".*47$".parse::<IdPattern>()?),
        ])
        .property_names(["availableSpotNumber", "totalSpotNumber"])
        .relationship_names(["isNextToBuilding"])
        .build()?;
    Ok(CSourceRegistration::builder("http://my.csource.org:1026/")
        .id(id.clone())
        .information([vehicle, parking])
        .maybe_expires_at(expires_at)
        .build()?)
}

#[cfg(test)]
mod tests {
    use crate::{
        namespace::Namespace,
        registration_vocabulary::{registration_vocabulary, vehicle_and_parking_registration},
    };
    use googletest::prelude::*;
    use ngsi_ld::model::Context;

    #[gtest]
    fn the_registration_holds_a_vehicle_and_two_parking_selectors() -> Result<()> {
        let namespace = Namespace::fresh();
        let registration = vehicle_and_parking_registration(&namespace, &namespace.registration_id(1)?, None)?;
        verify_that!(registration.information(), len(eq(2)))
    }

    #[gtest]
    fn the_context_names_an_inline_definition_and_the_core_context() -> Result<()> {
        let context = registration_vocabulary(&Namespace::fresh())?;
        let Context::Multiple(entries) = context else {
            return fail!("the vocabulary is a list of two entries");
        };
        verify_that!(entries, len(eq(2)))
    }
}
