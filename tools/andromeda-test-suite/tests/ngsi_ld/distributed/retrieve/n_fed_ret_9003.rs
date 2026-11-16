//! A forwarded retrieval carries the client's `lang`.

use andromeda_test_suite::{
    attribute_matcher::property,
    broker,
    cleanup::Cleanup,
    context_source::ContextSource,
    entity_matcher::has_attribute,
    namespace::Namespace,
    source_answer,
};
use googletest::prelude::*;
use http::Method;
use ngsi_ld::{
    http::{headers::Accept, operation::RetrieveEntityParameters},
    model::{Entity, LanguageMap, LanguageProperty, NgsiLdMediaType},
    query::language_filter::LanguageFilter,
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationInfo, RegistrationMode},
};

/// Pins: over an inclusive source advertising `retrieveEntity` for the type, Retrieve Entity with `lang` ranking
/// `de` above `en` reaches the source with that `lang`, and the source's `LanguageProperty` is answered as the
/// Property of its German entry
/// Clause: 5.7.1.4, 4.15
/// Recorded: clause 5.7.1.4 forwards the request, and `lang` is no filter a forward leaves behind
#[gtest]
#[tokio::test]
async fn n_fed_ret_9003_a_forwarded_retrieval_carries_the_client_lang() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let source = ContextSource::start().await?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle_id = namespace.urn("Vehicle", "lang")?;
    let registration_id = namespace.registration_id(1)?;
    let registration = CSourceRegistration::builder(source.endpoint("r")?)
        .id(registration_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id_pattern(namespace.id_pattern("Vehicle")?)])
            .build()?])
        .mode(RegistrationMode::Inclusive)
        .operations([DistributedOperation::RetrieveEntity])
        .build()?;
    broker.registrations().register(&registration).send().await?;
    let _cleanup = Cleanup::of(&broker).registration(&registration_id);
    let held = Entity::output_builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type)
        .language_property(
            "name",
            LanguageProperty::builder(LanguageMap::try_from_pairs([("en", "Tower"), ("de", "Turm")])?),
        )
        .build()?;
    let path = format!("/r/ngsi-ld/v1/entities/{vehicle_id}");
    source.answer(Method::GET, &path, source_answer::entity(&held)).await?;
    let lang: LanguageFilter = "de,en;q=0.5".parse()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&vehicle_id)
        .lang(lang.clone())
        .send()
        .await?;
    expect_that!(retrieved.body(), has_attribute("name", property().value("Turm").lang("de")));
    let asked = source.received_on(Method::GET, &path).await?;
    let asked = asked.first().or_fail()?;
    let parameters = RetrieveEntityParameters::parse(asked.url.query().unwrap_or_default())?;
    expect_that!(parameters.lang, some(eq(&lang)));
    Ok(())
}
