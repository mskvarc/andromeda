//! A get query without a collation keeps the icu root order.

use andromeda_test_suite::{broker, identity_matcher::has_id, namespace::Namespace, nth::nth, outcome::Upserted};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{NgsiLdMediaType, Property, evolution::TemporalEntity},
    query::{Locality, ordering::OrderBy, temporal_query},
};

/// Pins: Query Temporal Evolution of Entities with `orderBy=id` and no collation answers `collationaB` before
/// `collationAb`, the ICU root order putting the lowercase letter first where byte order would not
/// Clause: 4.23.1, 5.7.4.4, Table 6.18.3.2-1
#[gtest]
#[tokio::test]
async fn n_tmp_qry_9017_a_get_query_without_a_collation_keeps_the_icu_root_order() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let collation_reading_collation_ab_id = namespace.urn("CollationReading", "collationAb")?;
    let collation_reading_type = namespace.entity_type("CollationReading")?;
    let collation_reading_collationa_b_id = namespace.urn("CollationReading", "collationaB")?;
    let collation_reading_collation_ab_evolution = TemporalEntity::builder()
        .id(collation_reading_collation_ab_id.clone())
        .entity_type(collation_reading_type.clone())
        .property("reading", Property::builder(1).observed_at("2024-01-01T00:00:00Z".parse()?))
        .build()?;
    broker
        .temporal()
        .create_or_update(&collation_reading_collation_ab_evolution)
        .send()
        .await?
        .created()?;
    let collation_reading_collationa_b_evolution = TemporalEntity::builder()
        .id(collation_reading_collationa_b_id.clone())
        .entity_type(collation_reading_type.clone())
        .property("reading", Property::builder(1).observed_at("2024-01-01T00:00:00Z".parse()?))
        .build()?;
    broker
        .temporal()
        .create_or_update(&collation_reading_collationa_b_evolution)
        .send()
        .await?
        .created()?;
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .query(temporal_query::after("2020-01-01T00:00:00Z".parse()?))
        .entity_type(&collation_reading_type)
        .ids(vec![collation_reading_collation_ab_id.clone(), collation_reading_collationa_b_id.clone()])
        .locality(Locality::Local)
        .ordering("id".parse::<OrderBy>()?)
        .send()
        .await?;
    expect_that!(page.items(), len(eq(2)));
    expect_that!(page.items(), nth(0, has_id(&collation_reading_collationa_b_id)));
    expect_that!(page.items(), nth(1, has_id(&collation_reading_collation_ab_id)));
    Ok(())
}
