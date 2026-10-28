//! The configuration descriptor: every table and key with what a tool needs to
//! present it to an operator, as JSON.
//!
//! The descriptor is the configuration's own vocabulary, not a schema: each
//! key's type under the name an operator reads, its exact default, the choices
//! it allows with their meaning, its environment variable, and every rule the
//! validator holds it to. A documentation site renders it however it presents
//! settings.

use crate::{
    load::ENVIRONMENT_PREFIX,
    redact::SENSITIVE_KEYS,
    reference::{
        ReferenceError,
        key_listing::{KeyEntry, KeyState, ReferenceEntry, TableEntry, TableShape, key_listing},
        schema_node::{Choice, SchemaNode},
    },
    schema::apply_mode::ApplyMode,
    validate::{
        bound::{Quantity, Relation},
        rule::{Limit, Rule},
    },
};
use humantime::format_duration;
use serde::Serialize;

/// Every table and key of the configuration, in the order the file holds
/// them.
#[derive(Debug, Serialize)]
pub struct ConfigurationDescriptor {
    /// Every table, parents before the tables nested in them.
    tables: Vec<TableDescriptor>,
    /// Every key the configuration names.
    keys: Vec<KeyDescriptor>,
}

/// One table.
#[derive(Debug, Serialize)]
struct TableDescriptor {
    /// The dotted path.
    path: String,
    /// How the file writes it.
    shape: TableShape,
    /// Whether it is set by default.
    presence: KeyState,
    /// The operator description.
    description: String,
}

/// One key.
#[derive(Debug, Serialize)]
struct KeyDescriptor {
    /// The dotted path.
    path: String,
    /// The operator description.
    description: String,
    /// Whether a change reaches a running broker or takes a restart.
    apply: ApplyMode,
    /// Whether every replica must hold the same value.
    replica_consistent: bool,
    /// Whether `configuration show` hides the value.
    sensitive: bool,
    /// The type, under the name an operator reads, such as `duration`.
    type_name: String,
    /// Whether it is set by default.
    presence: KeyState,
    /// The value as TOML: the default when the key is set by default, an
    /// example otherwise.
    value: String,
    /// The values a choice allows, with what each means.
    choices: Vec<Choice>,
    /// The environment variable that sets it; absent for a key inside a list
    /// of tables, which only the file sets.
    environment_variable: Option<String>,
    /// Every comparison the validator holds the key to.
    limits: Vec<LimitDescriptor>,
    /// Every rule beyond a comparison that involves the key.
    conditions: Vec<&'static str>,
}

/// One comparison a key passes.
#[derive(Debug, Serialize)]
struct LimitDescriptor {
    /// The comparison as a phrase, such as `at least 1s`.
    statement: String,
    /// The comparison as data.
    #[serde(flatten)]
    shape: LimitShape,
}

/// What a comparison compares.
#[derive(Debug, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
enum LimitShape {
    /// The key holds more than zero.
    Positive,
    /// The key holds a positive whole number of days.
    WholeDays,
    /// The key stands in `relation` to `bound`.
    Ordered {
        /// How it relates to the bound.
        relation: Relation,
        /// What it is compared with.
        bound: BoundShape,
    },
}

/// What an ordering compares a key with.
#[derive(Debug, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
enum BoundShape {
    /// A limit the broker fixes, in the key's own spelling.
    Fixed {
        /// The limit.
        value: String,
    },
    /// The value another key holds.
    Key {
        /// The other key's dotted path.
        key: String,
    },
    /// The duration another key holds, plus a margin the broker fixes.
    KeyPlusMargin {
        /// The other key's dotted path.
        key: String,
        /// The margin, in the duration spelling.
        margin: String,
    },
}

