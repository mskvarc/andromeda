//! ETSI `037_03`: a query of context source registrations is refused with 400 when an entity id is not a URI or the
//! query, geoquery or temporal query is not valid (clause 5.10.2).

use crate::suite_keywords::refused_registration_query;
use andromeda_test_suite::{broker, namespace::Namespace, vocabulary::Vocabulary};
use googletest::prelude::*;

/// Registers an empty vocabulary and requires the query written as `path` to be refused.
async fn refused(path: &str) -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &[]).await?;
    refused_registration_query(&broker, &vocabulary, path).await
}

/// Pins: Check that one cannot query context source registrations, if the list of Entity identifiers includes a URI
/// which it is not valid, or the query, geo-query or temporal query are not syntactically valid
/// Clause: 5.10.2
#[gtest]
#[tokio::test]
async fn e037_03_01_invalid_uri() -> Result<()> {
    refused("csourceRegistrations?id=invalidUri").await
}

/// Pins: Check that one cannot query context source registrations, if the list of Entity identifiers includes a URI
/// which it is not valid, or the query, geo-query or temporal query are not syntactically valid
/// Clause: 5.10.2
#[gtest]
#[tokio::test]
async fn e037_03_02_invalid_query() -> Result<()> {
    refused("csourceRegistrations?q=%28invalidQuery").await
}

/// Pins: Check that one cannot query context source registrations, if the list of Entity identifiers includes a URI
/// which it is not valid, or the query, geo-query or temporal query are not syntactically valid
/// Clause: 5.10.2
#[gtest]
#[tokio::test]
async fn e037_03_03_invalid_geo_query() -> Result<()> {
    refused("csourceRegistrations?georel=within").await
}

/// Pins: Check that one cannot query context source registrations, if the list of Entity identifiers includes a URI
/// which it is not valid, or the query, geo-query or temporal query are not syntactically valid
/// Clause: 5.10.2
#[gtest]
#[tokio::test]
async fn e037_03_04_invalid_temporal_query() -> Result<()> {
    refused("csourceRegistrations?timerel=before").await
}
