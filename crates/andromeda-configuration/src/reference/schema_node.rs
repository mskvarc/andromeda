//! The part of the configuration schema the reference reads: each node's
//! operator description and the nodes nested in it.

use crate::{
    reference::{ReferenceError, json_schema::declared_schema},
    schema::apply_mode::ApplyMode,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// The `type` of a schema node: one JSON type name or several.
#[derive(Debug, Default, Deserialize)]
#[serde(untagged)]
pub(crate) enum JsonTypes {
    /// No type is stated.
    #[default]
    Unstated,
    /// One type.
    One(String),
    /// Any of several types.
    Several(Vec<String>),
}

impl JsonTypes {
    /// The stated types other than `null`, which TOML cannot write.
    fn writable(&self) -> Vec<&str> {
        match self {
            JsonTypes::Unstated => Vec::new(),
            JsonTypes::One(name) => [name.as_str()].into_iter().filter(|name| *name != "null").collect(),
            JsonTypes::Several(names) => names.iter().map(String::as_str).filter(|name| *name != "null").collect(),
        }
    }
}

/// One allowed value of a choice and what it means.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct Choice {
    /// The value as TOML writes it.
    pub(crate) value: String,
    /// What it means; empty only when the schema says nothing of it, which the
    /// descriptor test refuses.
    pub(crate) description: String,
}

/// One schema node, read back from the schema's JSON text.
///
/// Every member the reference does not read, such as a node's type or its
/// bounds, is left out of the reading.
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SchemaNode {
    /// The operator description; absent on a node that carries none.
    #[serde(default)]
    description: Option<String>,
    /// Whether a change to the key reaches a running broker.
    #[serde(default, rename = "x-andromeda-apply")]
    apply: ApplyMode,
    /// Whether every replica behind one load balancer must hold the same value.
    #[serde(default, rename = "x-andromeda-replica-consistent")]
    replica_consistent: bool,
    /// The short name of the value's type, where the configuration gives one.
    #[serde(default)]
    title: Option<String>,
    /// The JSON types the value may take.
    #[serde(default, rename = "type")]
    types: JsonTypes,
    /// The format of a string, such as `uri`.
    #[serde(default)]
    format: Option<String>,
    /// The smallest number allowed.
    #[serde(default)]
    minimum: Option<f64>,
    /// The values allowed, `None` standing for JSON `null`.
    #[serde(default, rename = "enum")]
    allowed: Vec<Option<String>>,
    /// The one value allowed.
    #[serde(default, rename = "const")]
    constant: Option<String>,
    /// The values a choice may take, each with its own description.
    #[serde(default)]
    one_of: Vec<SchemaNode>,
    /// The keys of a table, by name.
    #[serde(default)]
    properties: BTreeMap<String, SchemaNode>,
    /// The keys a table must hold.
    #[serde(default)]
    required: Vec<String>,
    /// The shapes a value may take; an optional table is one of them.
    #[serde(default)]
    any_of: Vec<SchemaNode>,
    /// The shape of each member of an array.
    #[serde(default)]
    items: Option<Box<SchemaNode>>,
}

impl SchemaNode {
    /// The schema of the whole configuration file, as its types declare it.
    ///
    /// # Errors
    ///
    /// Returns [`ReferenceError::Schema`] when the schema cannot be written or
    /// read back.
    pub(crate) fn configuration() -> Result<SchemaNode, ReferenceError> {
        Ok(serde_json::from_str(&serde_json::to_string(&declared_schema())?)?)
    }

    /// The operator description, when the node carries one.
    pub(crate) fn description(&self) -> Option<&str> {
        self.description.as_deref()
    }

    /// How a change to the key reaches a running broker.
    pub(crate) const fn apply(&self) -> ApplyMode {
        self.apply
    }

    /// Whether every replica must hold the same value.
    pub(crate) const fn replica_consistent(&self) -> bool {
        self.replica_consistent
    }

    /// The node at `path` below this one.
    pub(crate) fn descendant<'path>(&self, path: impl IntoIterator<Item = &'path str>) -> Option<&SchemaNode> {
        path.into_iter().try_fold(self, |node, name| node.child(name))
    }

    /// The short name of the value's type: the name the configuration gives
    /// it, or one read from its JSON type, a choice reading as `string`.
    pub(crate) fn type_name(&self) -> String {
        if let Some(title) = &self.title {
            return title.clone();
        }
        let shapes: Vec<String> = self.any_of.iter().map(SchemaNode::type_name).filter(|name| !name.is_empty()).collect();
        if !shapes.is_empty() {
            return shapes.join(" or ");
        }
        if !self.one_of.is_empty() || !self.allowed.is_empty() {
            return String::from("string");
        }
        match self.types.writable().first().copied() {
            Some("integer") if self.minimum.is_some_and(|minimum| minimum >= 1.0) => String::from("positive integer"),
            Some("string") if self.format.as_deref() == Some("uri") => String::from("URL"),
            Some("array") => format!("list of {}", self.items.as_deref().map(SchemaNode::type_name).unwrap_or_default()),
            Some("object") => String::from("table"),
            Some(other) => other.to_owned(),
            None => String::new(),
        }
    }

    /// The values a choice allows, with what each means; empty for a value
    /// that is no choice.
    pub(crate) fn choices(&self) -> Vec<Choice> {
        let listed = self.allowed.iter().flatten().map(|value| Choice {
            value: format!("{value:?}"),
            description: String::new(),
        });
        let described = self.one_of.iter().filter_map(|variant| {
            variant
                .constant
                .as_ref()
                .or_else(|| variant.allowed.iter().flatten().next())
                .map(|value| Choice {
                    value: format!("{value:?}"),
                    description: variant.description.clone().unwrap_or_default(),
                })
        });
        listed.chain(described).chain(self.any_of.iter().flat_map(SchemaNode::choices)).collect()
    }

    /// The node of the key `name` in this table, of a table among the shapes
    /// this value may take, or of the tables in this array.
    pub(crate) fn child(&self, name: &str) -> Option<&SchemaNode> {
        self.properties
            .get(name)
            .or_else(|| self.any_of.iter().find_map(|shape| shape.child(name)))
            .or_else(|| self.items.as_deref().and_then(|member| member.child(name)))
    }

    /// Whether this node is a table whose keys the schema names, rather than
    /// one whose keys are the operator's own.
    pub(crate) fn names_its_keys(&self) -> bool {
        !self.properties.is_empty() || self.any_of.iter().any(SchemaNode::names_its_keys) || self.items.as_deref().is_some_and(SchemaNode::names_its_keys)
    }

    /// Whether the table, or each table of this array, must hold `name`.
    pub(crate) fn requires(&self, name: &str) -> bool {
        self.required.iter().any(|required| required == name)
            || self.any_of.iter().any(|shape| shape.requires(name))
            || self.items.as_deref().is_some_and(|member| member.requires(name))
    }

    /// The dotted path of every key the configuration names below this node:
    /// a value, or a table whose keys are the operator's own.
    #[cfg(test)]
    pub(crate) fn leaf_paths(&self, prefix: &str) -> Vec<String> {
        let tables = self.any_of.iter().chain(self.items.as_deref());
        self.properties
            .iter()
            .flat_map(|(name, node)| {
                let path = if prefix.is_empty() { name.clone() } else { format!("{prefix}.{name}") };
                if node.names_its_keys() { node.leaf_paths(&path) } else { vec![path] }
            })
            .chain(tables.flat_map(|shape| shape.leaf_paths(prefix)))
            .collect()
    }
}
