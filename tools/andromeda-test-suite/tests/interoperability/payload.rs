//! The context source registrations the ETSI interoperability data files hold, one variant per file.
//!
//! Every file registers the `OffStreetParking` or `Vehicle` type of the case, optionally for one entity and for some
//! properties, in one mode. The `endpoint` is the broker the registration points at and the `id` is the case's.

use crate::parking::Kind;
use andromeda_test_suite::namespace::{Namespace, NamespaceError};
use ngsi_ld::{
    model::{BuildError, EntityTypeName, Input},
    registration::{CSourceRegistration, EntityInfo, OperationGroup, Problem, RegistrationInfo, RegistrationMode},
};
use url::Url;

/// Why a registration of a data file could not be built.
#[derive(Debug, thiserror::Error)]
pub(crate) enum PayloadError {
    /// A namespaced identifier or type is invalid.
    #[error(transparent)]
    Namespace(#[from] NamespaceError),
    /// The registration or its information is invalid.
    #[error("the registration cannot be built")]
    Build(#[source] BuildError<Problem>),
}

/// A registration file of `data/csourceRegistrations/interoperability`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Payload {
    /// `context-source-registration-auxiliary-1`: every parking, auxiliary.
    Auxiliary1,
    /// `context-source-registration-auxiliary-2`: the spot counts of every parking, auxiliary.
    Auxiliary2,
    /// `context-source-registration-auxiliary-3`: every vehicle, auxiliary.
    Auxiliary3,
    /// `context-source-registration-exclusive-1`: the `totalSpotsNumber` of parking 2, exclusive.
    Exclusive1,
    /// `context-source-registration-exclusive-2`: the `location` of parking 2, exclusive.
    Exclusive2,
    /// `context-source-registration-exclusive-3`: the `location` of parking 1, exclusive.
    Exclusive3,
    /// `context-source-registration-inclusive-1`: every parking, inclusive.
    Inclusive1,
    /// `context-source-registration-inclusive-2`: the spot counts of every parking, inclusive.
    Inclusive2,
    /// `context-source-registration-inclusive-3`: every vehicle, inclusive.
    Inclusive3,
    /// `context-source-registration-redirect-1`: every parking, redirect.
    Redirect1,
    /// `context-source-registration-redirect-2`: the `location` of every parking, redirect.
    Redirect2,
    /// `context-source-registration-redirect-3`: the `location` of parking 2, redirect.
    Redirect3,
    /// `context-source-registration-redirect-4`: the `location` of vehicle 2, redirect.
    Redirect4,
}

impl Payload {
    /// The registration mode the file names.
    pub(crate) const fn mode(self) -> RegistrationMode {
        match self {
            Payload::Auxiliary1 | Payload::Auxiliary2 | Payload::Auxiliary3 => RegistrationMode::Auxiliary,
            Payload::Exclusive1 | Payload::Exclusive2 | Payload::Exclusive3 => RegistrationMode::Exclusive,
            Payload::Inclusive1 | Payload::Inclusive2 | Payload::Inclusive3 => RegistrationMode::Inclusive,
            Payload::Redirect1 | Payload::Redirect2 | Payload::Redirect3 | Payload::Redirect4 => RegistrationMode::Redirect,
        }
    }

    /// The kind of entity the file registers.
    pub(crate) const fn kind(self) -> Kind {
        match self {
            Payload::Auxiliary3 | Payload::Inclusive3 | Payload::Redirect4 => Kind::Vehicle,
            Payload::Auxiliary1
            | Payload::Auxiliary2
            | Payload::Exclusive1
            | Payload::Exclusive2
            | Payload::Exclusive3
            | Payload::Inclusive1
            | Payload::Inclusive2
            | Payload::Redirect1
            | Payload::Redirect2
            | Payload::Redirect3 => Kind::Parking,
        }
    }

    /// The number of the one entity the file registers, none when it registers the whole type.
    pub(crate) const fn entity(self) -> Option<u32> {
        match self {
            Payload::Exclusive1 | Payload::Exclusive2 | Payload::Redirect3 | Payload::Redirect4 => Some(2),
            Payload::Exclusive3 => Some(1),
            Payload::Auxiliary1
            | Payload::Auxiliary2
            | Payload::Auxiliary3
            | Payload::Inclusive1
            | Payload::Inclusive2
            | Payload::Inclusive3
            | Payload::Redirect1
            | Payload::Redirect2 => None,
        }
    }

    /// The properties the file registers, empty when it registers them all.
    pub(crate) const fn properties(self) -> &'static [&'static str] {
        match self {
            Payload::Auxiliary2 | Payload::Inclusive2 => &["availableSpotsNumber", "totalSpotsNumber"],
            Payload::Exclusive1 => &["totalSpotsNumber"],
            Payload::Exclusive2 | Payload::Exclusive3 | Payload::Redirect2 | Payload::Redirect3 | Payload::Redirect4 => &["location"],
            Payload::Auxiliary1 | Payload::Auxiliary3 | Payload::Inclusive1 | Payload::Inclusive3 | Payload::Redirect1 => &[],
        }
    }

