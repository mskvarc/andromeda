//! Q admits the instance in effect when a between window opens.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    entity_lookup::with_id,
    evolution_matcher::has_evolution,
    namespace::Namespace,
    nth::nth,
    power_meters::{append_status, instants, meter_id, write_power},
};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::NgsiLdMediaType,
    query::{q, temporal_query},
};

/// Pins: a temporal q with a between window selects an entity by an in-window instance, by the instance observed
/// before the window and still in effect when it opens, and by one of several instances at one instant or of
/// several datasets, but not by a value a deletion or an instance at the inclusive window start replaced before the
/// window opens; q evaluates the attribute without rendering it, and each selected entity answers with its one
/// in-window projected instance
/// Clause: 5.7.4.4, 4.11, 4.9, 4.5.7
/// Recorded: clause 5.7.4.4 checks q against the Attribute instances "during the interval defined by the temporal
/// query" and clause 4.11 makes them the values "valid within the specified timeframe", without saying whether the
/// instance observed before the window opens and not yet replaced is one of them; Andromeda reads it as one (as
/// Annex C clause C.5.5 evaluates a q on an attribute it does not render), evaluated by q but never rendered
#[gtest]
#[tokio::test]
async fn o_tmp_qry_0002_q_admits_the_instance_in_effect_when_a_between_window_opens() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let at = instants()?;
    write_power(&broker, &namespace, &at).await?;
    append_status(&broker, &namespace, &at).await?;

    let meter_type = namespace.entity_type("Meter")?;
    let json = broker.with_accept(Accept::new(NgsiLdMediaType::Json));
    let page = json
        .temporal()
        .query(temporal_query::between(at.t60, at.t120).last_n(1))
        .entity_type(&meter_type)
        .q(q::attr("power").gt(8))
        .attrs(vec!["status".parse()?])
        .send()
        .await?;
    expect_that!(page.items(), len(eq(4)));
    for suffix in ["M1", "M2", "M5", "M6"] {
        let entity = with_id(page.items(), &meter_id(&namespace, suffix)?).or_fail()?;
        expect_that!(entity.attributes.len(), eq(1));
        expect_that!(entity, has_evolution("status", all!(len(eq(1)), nth(0, any_attribute().observed_at(at.t75)))));
    }
    Ok(())
}
