//! Focused immutable state for context source identity retrieval.

use andromeda_ngsi_ld_lexicon::{context_source_pseudonym::ContextSourcePseudonym, uri::Uri};
use andromeda_ngsi_ld_model::context_source_identity::{ContextSourceIdentity, ContextSourceIdentityType, uptime_duration};
use chrono::{DateTime, Utc};
use serde_json::Value;
use std::{sync::Arc, time::Duration};

/// What the identity binding knows about this context source.
///
/// Table 5.2.40-1 makes `contextSourceUptime` and `contextSourceTimeAt`
/// mandatory, so the state holds the instant the broker started and builds a
/// fresh document per request rather than a single immutable one.
#[derive(Clone)]
pub struct SourceIdentityState {
    /// The configured identity, shared by every request.
    configured: Arc<ConfiguredIdentity>,
}

/// The operator-configured part of the identity.
struct ConfiguredIdentity {
    /// The identifier of this context source.
    id: Uri,
    /// The pseudonym placed in `Via` chains.
    alias: ContextSourcePseudonym,
    /// The instant the broker became available.
    started_at: DateTime<Utc>,
    /// Raw, never-expanded instance information.
    extras: Option<Value>,
}

impl SourceIdentityState {
    /// Construct the context source identity exposed by the broker, available
    /// since `started_at`.
    #[must_use]
    pub fn new(id: Uri, alias: ContextSourcePseudonym, started_at: DateTime<Utc>, extras: Option<Value>) -> SourceIdentityState {
        SourceIdentityState {
            configured: Arc::new(ConfiguredIdentity { id, alias, started_at, extras }),
        }
    }

    /// The identity document as observed at `now`.
    #[must_use]
    pub fn identity_at(&self, now: DateTime<Utc>) -> ContextSourceIdentity {
        let elapsed = (now - self.configured.started_at).to_std().unwrap_or(Duration::ZERO);
        ContextSourceIdentity {
            id: self.configured.id.clone(), // clone: every response document owns its identifier
            type_: ContextSourceIdentityType,
            context_source_alias: self.configured.alias.clone(), // clone: every response document owns its alias
            context_source_uptime: uptime_duration(elapsed),
            context_source_time_at: now,
            context_source_extras: self.configured.extras.clone(), // clone: every response document owns its raw extras
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::state::SourceIdentityState;
    use andromeda_ngsi_ld_lexicon::{context_source_pseudonym::ContextSourcePseudonym, uri::Uri};
    use chrono::{Duration, Utc};
    use std::error::Error;

    fn state() -> Result<SourceIdentityState, Box<dyn Error>> {
        Ok(SourceIdentityState::new(
            "urn:ngsi-ld:ContextSourceIdentity:test".parse::<Uri>()?,
            "test".parse::<ContextSourcePseudonym>()?,
            Utc::now() - Duration::hours(2),
            Some(serde_json::json!({"node": 1})),
        ))
    }

    #[test]
    fn the_uptime_grows_with_the_observation_instant() -> Result<(), Box<dyn Error>> {
        let state = state()?;
        let now = Utc::now();
        let earlier = state.identity_at(now);
        let later = state.identity_at(now + Duration::minutes(5));
        let earlier_seconds = earlier.context_source_uptime.num_seconds().ok_or("finite uptime")?;
        let later_seconds = later.context_source_uptime.num_seconds().ok_or("finite uptime")?;
        assert!(later_seconds > earlier_seconds, "{later_seconds} must exceed {earlier_seconds}");
        pretty_assertions::assert_eq!(later.context_source_time_at, now + Duration::minutes(5));
        Ok(())
    }

    #[test]
    fn an_observation_before_the_start_reports_a_zero_uptime_rather_than_failing() -> Result<(), Box<dyn Error>> {
        let state = state()?;
        let identity = state.identity_at(Utc::now() - Duration::days(1));
        pretty_assertions::assert_eq!(identity.context_source_uptime.num_seconds(), Some(0.0));
        Ok(())
    }

    #[test]
    fn the_extras_travel_into_every_document() -> Result<(), Box<dyn Error>> {
        let identity = state()?.identity_at(Utc::now());
        pretty_assertions::assert_eq!(identity.context_source_extras, Some(serde_json::json!({"node": 1})));
        Ok(())
    }
}
