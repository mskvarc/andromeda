//! Discovery stays exact under a seeded random write sequence.

use andromeda_test_suite::{
    answered_status::AnsweredStatus,
    broker::{self, BrokerClient},
    discovery_state::ExpectedDiscovery,
    namespace::Namespace,
    wire::request_to,
};
use bytes::Bytes;
use googletest::prelude::*;
use http::{HeaderValue, Method, StatusCode, header::CONTENT_TYPE};

mod phases;

/// Sends the write `line` (`<METHOD> <path>` with ` <JSON>` after it when the write has a body) to the tenant
/// `client` speaks to, and returns the status the broker answered with.
async fn write(client: &BrokerClient, line: &str) -> Result<Option<StatusCode>> {
    let (method, rest) = line.split_once(' ').or_fail()?;
    let (path, body) = match rest.split_once(' ') {
        Some((path, body)) => (path, Some(body)),
        None => (rest, None),
    };
    // Fragments and merge bodies no typed operation states are sent as written, and the sequence checks only that the
    // broker does not fail on any write.
    let request = request_to(client, Method::from_bytes(method.as_bytes())?, path)?;
    let request = match body {
        Some(body) => request
            .append_header(CONTENT_TYPE, HeaderValue::from_static("application/json"))
            .body(Bytes::from(body.to_owned())),
        None => request,
    };
    Ok(client.send_unchecked(request).await.answered_status())
}

/// Pins: After every twelfth write of a seeded random sequence of 240 creates, replacements, appends, updates,
/// attribute and `datasetId` deletions, entity deletions, batch upserts in both modes, type merges, attribute
/// merges and batch deletions over 24 identifiers, four types, five attribute names, eight attribute types and
/// three datasets, the type list, type details, entity type information, attribute list, attribute details and
/// attribute information answer exactly the types, counts, attribute names and attribute types the Entities then
/// hold
/// Clause: 5.7.5, 5.7.6, 5.7.7, 5.7.8, 5.7.9, 5.7.10
/// Tenant: private (the subjects are the unfiltered type and attribute lists and their counts, exact only in a
/// tenant no other file writes to)
#[gtest]
#[tokio::test]
async fn n_dsc_tyi_0010_discovery_stays_exact_under_a_seeded_random_write_sequence() -> Result<()> {
    let namespace = Namespace::fresh();
    let front = broker::front()?;
    let tenant = broker::private_tenant(&front, &namespace).await?;
    let token = namespace.to_string();
    for (phase_number, phase) in phases::PHASES.iter().enumerate() {
        for line in phase.writes {
            let status = write(&tenant, &line.replace("{{ns}}", &token)).await?;
            expect_that!(status, some(lt(StatusCode::INTERNAL_SERVER_ERROR)));
        }
        let expected = ExpectedDiscovery::parse(&phase.expected.join("\n"))?;
        let differences: Vec<String> = expected
            .differences(&tenant, &namespace)
            .await?
            .into_iter()
            .map(|difference| format!("after write {}: {difference}", (phase_number + 1) * 12))
            .collect();
        expect_that!(differences, is_empty());
    }
    Ok(())
}
