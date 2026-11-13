//! The `ComponentReportList` that `GET /ngsi-ld/andromeda/v1/version` answers: an Andromeda extension no NGSI-LD model
//! covers, read into a type so a test states members, not paths.

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
#[serde(rename_all = "camelCase")]
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

/// Whether the database server's connections cover every pool the broker may open.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CapacityVerdict {
    /// The broker's pools fit.
    Sufficient,
    /// The broker's pools, all full at once, need more connections than it may open.
    Exceeded,
}

/// `connections` on the database server's entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ConnectionCapacity {
    /// `serverMaxConnections`.
    pub server_max_connections: u32,
    /// `reservedForOthers`.
    pub reserved_for_others: u32,
    /// `brokerCeiling`.
    pub broker_ceiling: u32,
    /// `verdict`.
    pub verdict: CapacityVerdict,
}

/// One component and what the broker reports of it. Every member any component carries is named here and any
/// other member fails the read; the members only one component carries are absent from the others.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ComponentReport {
    /// `component`.
    pub component: ComponentName,
    /// `version`: free-form text such as `v18.6 (Debian 18.6-1.pgdg13+2)`.
    pub version: String,
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
    /// `connections`, on the database server's entry.
    pub connections: Option<ConnectionCapacity>,
    /// `license`, on the time series extension's entry.
    pub license: Option<String>,
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
    use crate::component_report::{CapacityVerdict, ComponentName, ComponentReportList, ReportType};
    use googletest::prelude::*;

    const LIST: &str = r#"{"id":"urn:ngsi-ld:ComponentReportList:1","type":"ComponentReportList","components":[{"component":"andromeda","version":"v0.1.0","rustVersion":"rustc 1","rustChannel":"nightly","cargoVersion":"cargo 1","buildOperatingSystem":"linux-aarch64","buildTarget":"x"},{"component":"postgresql","version":"v18.6","connections":{"serverMaxConnections":200,"reservedForOthers":3,"brokerCeiling":150,"verdict":"sufficient"}},{"component":"timescaledb","version":"v2.31.0","license":"TSL"}]}"#;

    #[gtest]
    fn a_report_list_reads_each_component_with_its_own_members() -> Result<()> {
        let list: ComponentReportList = serde_json::from_str(LIST)?;
        expect_that!(list.report_type, eq(ReportType::ComponentReportList));
        expect_that!(list.of(ComponentName::Andromeda).count(), eq(1));
        let build = list.of(ComponentName::Andromeda).next().or_fail()?;
        expect_that!(build.rust_channel.as_deref(), some(eq("nightly")));
        expect_that!(build.connections, none());
        let storage = list.of(ComponentName::Postgresql).next().or_fail()?;
        expect_that!(storage.rust_version, none());
        expect_that!(
            storage.connections.map(|connections| connections.verdict),
            some(eq(CapacityVerdict::Sufficient))
        );
        Ok(())
    }

    #[gtest]
    fn a_member_beside_the_documented_three_is_refused() {
        let text = LIST.replacen(r#""type":"ComponentReportList""#, r#""type":"ComponentReportList","@context":"x""#, 1);
        expect_that!(serde_json::from_str::<ComponentReportList>(&text), err(anything()));
    }

    #[gtest]
    fn a_component_member_the_model_does_not_name_is_refused() {
        let text = LIST.replacen(r#""buildTarget":"x""#, r#""buildTarget":"x","commit":"0123456789""#, 1);
        expect_that!(serde_json::from_str::<ComponentReportList>(&text), err(anything()));
    }
}
