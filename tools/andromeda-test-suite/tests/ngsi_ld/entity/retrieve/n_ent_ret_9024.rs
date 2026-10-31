//! Every family reads back a bare seal pair when normalized.

use crate::retrieve::sealed_family::{SEALED_FAMILIES, create_sealed_building, expect_bare_seal_spelling, sealed_proof};
use andromeda_test_suite::{broker, entity_matcher::has_attribute, namespace::Namespace};
use googletest::prelude::*;
use ngsi_ld::{http::headers::Accept, model::NgsiLdMediaType};

/// Pins: An Entity whose attribute of each of the eight families carries an ngsildproof is retrieved with
/// ngsildproof a Property holding `entityIdSealed` and `entityTypeSealed` as bare strings and the Data Integrity
/// proof as written, and no prefixed seal spelling
/// Clause: 4.5.2.2, 4.5.3.2, 4.5.18, 4.5.20, 4.5.21, 4.5.22, 4.5.24, C.11
#[gtest]
#[tokio::test]
async fn n_ent_ret_9024_every_family_reads_back_a_bare_seal_pair_when_normalized() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_type = namespace.entity_type("Building")?;
    for family in SEALED_FAMILIES {
        let id = create_sealed_building(&broker, &namespace, &building_type, "normalized", family).await?;
        let received = broker
            .with_responses()
            .with_accept(Accept::new(NgsiLdMediaType::Json))
            .entities()
            .retrieve(&id)
            .send()
            .await?;
        expect_bare_seal_spelling(&received.body);
        expect_that!(
            received.outcome.body(),
            has_attribute("sealed", sealed_proof(id, building_type.clone()).with_proof())
        );
    }
    Ok(())
}
