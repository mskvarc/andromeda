//! Registration management information (ETSI GS CIM 009 clause 5.2.34).

use andromeda_ngsi_ld_lexicon::positive_milliseconds::PositiveMilliseconds;
use iso8601_duration::Duration as Iso8601Duration;
use serde::{Deserialize, Serialize};
use typed_builder::TypedBuilder;

/// Whether distributed operations through a registration cascade past the
/// registered context source (clause 5.2.34 `localOnly`, clause 4.3.6.4).
///
/// The wire form is the JSON boolean `localOnly`; the two named variants keep a
/// bare `bool` out of the domain model.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(from = "bool", into = "bool")]
pub enum LocalScope {
    /// `localOnly: true`: the source answers only from data it holds itself.
    LocalOnly,
    /// `localOnly: false`: the source may itself forward the operation further.
    Cascading,
}

// `localOnly` is a JSON boolean on the wire and `#[serde(from, into)]` needs
// the two conversions; no derive maps a boolean onto named variants.
impl From<bool> for LocalScope {
    fn from(local_only: bool) -> LocalScope {
        if local_only { LocalScope::LocalOnly } else { LocalScope::Cascading }
    }
}

impl From<LocalScope> for bool {
    fn from(scope: LocalScope) -> bool {
        match scope {
            LocalScope::LocalOnly => true,
            LocalScope::Cascading => false,
        }
    }
}

/// The `management` member of a registration: the knobs that alter how the
/// broker makes distributed requests to the registered source (Table 5.2.34-1).
///
/// Clause 5.2.34 defines exactly four members, so an unknown member is a
/// restriction violation and the type rejects it; the clause 5.2.9 allowance
/// for additional members applies to the registration's top level only.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize, TypedBuilder)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[builder(field_defaults(default = Default::default()))]
pub struct RegistrationManagementInfo {
    /// Minimal period between two consumption operations on the same data,
    /// within which a cached value is returned where available.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache_duration: Option<Iso8601Duration>,
    /// Minimum period, in milliseconds, before the endpoint is contacted again
    /// after a failure; a request inside it is answered with a timeout error.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cooldown: Option<PositiveMilliseconds>,
    /// Whether the registered source is asked for local data only.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub local_only: Option<LocalScope>,
    /// Maximum period, in milliseconds, before a forwarded request is assumed
    /// to have failed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timeout: Option<PositiveMilliseconds>,
}

#[cfg(test)]
mod tests {
    use crate::registration_management::{LocalScope, RegistrationManagementInfo};
    use serde_json::json;

    #[test]
    fn every_clause_5_2_34_member_round_trips_through_its_wire_shape() {
        let wire = json!({"cacheDuration": "PT10S", "cooldown": 500, "localOnly": true, "timeout": 3000});
        let parsed: RegistrationManagementInfo = serde_json::from_value(wire.clone()).expect("management parses"); // clone: compared below
        pretty_assertions::assert_eq!(parsed.local_only, Some(LocalScope::LocalOnly));
        pretty_assertions::assert_eq!(parsed.cooldown.map(|value| value.get()), Some(500));
        pretty_assertions::assert_eq!(parsed.timeout.map(|value| value.get()), Some(3000));
        pretty_assertions::assert_eq!(serde_json::to_value(&parsed).expect("serialises"), wire);
    }

    #[test]
    fn local_only_false_is_the_cascading_scope() {
        let parsed: RegistrationManagementInfo = serde_json::from_value(json!({"localOnly": false})).expect("management parses");
        pretty_assertions::assert_eq!(parsed.local_only, Some(LocalScope::Cascading));
        pretty_assertions::assert_eq!(serde_json::to_value(&parsed).expect("serialises"), json!({"localOnly": false}));
    }

    #[test]
    fn a_zero_or_negative_cooldown_or_timeout_is_rejected() {
        for wire in [json!({"cooldown": 0}), json!({"cooldown": -5}), json!({"timeout": 0}), json!({"timeout": -1})] {
            assert!(
                serde_json::from_value::<RegistrationManagementInfo>(wire.clone()).is_err(),
                "{wire} must be rejected"
            ); // clone: reported on failure
        }
    }

    #[test]
    fn a_cache_duration_that_is_not_iso_8601_is_rejected() {
        assert!(serde_json::from_value::<RegistrationManagementInfo>(json!({"cacheDuration": "10s"})).is_err());
        assert!(serde_json::from_value::<RegistrationManagementInfo>(json!({"cacheDuration": "PT10S"})).is_ok());
    }

    #[test]
    fn a_member_outside_table_5_2_34_1_is_rejected() {
        assert!(serde_json::from_value::<RegistrationManagementInfo>(json!({"managementInterval": {"startAt": "2020-01-01T00:00:00Z"}})).is_err());
    }
}
