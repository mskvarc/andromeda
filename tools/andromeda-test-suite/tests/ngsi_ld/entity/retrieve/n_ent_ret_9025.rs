//! Every family reads back a bare seal pair when concise.

use crate::retrieve::sealed_family::{SEALED_FAMILIES, create_sealed_building, expect_bare_seal_spelling, sealed_proof};
use andromeda_test_suite::{broker, entity_matcher::has_attribute, namespace::Namespace};
use googletest::prelude::*;
use ngsi_ld::{
    http::{headers::Accept, operation::RepresentationSpelling},
    model::{Lossless, NgsiLdMediaType},
};

/// Pins: An Entity whose attribute of each of the eight families carries an ngsildproof is retrieved with
/// options=concise with `entityIdSealed` and `entityTypeSealed` as bare strings beside the Data Integrity proof as
/// written, and no prefixed seal spelling
/// Clause: 4.5.2.2, 4.5.2.3, 4.5.3.3, C.11
#[gtest]
#[tokio::test]
async fn n_ent_ret_9025_every_family_reads_back_a_bare_seal_pair_when_concise() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_type = namespace.entity_type("Building")?;
    for family in SEALED_FAMILIES {
        let id = create_sealed_building(&broker, &namespace, &building_type, "concise", family).await?;
        let received = broker
            .with_responses()
            .with_accept(Accept::new(NgsiLdMediaType::Json))
            .entities()
            .retrieve(&id)
            .form(Lossless::Concise)
            .spelling(RepresentationSpelling::default().in_options())
            .send()
            .await?;
        expect_bare_seal_spelling(&received.body);
        expect_that!(received.outcome.body(), has_attribute("sealed", sealed_proof(id, building_type.clone())));
    }
    Ok(())
}
