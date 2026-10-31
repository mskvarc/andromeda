//! The `ComponentReportList` that `GET /ngsi-ld/v1/version` answers: an Andromeda extension no NGSI-LD model
//! covers, read into a type so a test states members, not paths.

use crate::reported_instant::ReportedInstant;
use serde::Deserialize;
use url::Url;

/// The `type` a component report list carries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
pub enum ReportType {
    /// A `ComponentReportList`.
    ComponentReportList,
}

/// A component the version endpoint reports.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ComponentName {
    /// The broker itself.
    Andromeda,
    /// The database server.
    Postgresql,
    /// The spatial extension of the database.
    Postgis,
    /// The time series extension of the database.
    Timescaledb,
}

/// One component and what the broker knows of its build; the members only the broker's own entry carries are
/// absent from the others.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ComponentReport {
    /// `component`.
    pub component: ComponentName,
    /// `version`: free-form text such as `v18.6 (Debian 18.6-1.pgdg13+2)`.
    pub version: String,
    /// `buildTime`.
    pub build_time: Option<ReportedInstant>,
    /// `branch`: the Git branch of the build, empty when it has none.
    pub branch: Option<String>,
    /// `commit`: the Git commit of the build, empty when it has none.
    pub commit: Option<String>,
    /// `gitClean`: whether the working tree was clean.
    pub git_clean: Option<bool>,
    /// `rustVersion`.
    pub rust_version: Option<String>,
    /// `rustChannel`.
    pub rust_channel: Option<String>,
    /// `cargoVersion`.
    pub cargo_version: Option<String>,
    /// `buildOperatingSystem`.
    pub build_operating_system: Option<String>,
    /// `buildTarget`.
    pub build_target: Option<String>,
}

/// The document the version endpoint answers.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ComponentReportList {
    /// `id`: `urn:ngsi-ld:ComponentReportList:` and a token.
    pub id: Url,
    /// `type`.
    #[serde(rename = "type")]
    pub report_type: ReportType,
    /// `components`.
    pub components: Vec<ComponentReport>,
}

impl ComponentReportList {
    /// The reports of `component`.
    pub fn of(&self, component: ComponentName) -> impl Iterator<Item = &ComponentReport> {
        self.components.iter().filter(move |report| report.component == component)
    }
}

#[cfg(test)]
mod tests {
    use crate::component_report::{ComponentName, ComponentReportList, ReportType};
    use googletest::prelude::*;

    const LIST: &str = r#"{"id":"urn:ngsi-ld:ComponentReportList:1","type":"ComponentReportList","components":[{"component":"andromeda","version":"v0.1.0","buildTime":"2026-10-29T16:37:22.853856262+00:00","branch":"","commit":"","gitClean":true,"rustVersion":"rustc 1","rustChannel":"nightly","cargoVersion":"cargo 1","buildOperatingSystem":"linux","buildTarget":"x"},{"component":"postgresql","version":"v18.6","connections":{"verdict":"sufficient"}},{"component":"timescaledb","version":"v2.31.0","license":"TSL"}]}"#;

    #[gtest]
    fn a_report_list_reads_each_component_with_its_own_members() -> Result<()> {
        let list: ComponentReportList = serde_json::from_str(LIST)?;
        expect_that!(list.report_type, eq(ReportType::ComponentReportList));
        expect_that!(list.of(ComponentName::Andromeda).count(), eq(1));
        let build = list.of(ComponentName::Andromeda).next().or_fail()?;
        expect_that!(build.git_clean, some(eq(true)));
        expect_that!(build.build_time, some(anything()));
        let storage = list.of(ComponentName::Postgresql).next().or_fail()?;
        expect_that!(storage.build_time, none());
        Ok(())
    }

    #[gtest]
    fn a_member_beside_the_documented_three_is_refused() {
        let text = LIST.replacen(r#""type":"ComponentReportList""#, r#""type":"ComponentReportList","@context":"x""#, 1);
        expect_that!(serde_json::from_str::<ComponentReportList>(&text), err(anything()));
    }
}
