//! Evaluating a compound rule against a configuration.

use crate::{
    broker_config::BrokerConfig,
    validate::{
        address_width::AddressWidth,
        bound::Quantity,
        compound_rule::{CompoundRule, Condition, Obligation, TLS_LISTENERS},
        config_key::ConfigKey,
        key_holding::{Holding, Presence},
        violation::{ConfigurationViolation, HeldChoice, Requirement},
        violation_log::ViolationLog,
    },
};
use std::collections::HashSet;

impl ViolationLog {
    /// Record every violation `rule` finds in `config`, with byte sizes
    /// checked against `width`. A rule whose conditions do not all hold
    /// finds none.
    pub(crate) fn check(&mut self, rule: &CompoundRule, config: &BrokerConfig, width: AddressWidth) {
        let Some(held) = held(rule.when, config) else {
            return;
        };
        let required_by = Requirement::new(held);
        rule.then.iter().for_each(|obligation| self.enforce(*obligation, config, width, &required_by));
    }

    /// Record the violations of one obligation, applying under
    /// `required_by`.
    fn enforce(&mut self, obligation: Obligation, config: &BrokerConfig, width: AddressWidth, required_by: &Requirement) {
        // clone: each violation owns the settings it names
        let owned = || required_by.clone();
        match obligation {
            Obligation::Present(key) | Obligation::NonEmpty(key) => {
                if presence(key, config) == Some(Presence::Absent) {
                    self.record(ConfigurationViolation::Unset { key, required_by: owned() });
                }
            }
            Obligation::Absent(key) | Obligation::Empty(key) => {
                if presence(key, config) == Some(Presence::Present) {
                    self.record(ConfigurationViolation::Unread { key, required_by: owned() });
                }
            }
            Obligation::IsNot { key, choice } => {
                if key.holding(config).and_then(Holding::choice) == Some(choice) {
                    self.record(ConfigurationViolation::ChoiceRefused {
                        key,
                        choice,
                        required_by: owned(),
                    });
                }
            }
            Obligation::Differs { key, other } => {
                if let Some(value) = key.reading(config).filter(|value| other.reading(config) == Some(*value)) {
                    self.record(ConfigurationViolation::Collision {
                        key,
                        other,
                        value,
                        required_by: owned(),
                    });
                }
            }
            Obligation::UniqueEntries(key) => {
                let contexts = key.holding(config).and_then(Holding::preloaded_contexts).unwrap_or_default();
                let mut named = HashSet::with_capacity(contexts.len());
                let mut reported = HashSet::new();
                contexts
                    .iter()
                    .map(|context| &context.url)
                    .filter(|url| !named.insert(*url) && reported.insert(*url))
                    .for_each(|url| self.record(ConfigurationViolation::Duplicate { key, url: url.clone() })); // clone: the violation outlives the configuration it reports on
            }
            Obligation::FitsAddressWidth(key) => match key.reading(config) {
                Some(Quantity::Bytes(bytes)) if !width.admits(bytes) => self.record(ConfigurationViolation::ExceedsAddressWidth { key, bytes }),
                Some(Quantity::Bytes(_) | Quantity::Duration(_) | Quantity::Count(_)) | None => {}
            },
            Obligation::Holds(rule) => self.apply(rule, config),
        }
    }
}

/// Whether the key is set, as an obligation over presence reads it.
fn presence(key: ConfigKey, config: &BrokerConfig) -> Option<Presence> {
    key.holding(config).and_then(Holding::presence)
}

/// The keywords that make every one of `conditions` hold, in the order they
/// are named; `None` when one of them does not hold.
fn held(conditions: &[Condition], config: &BrokerConfig) -> Option<Vec<HeldChoice>> {
    conditions.iter().try_fold(Vec::new(), |mut held, condition| {
        held.extend(condition_held(*condition, config)?);
        Some(held)
    })
}

/// The keywords that make `condition` hold; `None` when it does not.
fn condition_held(condition: Condition, config: &BrokerConfig) -> Option<Vec<HeldChoice>> {
    match condition {
        Condition::Is { key, choices } => key
            .holding(config)
            .and_then(Holding::choice)
            .filter(|choice| choices.contains(choice))
            .map(|choice| vec![HeldChoice { key, choice }]),
        Condition::IsNot { key, choice } => key
            .holding(config)
            .and_then(Holding::choice)
            .filter(|held| *held != choice)
            .map(|held| vec![HeldChoice { key, choice: held }]),
        Condition::TlsListener => TLS_LISTENERS.iter().find_map(|listener| held(listener, config)),
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        broker_config::BrokerConfig,
        schema::{http::cors::ExposeRule, management::ManagementMode},
        validate::{
            bound::Quantity,
            choice_value::ChoiceValue,
            compound_rule::{Condition, Obligation, TLS_LISTENERS},
            compound_rules,
            config_key::ConfigKey,
            key_holding::Holding,
        },
    };
    use std::mem::discriminant;

    /// Whether `key` holds a keyword of the same type as `choice`.
    fn holds_a_keyword_like(key: ConfigKey, choice: ChoiceValue, config: &BrokerConfig) -> bool {
        key.holding(config)
            .and_then(Holding::choice)
            .is_some_and(|held| discriminant(&held) == discriminant(&choice))
    }

    /// A rule held as data names its keys by value, so nothing but this test
    /// stops a keyword test over a quantity key, or a keyword of another
    /// key's type, from never holding. Every key is read here at the
    /// defaults, with the management plane on its own port so its port is
    /// read, and every CORS list at its wildcard so it holds a keyword.
    #[test]
    fn every_compound_rule_reads_each_key_as_the_kind_it_names_it() {
        let mut config = BrokerConfig::local();
        config.management.mode = ManagementMode::SeparatePort;
        config.http.cors.expose_headers = ExposeRule::Any;
        let rules: Vec<_> = compound_rules().collect();
        let conditions = rules
            .iter()
            .flat_map(|rule| rule.when.iter())
            .chain(TLS_LISTENERS.iter().flat_map(|listener| listener.iter()));
        for condition in conditions {
            let read = match *condition {
                Condition::Is { key, choices } => choices.iter().all(|choice| holds_a_keyword_like(key, *choice, &config)),
                Condition::IsNot { key, choice } => holds_a_keyword_like(key, choice, &config),
                Condition::TlsListener => true,
            };
            assert!(read, "{condition:?}");
        }
        for obligation in rules.iter().flat_map(|rule| rule.then.iter()) {
            let read = match *obligation {
                Obligation::Present(key) | Obligation::Absent(key) | Obligation::NonEmpty(key) | Obligation::Empty(key) => {
                    key.holding(&config).and_then(Holding::presence).is_some()
                }
                Obligation::IsNot { key, choice } => holds_a_keyword_like(key, choice, &config),
                Obligation::Differs { key, other } => key.reading(&config).is_some() && other.reading(&config).is_some(),
                Obligation::UniqueEntries(key) => key.holding(&config).and_then(Holding::preloaded_contexts).is_some(),
                Obligation::FitsAddressWidth(key) => matches!(key.reading(&config), Some(Quantity::Bytes(_))),
                Obligation::Holds(rule) => rule.key().reading(&config).is_some(),
            };
            assert!(read, "{obligation:?}");
        }
    }
}
