//! A query by id and type is forwarded to an inclusive context source registered only for `queryEntity` by `GET
//! /entities`, and to one registered only for `queryBatch` by `POST /entityOperations/query` (clauses 4.3.3, 4.3.6.2,
//! 5.7.2 and 6.23.3).

use crate::distributed_operations::consumption::entity::query_entities::restricted_source::{Binding, expect_forwarded_query};
use googletest::prelude::*;

/// Pins: Check that if one queries the Context Broker for type via GET, and the registration only allows the
/// queryEntity operation, the request is forwarded as expected
/// Clause: 4.3.3, 4.3.6.2, 5.7.2, 6.23.3
/// Expected: the request is application/json with the @context in a Link header, not application/ld+json (clause 6.3.5)
/// Tenant: private (a type-only registration matches every request of the tenant that names no entity type, clause
/// 5.12)
#[gtest]
#[tokio::test]
async fn d011_04_01_inc_query_the_context_broker_via_post_with_cs_forwarded_using_query_entity() -> Result<()> {
    expect_forwarded_query(Binding::QueryEntity).await
}

/// Pins: Check that if one queries the Context Broker for type via POST, and the registration only allows the
/// queryBatch operation, the request is forwarded as expected
/// Clause: 4.3.3, 4.3.6.2, 5.7.2, 6.23.3
/// Expected: the request is application/json with the @context in a Link header, not application/ld+json (clause 6.3.5)
/// Tenant: private (a type-only registration matches every request of the tenant that names no entity type, clause
/// 5.12)
#[gtest]
#[tokio::test]
async fn d011_04_02_inc_query_the_context_broker_via_post_with_cs_forwarded_using_query_batch() -> Result<()> {
    expect_forwarded_query(Binding::QueryBatch).await
}
