//! ETSI `D011_03_inc`: a query by id and type is forwarded to an inclusive context source registered only for
//! `queryEntity` by `GET /entities`, and to one registered only for `queryBatch` by `POST /entityOperations/query`
//! (clauses 4.3.3, 4.3.6.2 and 5.7.2).

use crate::distributed_operations::consumption::entity::query_entities::restricted_source::{Binding, expect_forwarded_query};
use googletest::prelude::*;

/// Pins: Check that if one queries the Context Broker for type, and the registration only allows the queryEntity
/// operation, the request is forwarded as expected
/// Clause: 4.3.3, 4.3.6.2, 5.7.2
/// Corrected: `fix(D011-json)`: use application/json with the link context header, not ld+json (6.3.5)
/// Changed: the context source is the test's own server in place of the suite's HTTP control server: the registration
/// names its address, each Set Stub Reply is a stub matched in the order it was set by the server's rule (case,
/// trailing slashes, the query tokens, the attrs and urn rules), and a request no stub matches is answered 502 after
/// 5 s where the server closes the connection.
/// Changed: the suite names this case `D011_03_inc_01`, with the case number after the mode; the file name moves it
/// before the mode (d011-03-01-inc), as in the other files of the area
/// Changed: the teardown delete is held to the 204 a deleted registration answers, not only to a status below 400.
/// Tenant: private (a type-only registration matches every request of the tenant that names no entity type, clause
/// 5.12)
#[gtest]
#[tokio::test]
async fn d011_03_01_inc_query_the_context_broker_via_get_using_query_entity() -> Result<()> {
    expect_forwarded_query(Binding::QueryEntity).await
}

/// Pins: Check that if one queries the Context Broker for type, and the registration only allows the queryBatch
/// operation, the request is forwarded as expected
/// Clause: 4.3.3, 4.3.6.2, 5.7.2
/// Corrected: `fix(D011-json)`: use application/json with the link context header, not ld+json (6.3.5)
/// Changed: the context source is the test's own server in place of the suite's HTTP control server: the registration
/// names its address, each Set Stub Reply is a stub matched in the order it was set by the server's rule (case,
/// trailing slashes, the query tokens, the attrs and urn rules), and a request no stub matches is answered 502 after
/// 5 s where the server closes the connection.
/// Changed: the suite names this case `D011_03_inc_02`, with the case number after the mode; the file name moves it
/// before the mode (d011-03-02-inc), as in the other files of the area
/// Changed: the teardown delete is held to the 204 a deleted registration answers, not only to a status below 400.
/// Tenant: private (a type-only registration matches every request of the tenant that names no entity type, clause
/// 5.12)
#[gtest]
#[tokio::test]
async fn d011_03_02_inc_query_the_context_broker_via_post_using_query_batch() -> Result<()> {
    expect_forwarded_query(Binding::QueryBatch).await
}
