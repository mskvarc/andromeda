//! Version information domain types for the `/version` endpoint.
//!
//! Returns a custom JSON shape (not an NGSI-LD entity) with build-time
//! metadata and runtime database extension versions.

use crate::domain::uri::Uri;
use serde::{Deserialize, Serialize};

/// Top-level version response for `GET /ngsi-ld/v1/version`.
///
/// This endpoint is a broker extension and always returns HTTP 200 with
/// build and runtime version information.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct VersionResponse {
    /// Unique identifier of this version response.
    pub id: Uri,
    /// Response type discriminator (`VersionInfo`).
    #[serde(rename = "type")]
    pub type_: VersionType,
    /// Nested version details for Andromeda and database extensions.
    pub version_info: VersionInfo,
}

/// Type discriminator for [`VersionResponse`].
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, strum::Display, strum::EnumString)]
pub enum VersionType {
    /// `VersionInfo`.
    #[serde(rename = "VersionInfo")]
    #[strum(serialize = "VersionInfo")]
    VersionInfo,
}

/// Build-time and runtime version details.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct VersionInfo {
    /// Andromeda build-time metadata.
    pub andromeda: AndromedaVersion,
    /// PostgreSQL server version.
    pub postgresql: ExtensionVersion,
    /// PostGIS extension version.
    pub postgis: ExtensionVersion,
    /// TimescaleDB extension version and license.
    pub timescaledb: TimescaleDbVersion,
}

/// Andromeda build-time version information populated from `shadow-rs`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AndromedaVersion {
    /// Semantic version prefixed with `"v"`.
    pub version: String,
    /// RFC 3339 build timestamp.
    pub build_time: String,
    /// Git branch at build time.
    pub branch: String,
    /// Git commit hash at build time.
    pub commit_hash: String,
    /// Whether the git working tree was clean at build time.
    pub git_clean: bool,
    /// Rust compiler version string.
    pub rust_version: String,
    /// Rust release channel (stable, nightly, etc.).
    pub rust_channel: String,
    /// Cargo version string.
    pub cargo_version: String,
    /// Build host operating system and architecture.
    pub build_os: String,
    /// Rust compilation target triple.
    pub build_target: String,
}

/// Version information for a database extension with only a version field.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ExtensionVersion {
    /// Extension version prefixed with `"v"`.
    pub version: String,
}

