//! The rules that are no single comparison, as the descriptor writes them:
//! when each applies and what it then asks, as data a tool evaluates the way
//! the validator does.

use crate::{
    reference::{
        descriptor::{LimitDescriptor, limit_descriptor},
        key_listing::KeyLimit,
    },
    validate::{
        compound_rule::{CompoundRule, Condition, Obligation, TLS_LISTENERS},
        compound_rules,
    },
};
use serde::Serialize;

/// One compound rule.
#[derive(Debug, Serialize)]
pub(crate) struct RuleDescriptor {
    /// Every key the rule names, as dotted paths; each key lists the rule's
    /// statement among its conditions.
    keys: Vec<String>,
    /// What the rule asks, as one sentence.
    statement: &'static str,
    /// The conditions under which the rule applies, all of them at once;
    /// empty for a rule that always applies.
    when: Vec<ConditionDescriptor>,
    /// What the rule asks while it applies, every one of them.
    then: Vec<ObligationDescriptor>,
}

/// One condition of a rule.
#[derive(Debug, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
enum ConditionDescriptor {
    /// The key holds one of `choices`.
    Is {
        /// The key's dotted path.
        key: String,
        /// The keywords that satisfy the condition.
        choices: Vec<String>,
    },
    /// The key holds a keyword other than `choice`.
    IsNot {
        /// The key's dotted path.
        key: String,
        /// The keyword that fails the condition.
        choice: String,
    },
    /// A listener terminates TLS: every condition of one entry of `any_of`
    /// holds.
    TlsListener {
        /// Each way a listener terminates TLS, as the conditions that make it
        /// do so.
        any_of: Vec<Vec<ConditionDescriptor>>,
    },
}

/// One obligation of a rule.
#[derive(Debug, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
enum ObligationDescriptor {
    /// The optional key is set.
    Present {
        /// The key's dotted path.
        key: String,
    },
    /// The optional key is left out.
    Absent {
        /// The key's dotted path.
        key: String,
    },
    /// The list holds at least one entry.
    NonEmpty {
        /// The key's dotted path.
        key: String,
    },
    /// The list holds no entry.
    Empty {
        /// The key's dotted path.
        key: String,
    },
    /// The key holds a keyword other than `choice`; a list written as its
    /// entries holds no keyword.
    IsNot {
        /// The key's dotted path.
        key: String,
        /// The keyword refused.
        choice: String,
    },
    /// The key holds a value other than the one `other` holds.
    Differs {
        /// The key's dotted path.
        key: String,
        /// The dotted path of the key it must differ from.
        other: String,
    },
    /// No two entries of the list of tables hold the same value under the
    /// key.
    UniqueEntries {
        /// The key's dotted path, inside its list of tables.
        key: String,
    },
    /// The byte size fits the platform's address width: at most 2^32 - 1
    /// bytes on a 32-bit platform.
    FitsAddressWidth {
        /// The key's dotted path.
        key: String,
    },
    /// The comparison `limit` holds.
    Holds {
        /// The dotted path of the key compared.
        key: String,
        /// The comparison, written as a key's limits are.
        limit: LimitDescriptor,
    },
}

/// Every compound rule, in the order the validator runs them.
pub(crate) fn rule_descriptors() -> Vec<RuleDescriptor> {
    compound_rules()
        .map(|rule: CompoundRule| RuleDescriptor {
            keys: rule.keys().iter().map(ToString::to_string).collect(),
            statement: rule.statement,
            when: rule.when.iter().copied().map(condition_descriptor).collect(),
            then: rule.then.iter().copied().map(obligation_descriptor).collect(),
        })
        .collect()
}

/// One condition as the descriptor writes it.
fn condition_descriptor(condition: Condition) -> ConditionDescriptor {
    match condition {
        Condition::Is { key, choices } => ConditionDescriptor::Is {
            key: key.to_string(),
            choices: choices.iter().map(ToString::to_string).collect(),
        },
        Condition::IsNot { key, choice } => ConditionDescriptor::IsNot {
            key: key.to_string(),
            choice: choice.to_string(),
        },
        Condition::TlsListener => ConditionDescriptor::TlsListener {
            any_of: TLS_LISTENERS
                .iter()
                .map(|listener| listener.iter().copied().map(condition_descriptor).collect())
                .collect(),
        },
    }
}

/// One obligation as the descriptor writes it.
fn obligation_descriptor(obligation: Obligation) -> ObligationDescriptor {
    match obligation {
        Obligation::Present(key) => ObligationDescriptor::Present { key: key.to_string() },
        Obligation::Absent(key) => ObligationDescriptor::Absent { key: key.to_string() },
        Obligation::NonEmpty(key) => ObligationDescriptor::NonEmpty { key: key.to_string() },
        Obligation::Empty(key) => ObligationDescriptor::Empty { key: key.to_string() },
        Obligation::IsNot { key, choice } => ObligationDescriptor::IsNot {
            key: key.to_string(),
            choice: choice.to_string(),
        },
        Obligation::Differs { key, other } => ObligationDescriptor::Differs {
            key: key.to_string(),
            other: other.to_string(),
        },
        Obligation::UniqueEntries(key) => ObligationDescriptor::UniqueEntries { key: key.to_string() },
        Obligation::FitsAddressWidth(key) => ObligationDescriptor::FitsAddressWidth { key: key.to_string() },
        Obligation::Holds(rule) => ObligationDescriptor::Holds {
            key: rule.key().to_string(),
            limit: limit_descriptor(KeyLimit::Declared(rule)),
        },
    }
}
