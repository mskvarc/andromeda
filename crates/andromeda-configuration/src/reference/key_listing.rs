//! The configuration as the list of its tables and keys, in the order the
//! file holds them, each with its operator description and the value it is
//! shown at.

use crate::{
    built_in_defaults::BuiltInDefaults,
    reference::{
        ReferenceError,
        example::example_configuration,
        schema_node::{Choice, ChoiceRange, SchemaNode},
    },
    validate::{compound_rule::CompoundRule, compound_rules, declared_rules, rule::Rule},
};
use derive_more::Display;
use serde::Serialize;
use toml_edit::{DocumentMut, Item, Key, Table, Value};

/// A table or key's dotted path from the root of the file.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct KeyPath(Vec<String>);

impl KeyPath {
    /// The path of `name` inside this table.
    fn child(&self, name: &str) -> KeyPath {
        KeyPath(self.0.iter().cloned().chain([name.to_owned()]).collect())
    }

    /// The path as the file and the documentation spell it, segments joined
    /// by dots.
    pub(crate) fn dotted(&self) -> String {
        self.0.join(".")
    }

    /// The path's segments, from the root.
    pub(crate) fn segments(&self) -> impl Iterator<Item = &str> {
        self.0.iter().map(String::as_str)
    }

    /// Whether this path lies inside `table`, or is it.
    pub(crate) fn starts_with(&self, table: &KeyPath) -> bool {
        self.0.starts_with(&table.0)
    }

    /// The last segment: the key's own name.
    pub(crate) fn name(&self) -> Option<&str> {
        self.0.last().map(String::as_str)
    }
}

/// Whether a table or key holds a value when the file leaves it out.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum KeyState {
    /// It holds its built-in default, the value shown.
    Default,
    /// It is not set; the value shown is an example.
    Unset,
    /// It has no default, and the configuration must set it once its table is
    /// set; the value shown is an example. A key of a table set by default,
    /// such as `database.connection`, must always be set.
    Required,
}

impl KeyState {
    /// What the file line of a table or key starts with: nothing when it is
    /// set by default, a comment mark when it is not.
    pub(crate) const fn line_mark(self) -> &'static str {
        match self {
            KeyState::Default => "",
            KeyState::Unset | KeyState::Required => "# ",
        }
    }
}

/// How a table is written.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum TableShape {
    /// A `[table]` whose keys the configuration names.
    Named,
    /// A `[[table]]` written once per member of a list.
    Array,
    /// A `[table]` whose keys are the operator's own.
    Free,
}

/// One table of the file.
#[derive(Debug)]
pub(crate) struct TableEntry {
    /// Where the table lies.
    pub(crate) path: KeyPath,
    /// How it is written.
    pub(crate) shape: TableShape,
    /// Whether it is set by default.
    pub(crate) state: KeyState,
    /// Its operator description.
    pub(crate) description: Option<String>,
    /// A free table's `key = value` lines, at their default or example.
    pub(crate) members: Vec<String>,
}

impl TableEntry {
    /// The table's header as the file writes it, commented out when the table
    /// is not set by default.
    pub(crate) fn header_line(&self) -> String {
        let path = self.path.dotted();
        match self.shape {
            TableShape::Named | TableShape::Free => format!("{}[{path}]", self.state.line_mark()),
            TableShape::Array => format!("{}[[{path}]]", self.state.line_mark()),
        }
    }

    /// A free table's member lines, commented out when the table is not set
    /// by default.
    pub(crate) fn member_lines(&self) -> impl Iterator<Item = String> {
        self.members.iter().map(|member| format!("{}{member}", self.state.line_mark()))
    }
}

/// One condition a key's value meets, displayed as the condition it states.
#[derive(Debug, Clone, Copy, Display)]
pub(crate) enum KeyLimit {
    /// A comparison the validator runs.
    #[display("{_0}")]
    Declared(Rule),
    /// A floor the key's type holds: a non-zero count cannot spell zero, so
    /// no rule needs to check it.
    #[display("above zero")]
    PositiveByType,
}

/// One key of the file.
#[derive(Debug)]
pub(crate) struct KeyEntry {
    /// Where the key lies.
    pub(crate) path: KeyPath,
    /// Whether it is set by default.
    pub(crate) state: KeyState,
    /// Its value as TOML: its default, or an example when it is not set.
    pub(crate) value: String,
    /// Its operator description.
    pub(crate) description: Option<String>,
    /// The values it allows, with what each means; empty for a key that is
    /// no choice.
    pub(crate) choices: Vec<Choice>,
    /// Whether it takes other values beside its choices.
    pub(crate) choice_range: ChoiceRange,
    /// Every condition its value meets: the comparisons the validator holds
    /// it to, and a floor its type holds.
    pub(crate) limits: Vec<KeyLimit>,
    /// Every rule beyond a comparison that involves it.
    pub(crate) conditions: Vec<&'static str>,
}

impl KeyEntry {
    /// The key as the file writes it inside its table, commented out when it
    /// is not set by default.
    pub(crate) fn line(&self) -> String {
        format!("{}{} = {}", self.state.line_mark(), self.path.name().unwrap_or_default(), self.value)
    }
}

