//! A context source registration whose `information` is null is refused with 400 (clause 5.9.2).

use crate::unchecked_requests::refused;
use andromeda_test_suite::{
    broker::{self, BrokerClient},
    cleanup::Cleanup,
    config::SuiteConfig,
    namespace::Namespace,
    vocabulary::Vocabulary,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::{ContextDelivery, UncheckedDocument, operation::RegisterContextSource},
    model::{ErrorType, RawJson},
    registration::{CSourceRegistration, RegistrationId, RegistrationInfo},
};

/// Pins: Check that one cannot create a context source with invalid content
/// Clause: 5.9.2
/// Tenant: private (a broker that failed to refuse the request would hold a Context Source Registration without
/// information, which matches every other file's queries in the shared tenant (clause 5.12) and would forward them;
/// clause 4.14 keeps each tenant's registrations its own)
#[gtest]
#[tokio::test]
async fn e033_02_01_create_a_context_source_registration_with_invalid_content() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let tenant: BrokerClient = broker::private_tenant(&broker, &namespace).await?;
    let vocabulary = Vocabulary::register(&tenant, &namespace, &[]).await?;
    let registration = CSourceRegistration::builder(SuiteConfig::get()?.unreachable_origin().as_str())
        .id(namespace.urn_text("ContextSourceRegistration", "randomUUID").parse::<RegistrationId>()?)
        .information([RegistrationInfo::builder().entities(["ProvisioningSentinel"]).build()?])
        .build()?;
    // The typed builder cannot write a null `information`; the rendered registration carries the member it replaces.
    let null_information: RawJson = "null".parse()?;
    let document = UncheckedDocument::try_from(registration.render())?.set_member("information", &null_information);
    let client = tenant
        .with_context(vocabulary.context_list())?
        .with_context_delivery(ContextDelivery::LdJsonBody);
    let _registration = Cleanup::of(&client).registration_of(&registration);
    let request = client.unchecked(&RegisterContextSource { registration: &registration })?.document(&document);
    refused(&tenant, request, ErrorType::BadRequestData).await
}
