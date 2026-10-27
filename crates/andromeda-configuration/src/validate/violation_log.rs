//! The violations one validation pass has found so far.

use crate::{
    broker_config::BrokerConfig,
    human_duration::HumanDuration,
    schema::temporal::storage::is_positive_whole_days,
    validate::{
        InvalidConfiguration,
        bound::{Bound, Quantity},
        rule::{Limit, Rule},
        violation::ConfigurationViolation,
    },
};

/// The bound `limit` is in `config`, `None` when it reads a key that holds
/// no value.
fn bound(limit: Limit, config: &BrokerConfig) -> Option<Bound> {
    match limit {
        Limit::Fixed(limit) => Some(Bound::Fixed(limit)),
        Limit::Key(key) => key.reading(config).map(|value| Bound::Setting { key, value }),
        Limit::KeyPlusMargin(key, margin) => match key.reading(config)? {
            Quantity::Duration(value) => Some(Bound::SettingPlusMargin { key, value, margin }),
            Quantity::Count(_) => None,
        },
    }
}

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

    /// Record the violation `rule` finds in `config`, if any. A rule over a
    /// key, or against a limit, that holds no value passes.
    pub(crate) fn apply(&mut self, rule: Rule, config: &BrokerConfig) {
        let Some(value) = rule.key().reading(config) else {
            return;
        };
        let violation = match rule {
            Rule::Positive(key) => value.is_zero().then_some(ConfigurationViolation::Zero { key }),
            Rule::Ordered { key, relation, limit } => bound(limit, config)
                .filter(|bound| !relation.holds(value, bound.limit()))
                .map(|bound| ConfigurationViolation::OutOfRange { key, value, relation, bound }),
            Rule::WholeDays(key) => match value {
                Quantity::Duration(value) if !is_positive_whole_days(HumanDuration::new(value)) => Some(ConfigurationViolation::NotWholeDays { key, value }),
                Quantity::Duration(_) | Quantity::Count(_) => None,
            },
        };
        self.0.extend(violation);
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