    /// The operations the file names: the redirection operations, none for an auxiliary registration.
    pub(crate) const fn operations(self) -> Option<OperationGroup> {
        match self.mode() {
            RegistrationMode::Auxiliary => None,
            RegistrationMode::Inclusive | RegistrationMode::Exclusive | RegistrationMode::Redirect => Some(OperationGroup::RedirectionOps),
        }
    }

    /// The registration of the file for the case of `namespace`, numbered `number` and pointing at `endpoint`.
    ///
    /// The registration carries no `mode`: the model refuses an auxiliary registration that names no operations,
    /// which the files do, so the case writes the mode into the document it sends.
    ///
    /// # Errors
    ///
    /// Returns [`PayloadError`] when the namespaced identifiers are invalid or the model refuses the registration.
    pub(crate) fn registration(self, namespace: &Namespace, number: u32, endpoint: &Url) -> Result<CSourceRegistration<Input>, PayloadError> {
        let entity_type: EntityTypeName = namespace.entity_type(self.kind().base())?;
        let selector = match self.entity() {
            Some(entity) => EntityInfo::new(entity_type).with_id(self.kind().id(namespace, entity)?),
            None => EntityInfo::new(entity_type),
        };
        let information = match self.properties() {
            [] => RegistrationInfo::builder().entities([selector]).build(),
            names => RegistrationInfo::builder().entities([selector]).property_names(names.iter().copied()).build(),
        }
        .map_err(PayloadError::Build)?;
        let registration = CSourceRegistration::builder(endpoint.as_str())
            .id(namespace.registration_id(number)?)
            .information([information]);
        match self.operations() {
            Some(operations) => registration.operations([operations]).build(),
            None => registration.build(),
        }
        .map_err(PayloadError::Build)
    }
}

#[cfg(test)]
mod tests {
    use crate::payload::Payload;
    use andromeda_test_suite::namespace::Namespace;
    use googletest::prelude::*;
    use ngsi_ld::registration::RegistrationMode;
    use serde_json::{Value, json};
    use url::Url;

    fn rendered(payload: Payload, namespace: &Namespace) -> Result<Value> {
        let endpoint = Url::parse("http://broker-2:1056")?;
        let registration = payload.registration(namespace, 3, &endpoint)?;
        Ok(serde_json::from_str(&registration.render().to_string())?)
    }

    #[gtest]
    fn the_inclusive_file_with_properties_registers_the_counts_of_every_parking() -> Result<()> {
        let namespace = Namespace::fresh();
        let body = rendered(Payload::Inclusive2, &namespace)?;
        let information = json!([{
            "entities": [{"type": namespace.suffixed("OffStreetParking")}],
            "propertyNames": ["availableSpotsNumber", "totalSpotsNumber"],
        }]);
        expect_that!(body.get("information"), some(eq(&information)));
        expect_that!(body.get("operations"), some(eq(&json!(["redirectionOps"]))));
        expect_that!(body.get("endpoint"), some(eq(&json!("http://broker-2:1056/"))));
        expect_that!(body.get("id"), some(eq(&json!(namespace.urn_text("ContextSourceRegistration", 3)))));
        Ok(())
    }

    #[gtest]
    fn the_exclusive_file_registers_the_location_of_parking_two() -> Result<()> {
        let namespace = Namespace::fresh();
        let body = rendered(Payload::Exclusive2, &namespace)?;
        let information = json!([{
            "entities": [{"id": namespace.urn_text("OffStreetParking", 2), "type": namespace.suffixed("OffStreetParking")}],
            "propertyNames": ["location"],
        }]);
        expect_that!(body.get("information"), some(eq(&information)));
        Ok(())
    }

    #[gtest]
    fn an_auxiliary_file_names_no_operations() -> Result<()> {
        let namespace = Namespace::fresh();
        let body = rendered(Payload::Auxiliary1, &namespace)?;
        expect_that!(body.get("operations"), none());
        expect_that!(Payload::Auxiliary1.mode(), eq(RegistrationMode::Auxiliary));
        Ok(())
    }

    #[gtest]
    fn the_vehicle_redirect_file_registers_the_location_of_vehicle_two() -> Result<()> {
        let namespace = Namespace::fresh();
        let body = rendered(Payload::Redirect4, &namespace)?;
        let information = json!([{
            "entities": [{"id": namespace.urn_text("Vehicle", 2), "type": namespace.suffixed("Vehicle")}],
            "propertyNames": ["location"],
        }]);
        expect_that!(body.get("information"), some(eq(&information)));
        Ok(())
    }
}