/// TimescaleDB version information including its license type.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TimescaleDbVersion {
    /// Extension version prefixed with `"v"`.
    pub version: String,
    /// License identifier (`"TSL"` or `"Apache-2.0"`).
    pub license: String,
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used)]
    use super::*;
    use urn_rs::UrnBuilder;

    fn sample_response() -> VersionResponse {
        let id_urn = UrnBuilder::new("ngsi-ld", "VersionInfo:550e8400-e29b-41d4-a716-446655440000").build().unwrap();

        VersionResponse {
            id: Uri::Urn(id_urn),
            type_: VersionType::VersionInfo,
            version_info: VersionInfo {
                andromeda: AndromedaVersion {
                    version: "v0.1.0".to_owned(),
                    build_time: "2024-12-01T12:00:00+00:00".to_owned(),
                    branch: "main".to_owned(),
                    commit_hash: "abc123".to_owned(),
                    git_clean: true,
                    rust_version: "1.82.0".to_owned(),
                    rust_channel: "nightly".to_owned(),
                    cargo_version: "cargo 1.82.0".to_owned(),
                    build_os: "linux".to_owned(),
                    build_target: "x86_64".to_owned(),
                },
                postgresql: ExtensionVersion { version: "v16.2".to_owned() },
                postgis: ExtensionVersion { version: "v3.4.1".to_owned() },
                timescaledb: TimescaleDbVersion {
                    version: "v2.13.0".to_owned(),
                    license: "Apache-2.0".to_owned(),
                },
            },
        }
    }

    #[test]
    fn version_response_serializes_to_expected_json() {
        let response = sample_response();
        let json = serde_json::to_value(&response).unwrap();

        let expected = serde_json::json!({
            "id": "urn:ngsi-ld:VersionInfo:550e8400-e29b-41d4-a716-446655440000",
            "type": "VersionInfo",
            "versionInfo": {
                "andromeda": {
                    "version": "v0.1.0",
                    "buildTime": "2024-12-01T12:00:00+00:00",
                    "branch": "main",
                    "commitHash": "abc123",
                    "gitClean": true,
                    "rustVersion": "1.82.0",
                    "rustChannel": "nightly",
                    "cargoVersion": "cargo 1.82.0",
                    "buildOs": "linux",
                    "buildTarget": "x86_64"
                },
                "postgresql": {
                    "version": "v16.2"
                },
                "postgis": {
                    "version": "v3.4.1"
                },
                "timescaledb": {
                    "version": "v2.13.0",
                    "license": "Apache-2.0"
                }
            }
        });

        pretty_assertions::assert_eq!(json, expected);
    }

    #[test]
    fn version_response_round_trip_serde() {
        let response = sample_response();
        let json = serde_json::to_string(&response).unwrap();
        let deserialized: VersionResponse = serde_json::from_str(&json).unwrap();
        pretty_assertions::assert_eq!(deserialized, response);
    }

    #[test]
    fn version_type_serializes_as_version_info() {
        let json = serde_json::to_value(VersionType::VersionInfo).unwrap();
        pretty_assertions::assert_eq!(json, serde_json::Value::String("VersionInfo".to_owned()));
    }

    #[test]
    fn andromeda_version_fields_serialize_camel_case() {
        let version = AndromedaVersion {
            version: "v0.1.0".to_owned(),
            build_time: "2024-12-01T12:00:00+00:00".to_owned(),
            branch: "main".to_owned(),
            commit_hash: "abc123".to_owned(),
            git_clean: true,
            rust_version: "1.82.0".to_owned(),
            rust_channel: "nightly".to_owned(),
            cargo_version: "cargo 1.82.0".to_owned(),
            build_os: "linux".to_owned(),
            build_target: "x86_64".to_owned(),
        };
        let json = serde_json::to_value(&version).unwrap();
        let obj = json.as_object().unwrap();

        assert!(obj.contains_key("buildTime"));
        assert!(obj.contains_key("commitHash"));
        assert!(obj.contains_key("gitClean"));
        assert!(obj.contains_key("rustVersion"));
        assert!(obj.contains_key("rustChannel"));
        assert!(obj.contains_key("cargoVersion"));
        assert!(obj.contains_key("buildOs"));
        assert!(obj.contains_key("buildTarget"));
    }

    #[test]
    fn extension_version_serializes_correctly() {
        let ext = ExtensionVersion { version: "v16.2".to_owned() };
        let json = serde_json::to_value(&ext).unwrap();
        let expected = serde_json::json!({"version": "v16.2"});
        pretty_assertions::assert_eq!(json, expected);
    }

    #[test]
    fn timescaledb_version_includes_license() {
        let ts = TimescaleDbVersion {
            version: "v2.13.0".to_owned(),
            license: "TSL".to_owned(),
        };
        let json = serde_json::to_value(&ts).unwrap();
        let expected = serde_json::json!({"version": "v2.13.0", "license": "TSL"});
        pretty_assertions::assert_eq!(json, expected);
    }

    #[test]
    fn git_clean_serializes_as_boolean() {
        let version_true = AndromedaVersion {
            version: "v0.1.0".to_owned(),
            build_time: "2024-12-01T12:00:00+00:00".to_owned(),
            branch: "main".to_owned(),
            commit_hash: "abc123".to_owned(),
            git_clean: true,
            rust_version: "1.82.0".to_owned(),
            rust_channel: "nightly".to_owned(),
            cargo_version: "cargo 1.82.0".to_owned(),
            build_os: "linux".to_owned(),
            build_target: "x86_64".to_owned(),
        };
        let json = serde_json::to_value(&version_true).unwrap();
        assert_eq!(json["gitClean"], serde_json::Value::Bool(true));

        let version_false = AndromedaVersion {
            git_clean: false,
            ..version_true
        };
        let json = serde_json::to_value(&version_false).unwrap();
        assert_eq!(json["gitClean"], serde_json::Value::Bool(false));
    }
}
