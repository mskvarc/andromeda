//! The violations one validation pass has found so far.

use crate::{
    human_duration::HumanDuration,
    validate::{
        InvalidConfiguration,
        bound::{Bound, Quantity, Relation},
        config_key::ConfigKey,
        violation::ConfigurationViolation,
    },
};

/// Every violation found so far, in the order the rules ran.
///
/// A rule records what it finds and the pass goes on, so one run reports
/// every rule the configuration breaks.
#[derive(Debug, Default)]
pub(crate) struct ViolationLog(Vec<ConfigurationViolation>);

impl ViolationLog {
    /// Record a violation.
    pub(crate) fn record(&mut self, violation: ConfigurationViolation) {
        self.0.push(violation);
    }

    /// Record [`ConfigurationViolation::Zero`] for a zero duration.
    pub(crate) fn positive_duration(&mut self, key: ConfigKey, value: HumanDuration) {
        if value.is_zero() {
            self.record(ConfigurationViolation::Zero { key });
        }
    }

    /// Record [`ConfigurationViolation::Zero`] for a zero count.
    pub(crate) fn positive_count(&mut self, key: ConfigKey, value: u64) {
        if value == 0 {
            self.record(ConfigurationViolation::Zero { key });
        }
    }

    /// Record [`ConfigurationViolation::OutOfRange`] unless `value` stands in
    /// `relation` to `bound`.
    pub(crate) fn require(&mut self, key: ConfigKey, value: impl Into<Quantity>, relation: Relation, bound: Bound) {
        let value = value.into();
        if !relation.holds(value, bound.limit()) {
            self.record(ConfigurationViolation::OutOfRange { key, value, relation, bound });
        }
    }

    /// The configuration's verdict: valid when nothing was recorded.
    pub(crate) fn into_result(self) -> Result<(), InvalidConfiguration> {
        if self.0.is_empty() {
            Ok(())
        } else {
            Err(InvalidConfiguration { violations: self.0 })
        }
    }
}