/// One line of the listing.
#[derive(Debug)]
pub(crate) enum ReferenceEntry {
    /// A table header.
    Table(TableEntry),
    /// A key inside the table listed last before it.
    Key(KeyEntry),
}

/// Every table and key of the configuration, each table's keys before its
/// nested tables, as TOML requires.
///
/// # Errors
///
/// Returns a [`ReferenceError`] when the schema, the defaults or the example
/// configuration cannot be written and read back.
pub(crate) fn key_listing() -> Result<Vec<ReferenceEntry>, ReferenceError> {
    let schema = SchemaNode::configuration()?;
    let defaults = document(&BuiltInDefaults::default())?;
    let example = document(&example_configuration()?)?;
    let mut entries = Vec::new();
    list_table(&mut entries, &KeyPath::default(), example.as_table(), Some(defaults.as_table()), &schema);
    let rules: Vec<Rule> = declared_rules().collect();
    let compound: Vec<CompoundRule> = compound_rules().collect();
    for entry in &mut entries {
        if let ReferenceEntry::Key(key) = entry {
            let path = key.path.dotted();
            let declared: Vec<Rule> = rules.iter().copied().filter(|rule| rule.key().to_string() == path).collect();
            let positive_by_type = schema.descendant(key.path.segments()).is_some_and(SchemaNode::is_positive_integer)
                && !declared.iter().any(|rule| matches!(rule, Rule::Positive(_)));
            key.limits = positive_by_type
                .then_some(KeyLimit::PositiveByType)
                .into_iter()
                .chain(declared.into_iter().map(KeyLimit::Declared))
                .collect();
            key.conditions = compound
                .iter()
                .filter(|rule| rule.keys().iter().any(|named| named.to_string() == path))
                .map(|rule| rule.statement)
                .collect();
        }
    }
    Ok(entries)
}

/// A configuration, or its defaults, as a TOML document.
fn document(config: &impl Serialize) -> Result<DocumentMut, ReferenceError> {
    Ok(toml::to_string(config)?.parse::<DocumentMut>()?)
}

/// List the keys of the `example` table at `path`, then its tables.
///
/// `defaults` is the same table in the built-in defaults, `None` when the
/// defaults leave it out.
fn list_table(entries: &mut Vec<ReferenceEntry>, path: &KeyPath, example: &Table, defaults: Option<&Table>, schema: &SchemaNode) {
    for (name, value) in example.iter().filter_map(|(name, item)| item.as_value().map(|value| (name, value))) {
        let default = defaults.and_then(|table| table.get(name)).and_then(Item::as_value);
        let state = match default {
            Some(_) => KeyState::Default,
            None if schema.requires(name) => KeyState::Required,
            None => KeyState::Unset,
        };
        entries.push(ReferenceEntry::Key(KeyEntry {
            path: path.child(name),
            state,
            value: literal(default.unwrap_or(value)),
            description: described(schema.child(name)),
            choices: schema.child(name).map(SchemaNode::choices).unwrap_or_default(),
            choice_range: schema.child(name).map_or(ChoiceRange::Closed, SchemaNode::choice_range),
            limits: Vec::new(),
            conditions: Vec::new(),
        }));
    }
    for (name, item) in example {
        let node = schema.child(name);
        let default = defaults.and_then(|table| table.get(name));
        let state = if default.is_some() { KeyState::Default } else { KeyState::Unset };
        match item {
            Item::Table(table) if node.is_some_and(SchemaNode::names_its_keys) => {
                entries.push(table_entry(path.child(name), TableShape::Named, state, node, Vec::new()));
                list_table(
                    entries,
                    &path.child(name),
                    table,
                    default.and_then(Item::as_table),
                    node.unwrap_or(&SchemaNode::default()),
                );
            }
            Item::Table(table) => {
                let shown = default.and_then(Item::as_table).unwrap_or(table);
                entries.push(table_entry(path.child(name), TableShape::Free, state, node, free_members(shown)));
            }
            Item::ArrayOfTables(array) => {
                if let Some(first) = array.iter().next() {
                    entries.push(table_entry(path.child(name), TableShape::Array, state, node, Vec::new()));
                    let default_first = default.and_then(Item::as_array_of_tables).and_then(|tables| tables.iter().next());
                    list_table(entries, &path.child(name), first, default_first, node.unwrap_or(&SchemaNode::default()));
                }
            }
            Item::Value(_) | Item::None => {}
        }
    }
}

/// The listing entry of one table.
fn table_entry(path: KeyPath, shape: TableShape, state: KeyState, node: Option<&SchemaNode>, members: Vec<String>) -> ReferenceEntry {
    ReferenceEntry::Table(TableEntry {
        path,
        shape,
        state,
        description: described(node),
        members,
    })
}

/// A free table's `key = value` lines.
fn free_members(table: &Table) -> Vec<String> {
    table
        .iter()
        .filter_map(|(name, item)| item.as_value().map(|value| format!("{} = {}", Key::new(name), literal(value))))
        .collect()
}

/// The operator description of a node, when there is one.
fn described(node: Option<&SchemaNode>) -> Option<String> {
    node.and_then(SchemaNode::description).map(str::to_owned)
}

/// A value as TOML, without the spacing around it in the document.
fn literal(value: &Value) -> String {
    value.to_string().trim().to_owned()
}
