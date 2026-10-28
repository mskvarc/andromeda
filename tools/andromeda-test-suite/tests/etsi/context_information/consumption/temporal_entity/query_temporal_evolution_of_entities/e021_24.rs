//! ETSI `021_24`: a temporal query that names identifiers and a time, but neither a type, nor attributes, nor a `q`,
//! nor a geoquery, and is not local, is refused with `BadRequestData` (clause 5.7.4). The typed query refuses such a
//! request before sending it, so the request is written on the wire.

use crate::suite_keywords::refused_get;
use andromeda_test_suite::{broker, namespace::Namespace, vocabulary::Vocabulary};
use googletest::prelude::*;

/// Pins: Check that one can not query the temporal evolution of entities if it has none of the minimal parameters
/// Clause: 5.7.4
#[gtest]
#[tokio::test]
async fn e021_24_query_temporal_evolution_of_entities_without_minimal_parameters() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &[]).await?;

    let target = format!(
        "temporal/entities?id=urn%3Angsi-ld%3AEntity%3A{namespace}-01%2Curn%3Angsi-ld%3AEntity%3A{namespace}-02&timerel=after&timeAt=2020-07-01T12%3A05%3A00Z"
    );
    refused_get(&broker.with_context(vocabulary.context())?, &target).await?;
    Ok(())
}
