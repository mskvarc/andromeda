//! A prefixed name or an absolute IRI in csf goes in double quotes.

use andromeda_test_suite::{
    broker,
    cleanup::Cleanup,
    config::SuiteConfig,
    hosted_context::HostedContext,
    identity_matcher::has_id,
    namespace::Namespace,
    nth::nth,
    problem::{answered, problem},
    written_parameters::{send_written, written_request},
};
use googletest::prelude::*;
use http::StatusCode;
use iri_rs::IriRefBuf;
use ngsi_ld::{
    http::{ContextDelivery, QueryParameter, operation::QueryRegistrations},
    model::{Context, ContextEntry, ErrorType, InlineContext},
    query::type_selection,
    registration::{CSourceRegistration, EntityInfo, RegistrationInfo},
};

/// Pins: over two registrations whose Context Source Property ex:tier, written under an @context holding the prefix
/// ex, is "gold" and "silver", Query Context Source Registrations with csf="ex:tier"=="gold" under that prefix and
/// with csf="https://example.org/<namespace>/tier"=="gold" answers only the gold one, and csf=ex:tier=="gold" and
/// csf=https://example.org/<namespace>/tier=="gold" are each 400 `BadRequestData`
/// Clause: 4.9, 4.6.2, 5.5.7, 5.10.2.4
/// Recorded: clause 4.9 spells `AttrName` as a letter followed by letters, digits and underscores, and has every name
/// expanded as clause 5.5.7 says, which reads a prefixed name and an absolute IRI too; Andromeda reads a csf by the
/// path grammar of q, an unquoted name as exactly the grammar's `AttrName` and any other name as the grammar's
/// `quotedStr`, whose value is the name, and refuses an unquoted name holding a colon
#[gtest]
#[tokio::test]
async fn a_csr_qry_9100_a_prefixed_name_or_an_absolute_iri_in_csf_goes_in_double_quotes() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let config = SuiteConfig::get()?;
    let prefix = format!(r#"{{"ex":"https://example.org/{namespace}/"}}"#);
    let prefixed = HostedContext::add(&broker, &Context::Single(ContextEntry::Inline(prefix.parse::<InlineContext>()?))).await?;
    let writer = broker
        .with_context(Context::Multiple(vec![
            ContextEntry::Inline(prefix.parse::<InlineContext>()?),
            ContextEntry::Reference(IriRefBuf::new("https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.9.jsonld".to_owned())?),
        ]))?
        .with_context_delivery(ContextDelivery::LdJsonBody);
    let sensor_type = namespace.entity_type("Sensor")?;
    let gold_id = namespace.registration_id("gold")?;
    let silver_id = namespace.registration_id("silver")?;
    for (id, tier) in [(&gold_id, "gold"), (&silver_id, "silver")] {
        let registration = CSourceRegistration::builder(config.unreachable_origin().as_str())
            .id(id.clone())
            .information([RegistrationInfo::builder()
                .entities([EntityInfo::new(sensor_type.clone()).with_id_pattern(namespace.id_pattern("Sensor")?)])
                .build()?])
            .property("ex:tier", tier)
            .build()?;
        let registered = writer.with_responses().registrations().register(&registration).send().await;
        expect_that!(&registered, answered(StatusCode::CREATED), "{tier}");
    }
    // Registrations are deleted when the test ends: a forward to one left behind fails at its endpoint, and the
    // cooldown that opens there holds back a later test's registration naming the same endpoint.
    let _gold_cleanup = Cleanup::of(&broker).registration(&gold_id);
    let _silver_cleanup = Cleanup::of(&broker).registration(&silver_id);
    let selection = type_selection::entity_type(&sensor_type).build()?;
    let query = QueryRegistrations::builder().entity_type(&selection).build();
    let prefixed_reader = broker.with_context(Context::Single(prefixed.entry()?))?;
    let quoted_iri = format!(r#""https://example.org/{namespace}/tier"=="gold""#);
    // The q model writes a name only as an unquoted AttrName, so a csf naming a prefixed name or an IRI is set as
    // written.
    for (csf, reader) in [(r#""ex:tier"=="gold""#, &prefixed_reader), (quoted_iri.as_str(), &broker)] {
        let page = send_written(reader, &query, &[(QueryParameter::Csf, csf)]).await?;
        expect_that!(page.items(), len(eq(1)), "{csf}");
        expect_that!(page.items(), nth(0, has_id(&gold_id)), "{csf}");
    }
    for (csf, reader) in [
        (r#"ex:tier=="gold""#.to_owned(), &prefixed_reader),
        (format!(r#"https://example.org/{namespace}/tier=="gold""#), &broker),
    ] {
        let request = written_request(reader, &query, &[(QueryParameter::Csf, csf.as_str())])?;
        expect_that!(
            &reader.send_unchecked(request).await,
            problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData),
            "{csf}"
        );
    }
    Ok(())
}
