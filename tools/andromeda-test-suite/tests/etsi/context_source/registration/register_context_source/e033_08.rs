//! ETSI `033_08`: a registration sent as `application/ld+json` whose body carries no `@context` is refused with 400
//! (clause 6.3.5).

use crate::suite_keywords::refused;
use andromeda_test_suite::{
    broker::{self, BrokerClient},
    cleanup::Cleanup,
    config::SuiteConfig,
    namespace::Namespace,
    wire::content_type,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::operation::RegisterContextSource,
    model::{APPLICATION_LD_JSON, ErrorType},
    registration::{CSourceRegistration, RegistrationId, RegistrationInfo},
};

/// Pins: Check that an HTTP error response of type `BadRequestData` is raised if the Content-Type header is
/// "application/ld+json" and the request payload body does not contain a @context term
/// Clause: 6.3.5
/// Tenant: private (the case sends a registration of Building with no @context, which expands under the core default
/// vocabulary, where any other file that names Building without a @context shares it; a broker that failed to refuse it
/// would hold a registration that captures and forwards that file's queries (clause 5.12), and clause 4.14 keeps each
/// tenant's registrations its own)
/// Vocabulary: none (the ETSI case references no @context)
#[gtest]
#[tokio::test]
async fn e033_08_01_no_context_with_json_ld_content_type() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let tenant: BrokerClient = broker::private_tenant(&broker, &namespace).await?;
    let registration = CSourceRegistration::builder(SuiteConfig::get()?.unreachable_origin().as_str())
        .id(namespace.urn_text("ContextSourceRegistration", 1).parse::<RegistrationId>()?)
        .information([RegistrationInfo::builder().entities(["Building"]).build()?])
        .build()?;
    // The tenant's client carries no `@context` at all: neither a `Link` nor a body member, as the case sends it.
    let _registration = Cleanup::of(&tenant).registration_of(&registration);
    let request = tenant.unchecked(&RegisterContextSource { registration: &registration })?;
    refused(&tenant, content_type(request, &APPLICATION_LD_JSON)?, ErrorType::BadRequestData).await
}
