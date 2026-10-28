//! ETSI `037_02`: a query of context source registrations naming neither entity types nor attribute names is refused
//! with 400 (clause 5.10.2).

use crate::suite_keywords::refused_registration_query;
use andromeda_test_suite::{broker, namespace::Namespace, vocabulary::Vocabulary};
use googletest::prelude::*;

/// Pins: Check that one cannot query context source registrations, if neither Entity types nor Attribute names are
/// provided, an error of type 400 shall be raised.
/// Clause: 5.10.2
#[gtest]
#[tokio::test]
async fn e037_02_01_query_context_source_registrations_without_entity_types_and_attribute_names() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &[]).await?;
    refused_registration_query(&broker, &vocabulary, "csourceRegistrations").await
}
