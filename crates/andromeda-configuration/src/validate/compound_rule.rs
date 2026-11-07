//! The rules that are no single comparison, held as data.
//!
//! A compound rule applies while every one of its conditions holds, and then
//! asks every one of its obligations. The validator evaluates it from this
//! data, and the configuration descriptor writes the same data out, so a tool
//! checks a configuration the way the validator does.

use crate::{
    schema::{http3::Http3Mode, listener_transport::ListenerTransport, management::ManagementMode},
    validate::{
        choice_value::ChoiceValue,
        config_key::ConfigKey,
        rule::{Limit, Rule},
    },
};

/// A rule that is no single comparison: when it applies and what it asks
/// then, with one sentence stating it for the operator.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CompoundRule {
    /// What the rule asks, as one sentence.
    pub statement: &'static str,
    /// The conditions under which the rule applies, all of them at once;
    /// empty for a rule that always applies.
    pub when: &'static [Condition],
    /// What the rule asks while it applies, every one of them.
    pub then: &'static [Obligation],
}

impl CompoundRule {
    /// Every key the rule names, the keys it asks something of before the
    /// keys it applies on, each once; the descriptor lists the rule under
    /// each. A [`Condition::TlsListener`] names none: the listener keys are
    /// named once, by [`TLS_LISTENERS`], so a rule asking something of every
    /// TLS listener is not listed under each of them.
    #[must_use]
    pub fn keys(&self) -> Vec<ConfigKey> {
        let mut keys = Vec::new();
        self.then.iter().for_each(|obligation| obligation.name_keys(&mut keys));
        self.when.iter().for_each(|condition| condition.name_keys(&mut keys));
        keys
    }
}

/// What has to hold for a compound rule to apply.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Condition {
    /// The key holds one of these keywords.
    Is {
        /// The key read.
        key: ConfigKey,
        /// The keywords that satisfy the condition.
        choices: &'static [ChoiceValue],
    },
    /// The key holds a keyword other than this one.
    IsNot {
        /// The key read.
        key: ConfigKey,
        /// The keyword that fails the condition.
        choice: ChoiceValue,
    },
    /// A listener terminates TLS: every condition of one of
    /// [`TLS_LISTENERS`] holds.
    TlsListener,
}

impl Condition {
    /// Add the key the condition reads to `keys`, unless it is there
    /// already or the condition is a TLS listener.
    fn name_keys(self, keys: &mut Vec<ConfigKey>) {
        match self {
            Condition::Is { key, .. } | Condition::IsNot { key, .. } => name(keys, key),
            Condition::TlsListener => {}
        }
    }
}

/// What a compound rule asks while it applies.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Obligation {
    /// The optional key is set.
    Present(ConfigKey),
    /// The optional key is left out.
    Absent(ConfigKey),
    /// The list holds at least one entry.
    NonEmpty(ConfigKey),
    /// The list holds no entry.
    Empty(ConfigKey),
    /// The key holds a keyword other than this one; a list written as its
    /// entries holds no keyword.
    IsNot {
        /// The key read.
        key: ConfigKey,
        /// The keyword refused.
        choice: ChoiceValue,
    },
    /// The key holds a value other than the one `other` holds.
    Differs {
        /// The key read.
        key: ConfigKey,
        /// The key it must differ from.
        other: ConfigKey,
    },
    /// No two entries of the list of tables hold the same value under this
    /// key.
    UniqueEntries(ConfigKey),
    /// The byte size fits the platform's address width, so the limit it sets
    /// can be reached.
    FitsAddressWidth(ConfigKey),
    /// The comparison holds.
    Holds(Rule),
}

impl Obligation {
    /// Add the keys the obligation reads to `keys`, skipping one already
    /// there.
    fn name_keys(self, keys: &mut Vec<ConfigKey>) {
        match self {
            Obligation::Present(key)
            | Obligation::Absent(key)
            | Obligation::NonEmpty(key)
            | Obligation::Empty(key)
            | Obligation::IsNot { key, .. }
            | Obligation::UniqueEntries(key)
            | Obligation::FitsAddressWidth(key) => name(keys, key),
            Obligation::Differs { key, other } => {
                name(keys, key);
                name(keys, other);
            }
            Obligation::Holds(rule) => {
                name(keys, rule.key());
                if let Rule::Ordered {
                    limit: Limit::Key(bound) | Limit::KeyPlusMargin(bound, _),
                    ..
                } = rule
                {
                    name(keys, bound);
                }
            }
        }
    }
}

/// Add `key` to `keys` unless it is there already.
fn name(keys: &mut Vec<ConfigKey>, key: ConfigKey) {
    if !keys.contains(&key) {
        keys.push(key);
    }
}

/// Each way a listener terminates TLS, as the conditions that make it do so,
/// in the order a refusal names the first that holds.
///
/// QUIC is always TLS, so an enabled HTTP/3 listener always terminates it; the
/// management listener reads its `transport` only in `separate-port` mode.
pub const TLS_LISTENERS: [&[Condition]; 3] = [
    &[Condition::Is {
        key: ConfigKey::ServerTransport,
        choices: &[ChoiceValue::Transport(ListenerTransport::Tls)],
    }],
    &[Condition::Is {
        key: ConfigKey::ServerHttp3Mode,
        choices: &[ChoiceValue::Http3(Http3Mode::Enabled)],
    }],
    &[
        Condition::Is {
            key: ConfigKey::ManagementTransport,
            choices: &[ChoiceValue::Transport(ListenerTransport::Tls)],
        },
        Condition::Is {
            key: ConfigKey::ManagementMode,
            choices: &[ChoiceValue::Management(ManagementMode::SeparatePort)],
        },
    ],
];

#[cfg(test)]
mod tests {
    use crate::validate::{
        bound::Relation,
        choice_value::ChoiceValue,
        compound_rule::{CompoundRule, Condition, Obligation},
        config_key::ConfigKey,
        rule::{Limit, Rule},
    };
    use pretty_assertions::assert_eq;

    #[test]
    fn a_rule_names_the_keys_it_asks_of_before_the_keys_it_applies_on_each_once() {
        let rule = CompoundRule {
            statement: "",
            when: &[Condition::IsNot {
                key: ConfigKey::FederationCooldownMode,
                choice: ChoiceValue::Any,
            }],
            then: &[
                Obligation::Holds(Rule::Ordered {
                    key: ConfigKey::FederationCooldownMaxRetryInterval,
                    relation: Relation::AtLeast,
                    limit: Limit::Key(ConfigKey::FederationCooldownRetryInterval),
                }),
                Obligation::Holds(Rule::Positive(ConfigKey::FederationCooldownRetryInterval)),
            ],
        };
        assert_eq!(
            rule.keys(),
            [
                ConfigKey::FederationCooldownMaxRetryInterval,
                ConfigKey::FederationCooldownRetryInterval,
                ConfigKey::FederationCooldownMode,
            ]
        );
    }

    #[test]
    fn a_tls_listener_condition_names_no_listener_key() {
        let rule = CompoundRule {
            statement: "",
            when: &[Condition::TlsListener],
            then: &[Obligation::Present(ConfigKey::TlsCertificate)],
        };
        assert_eq!(rule.keys(), [ConfigKey::TlsCertificate]);
    }
}
