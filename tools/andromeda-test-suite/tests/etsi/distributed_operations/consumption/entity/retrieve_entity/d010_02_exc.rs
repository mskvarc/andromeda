//! ETSI `D010_02_exc`: a retrieval with `local=true` of an entity whose `speed` an exclusive context source holds
//! answers the broker's part without the `speed` (clauses 4.3.3, 4.3.6.3, 5.7.1 and 6.3.18).

use crate::distributed_operations::consumption::entity::retrieve_entity::vehicle_sources::{exclusive_speed_source, torn_down};
use andromeda_test_suite::{broker, namespace::Namespace, wire::json_body};
use googletest::prelude::*;
use ngsi_ld::{http::headers::Accept, query::Locality};

/// Pins: Check that if one retrieves a fragmented entity locally, the response does not contain the exclusive
/// attribute
/// Clause: 4.3.3, 4.3.6.3, 5.7.1
/// Changed: the context source is the test's own server in place of the suite's HTTP control server: the registration
/// names its address, each Set Stub Reply is a stub matched in the order it was set by the server's rule (case,
/// trailing slashes, the query tokens, the attrs and urn rules), and a request no stub matches is answered 502 after
/// 5 s where the server closes the connection.
/// Changed: the retrieval is held to a success status, where the case reads the body of any status.
/// Changed: the teardown delete of the registration is held to the 204 a deleted registration answers, not only to a
/// status below 400.
#[gtest]
#[tokio::test]
async fn d010_02_exc_query_context_broker_and_retrieve_entity_by_id() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let (vocabulary, _source, _registrations) = exclusive_speed_source(&broker, &namespace).await?;
    let retrieved = broker
        .with_context(vocabulary.context())?
        .with_accept(Accept::any())
        .with_responses()
        .entities()
        .retrieve(&namespace.urn("Vehicle", 1)?)
        .locality(Locality::Local)
        .send()
        .await?;
    let body = json_body(&retrieved)?;
    expect_that!(body.as_object(), some(anything()));
    expect_that!(body.pointer("/speed"), none());
    torn_down(&broker, &namespace, &[1]).await
}
