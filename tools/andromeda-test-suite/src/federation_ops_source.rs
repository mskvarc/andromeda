//! A source registered for the `federationOps` group (clause 5.2.9, Table 5.2.9-2) beside operations the broker answers
//! alone: the broker must not contact the source for them, and its registration must report no forward.

use crate::{broker::BrokerClient, context_source::ContextSource, namespace::Namespace, source_request::SourceRequest};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::Input,
    registration::{CSourceRegistration, EntityInfo, OperationGroup, RegistrationId, RegistrationInfo, RegistrationMode},
};
use url::Url;

/// The registration `number` 1 of the test: an inclusive source at `endpoint` advertising `federationOps` for the test's
/// `Vehicle` type and identifiers.
fn federation_ops_registration(namespace: &Namespace, endpoint: Url) -> Result<CSourceRegistration<Input>> {
    let registration = CSourceRegistration::builder(endpoint)
        .id(namespace.registration_id(1)?)
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(namespace.entity_type("Vehicle")?).with_id_pattern(namespace.id_pattern("Vehicle")?)])
            .build()?])
        .mode(RegistrationMode::Inclusive)
        .operations([OperationGroup::FederationOps])
        .build()?;
    Ok(registration)
}

/// Registers the endpoint `a` of `source` as an inclusive source advertising `federationOps` for the test's `Vehicle`
/// type and identifiers, and returns the registration identifier.
///
/// # Errors
///
/// Returns the failure to build or register the registration.
pub async fn register_federation_ops_source(broker: &BrokerClient, source: &ContextSource, namespace: &Namespace) -> Result<RegistrationId> {
    let registration = federation_ops_registration(namespace, source.endpoint("a")?)?;
    broker.registrations().register(&registration).send().await?;
    Ok(namespace.registration_id(1)?)
}

/// Requires that `source` received no request matching `forwarded`.
///
/// # Errors
///
/// Returns the failure to read the source's journal.
pub async fn expect_unreached(source: &ContextSource, forwarded: &SourceRequest) -> Result<()> {
    let received = source.received_matching(forwarded).await?;
    verify_that!(received, len(eq(0)))
}

/// Requires that the registration `registration_id` reports no `timesSent`: nothing was forwarded under it.
///
/// # Errors
///
/// Returns the failure to read the registration.
pub async fn expect_no_forward_reported(broker: &BrokerClient, registration_id: &RegistrationId) -> Result<()> {
    let registration = broker.with_accept(Accept::any()).registrations().retrieve(registration_id).send().await?;
    verify_that!(registration.output().times_sent, none())
}

#[cfg(test)]
mod tests {
    use crate::{federation_ops_source::federation_ops_registration, namespace::Namespace};
    use googletest::prelude::*;

    #[gtest]
    fn the_registration_advertises_the_federation_ops_group_for_the_tests_vehicles() -> Result<()> {
        let namespace = Namespace::fresh();
        let registration = federation_ops_registration(&namespace, "http://source.test/a".parse()?)?;
        verify_that!(
            registration.render().to_string(),
            all!(
                contains_substring(r#""operations":["federationOps"]"#),
                contains_substring(r#""mode":"inclusive""#),
                contains_substring(format!(r#""idPattern":"urn:ngsi-ld:Vehicle:{namespace}-.*""#))
            )
        )
    }
}
