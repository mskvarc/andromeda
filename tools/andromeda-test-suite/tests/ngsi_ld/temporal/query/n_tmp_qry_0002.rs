//! Q reads only the instances inside a between window.

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

/// Pins: a temporal q power>8 with a window between t60 and t120 selects M1 alone, whose 9 is observed inside the
/// window, and none of M2, M5 and M6, whose only values above 8 are observed before it, M3, whose power is deleted,
/// or M4, whose one instance inside the window is 1; q evaluates the attribute without rendering it, and M1 answers
/// with its one projected status instance inside the window
/// Clause: 5.7.4.4, 4.11, 4.9
#[gtest]
#[tokio::test]
async fn n_tmp_qry_0002_q_reads_only_the_instances_inside_a_between_window() -> Result<()> {
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
    expect_that!(page.items(), len(eq(1)));
    let entity = with_id(page.items(), &meter_id(&namespace, "M1")?).or_fail()?;
    expect_that!(entity.attributes.len(), eq(1));
    expect_that!(entity, has_evolution("status", all!(len(eq(1)), nth(0, any_attribute().observed_at(at.t75)))));
    Ok(())
}
