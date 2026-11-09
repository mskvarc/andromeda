//! The component report names the build and the storage.

use andromeda_test_suite::{
    broker,
    component_report::{ComponentName, ComponentReportList, ReportType},
    raw_call::call,
};
use googletest::prelude::*;
use http::{Method, StatusCode, header::CONTENT_TYPE};

/// Pins: GET /ngsi-ld/v1/version answers a plain JSON `ComponentReportList` with no @context, whose andromeda
/// component carries its version and toolchain and no member naming the repository or the time of the build, and
/// which names andromeda and postgresql once each
/// Clause: -
/// Recorded: Andromeda extension, not v1.9.1: the version endpoint and its `ComponentReportList` body
#[gtest]
#[tokio::test]
async fn a_mgt_ver_0002_the_component_report_names_the_build_and_the_storage() -> Result<()> {
    let front = broker::front()?;
    // The component report is an Andromeda extension no NGSI-LD operation covers, so the request is sent as written.
    let answer = call(&front, Method::GET, "version", &[], None).await?;
    expect_that!(answer.status(), eq(StatusCode::OK));
    expect_that!(
        answer.headers().get(CONTENT_TYPE).and_then(|value| value.to_str().ok()),
        some(eq("application/json"))
    );
    let report: ComponentReportList = serde_json::from_slice(answer.body())?;
    expect_that!(report.id.as_str(), contains_regex("^urn:ngsi-ld:ComponentReportList:."));
    expect_that!(report.report_type, eq(ReportType::ComponentReportList));
    expect_that!(report.components, not(is_empty()));
    expect_that!(report.of(ComponentName::Andromeda).count(), eq(1));
    expect_that!(report.of(ComponentName::Postgresql).count(), eq(1));
    let andromeda = report.of(ComponentName::Andromeda).next().or_fail()?;
    expect_that!(andromeda.version, contains_regex("^v[0-9]"));
    expect_that!(andromeda.rust_version, some(contains_regex("^rustc ")));
    expect_that!(andromeda.rust_channel, some(contains_regex(".")));
    expect_that!(andromeda.cargo_version, some(contains_regex("^cargo ")));
    expect_that!(andromeda.build_operating_system, some(contains_regex(".")));
    expect_that!(andromeda.build_target, some(contains_regex(".")));
    let postgresql = report.of(ComponentName::Postgresql).next().or_fail()?;
    expect_that!(postgresql.version, contains_regex("^v[0-9]"));
    Ok(())
}
