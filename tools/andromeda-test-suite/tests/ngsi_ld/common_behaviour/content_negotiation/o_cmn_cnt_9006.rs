//! A resource without a geojson form refuses it with its own list.

use andromeda_test_suite::{
    body_json::{json_count, json_equals},
    broker,
    namespace::Namespace,
    not_acceptable::not_acceptable,
    wire::accept,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::operation::{NormalizedTemporal, QueryRegistrations, QueryTemporalEvolution, RetrieveEntityTypes, RetrieveSubscription},
    query::{builders::SelectorTypeBuilder, temporal_query::TemporalQuery, type_selection},
    subscription::EntitySubscription,
};

/// Pins: Retrieve Available Entity Types, Retrieve Subscription, Query Context Source Registrations and Query
/// Temporal Evolution of Entities accepting application/geo+json answer 406 with a body holding only
/// `availableRepresentations`: application/json, application/ld+json
/// Clause: 6.3.4, 6.3.15
/// Recorded: clause 6.3.4 requires the 406 body to contain the list of the available representations without fixing
/// its shape; Andromeda answers an object holding only `availableRepresentations`, the media types the endpoint
/// renders, in the order of clause 6.3.4
#[gtest]
#[tokio::test]
async fn o_cmn_cnt_9006_a_resource_without_a_geojson_form_refuses_it_with_its_own_list() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let subscription_id = namespace.subscription_id("not-acceptable")?;
    let building_type = type_selection::entity_type(&namespace.entity_type("Building")?).build()?;
    let vehicle_type = SelectorTypeBuilder::from(&namespace.entity_type("Vehicle")?).build()?;
    let after = TemporalQuery::after("2020-08-01T12:00:00Z".parse()?);
    let requests = [
        broker.unchecked(&RetrieveEntityTypes::builder().build())?,
        broker.unchecked(&RetrieveSubscription::builder().id(&subscription_id).kind(EntitySubscription).build())?,
        broker.unchecked(&QueryRegistrations::builder().entity_type(&building_type).build())?,
        broker.unchecked(
            &QueryTemporalEvolution::builder()
                .entity_type(&vehicle_type)
                .temporal_q(&after)
                .format(NormalizedTemporal)
                .build(),
        )?,
    ];
    for request in requests {
        let answer = broker.send_unchecked(accept(request, "application/geo+json")).await;
        let (_, body) = not_acceptable(&answer).or_fail()?;
        expect_that!(body, json_count("$", 1));
        expect_that!(body, json_count("$.availableRepresentations", 2));
        expect_that!(body, json_equals("$.availableRepresentations[0]", "application/json"));
        expect_that!(body, json_equals("$.availableRepresentations[1]", "application/ld+json"));
    }
    Ok(())
}
