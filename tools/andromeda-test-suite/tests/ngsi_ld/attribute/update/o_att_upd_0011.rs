//! A scope on an entity without scope is ignored with 207.

use andromeda_test_suite::{broker, namespace::Namespace, nth::nth, outcome::Done, partial_outcome::PartialSuccess, report_matcher::not_updated};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, EntityFragment, NgsiLdMediaType, OneOrMany, Scope, ScopeChange},
};

/// Pins: Update Attributes with a scope on an Entity that has no scope ignores the scope, answering 207 with an
/// `UpdateResult` that names scope as not updated and none as updated, and the Entity still has no scope
/// Clause: 5.6.2.4, 6.6.3.2, 4.18, 5.2.18
/// Recorded: clause 5.6.2.4 says a scope in the Fragment is ignored when the target Entity has none, without saying
/// whether the answer is then 204 or 207; Andromeda answers 207 and reports scope as not updated
#[gtest]
#[tokio::test]
async fn o_att_upd_0011_a_scope_on_an_entity_without_scope_is_ignored_with_207() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_id = namespace.urn("Building", 1)?;
    let building_type = namespace.entity_type("Building")?;
    let building = Entity::builder().id(building_id.clone()).entity_type(building_type).build()?;
    broker.entities().create(&building).send().await?.done()?;
    let fragment = EntityFragment::builder()
        .scope(ScopeChange::Set(OneOrMany::one("/CompanyA/OrganizationB/UnitC".parse::<Scope>()?)))
        .build()?;
    let report = broker.entities().update_attributes(&building_id, &fragment).send().await?.partial()?;
    expect_that!(&report.updated, is_empty());
    expect_that!(&report.not_updated, len(eq(1)));
    // Clause 6.3.6 names an attribute of a partial success by its Fully Qualified Name, and the core `@context` maps
    // `scope` to that IRI (Annex B).
    expect_that!(&report.not_updated, nth(0, not_updated().attribute("https://uri.etsi.org/ngsi-ld/scope")));
    expect_that!(report.not_updated.first().map(|details| details.reason.as_str()), some(not(eq(""))));
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(entity.id, eq(&building_id));
    expect_that!(entity.scope, none());
    Ok(())
}