/// The descriptor of the configuration.
///
/// # Errors
///
/// Returns a [`ReferenceError`] when the configuration cannot be listed.
pub fn configuration_descriptor() -> Result<ConfigurationDescriptor, ReferenceError> {
    let schema = SchemaNode::configuration()?;
    let mut tables: Vec<TableEntry> = Vec::new();
    let mut keys = Vec::new();
    for entry in key_listing()? {
        match entry {
            ReferenceEntry::Table(table) => tables.push(table),
            ReferenceEntry::Key(key) => keys.push(key_descriptor(key, &schema, &tables)),
        }
    }
    Ok(ConfigurationDescriptor {
        tables: tables
            .into_iter()
            .map(|table| TableDescriptor {
                path: table.path.dotted(),
                shape: table.shape,
                presence: table.state,
                description: table.description.unwrap_or_default(),
            })
            .collect(),
        keys,
    })
}

/// The descriptor as pretty-printed JSON text, ending in a newline.
///
/// # Errors
///
/// Returns a [`ReferenceError`] when the configuration cannot be listed or
/// the descriptor cannot be written.
pub fn configuration_descriptor_text() -> Result<String, ReferenceError> {
    let mut text = serde_json::to_string_pretty(&configuration_descriptor()?)?;
    text.push('\n');
    Ok(text)
}

/// The descriptor of one key; `tables` are the tables listed before it.
fn key_descriptor(key: KeyEntry, schema: &SchemaNode, tables: &[TableEntry]) -> KeyDescriptor {
    let path = key.path.dotted();
    let node = schema.descendant(key.path.segments());
    let in_list = tables
        .iter()
        .any(|table| matches!(table.shape, TableShape::Array) && key.path.starts_with(&table.path));
    KeyDescriptor {
        type_name: type_name(node.map(SchemaNode::type_name).unwrap_or_default(), &key.limits),
        choices: node.map(SchemaNode::choices).unwrap_or_default(),
        environment_variable: (!in_list).then(|| {
            let segments: Vec<String> = key.path.segments().map(str::to_uppercase).collect();
            format!("{ENVIRONMENT_PREFIX}{}", segments.join("__"))
        }),
        limits: key.limits.iter().copied().map(limit_descriptor).collect(),
        conditions: key.conditions,
        apply: node.map(SchemaNode::apply).unwrap_or_default(),
        replica_consistent: node.is_some_and(SchemaNode::replica_consistent),
        sensitive: SENSITIVE_KEYS.contains(&path.as_str()),
        path,
        description: key.description.unwrap_or_default(),
        presence: key.state,
        value: key.value,
    }
}

/// The type name, narrowed by a rule that keeps the value above zero.
///
/// A name of several shapes, such as `string or duration`, is left as it is.
fn type_name(name: String, limits: &[Rule]) -> String {
    let positive = limits.iter().any(|rule| matches!(rule, Rule::Positive(_)));
    match name.as_str() {
        "integer" if positive => String::from("positive integer"),
        "positive integer" => name,
        _ if positive && !name.contains(" or ") => format!("non-zero {name}"),
        _ => name,
    }
}

/// One rule as the descriptor writes it.
fn limit_descriptor(rule: Rule) -> LimitDescriptor {
    let shape = match rule {
        Rule::Positive(_) => LimitShape::Positive,
        Rule::WholeDays(_) => LimitShape::WholeDays,
        Rule::Ordered { relation, limit, .. } => LimitShape::Ordered {
            relation,
            bound: match limit {
                Limit::Fixed(Quantity::Duration(value)) => BoundShape::Fixed {
                    value: format_duration(value).to_string(),
                },
                Limit::Fixed(Quantity::Count(value)) => BoundShape::Fixed { value: value.to_string() },
                Limit::Key(key) => BoundShape::Key { key: key.to_string() },
                Limit::KeyPlusMargin(key, margin) => BoundShape::KeyPlusMargin {
                    key: key.to_string(),
                    margin: format_duration(margin).to_string(),
                },
            },
        },
    };
    LimitDescriptor {
        statement: rule.to_string(),
        shape,
    }
}
