//! An index, as a migration declares it.

use crate::declared_schema::{origin::Origin, qualified_name::QualifiedName, statement_context::StatementContext, syntax_walk::SyntaxSites};
use pg_query::{
    Node,
    NodeEnum,
    protobuf::{IndexElem, IndexStmt, Token},
};
use std::ops::Range;

/// The access method an index without `using` is built with.
const DEFAULT_ACCESS_METHOD: &str = "btree";

/// One key of an index: a column or an expression.
#[derive(Debug, Clone)]
pub struct IndexElement {
    /// The column, for a column key.
    column: Option<String>,
    /// The collation the key overrides the column's with, if any.
    collation: Option<QualifiedName>,
    /// The operator class it names, if any.
    operator_class: Option<QualifiedName>,
    /// The key as written, its whitespace collapsed.
    written: String,
}

impl IndexElement {
    /// The column, for a column key.
    #[must_use]
    pub fn column(&self) -> Option<&str> {
        self.column.as_deref()
    }

    /// The collation the key overrides the column's with, if any.
    #[must_use]
    pub const fn collation(&self) -> Option<&QualifiedName> {
        self.collation.as_ref()
    }

    /// The operator class it names, if any.
    #[must_use]
    pub const fn operator_class(&self) -> Option<&QualifiedName> {
        self.operator_class.as_ref()
    }

    /// The key as written, its whitespace collapsed.
    #[must_use]
    pub fn written(&self) -> &str {
        &self.written
    }
}

/// One index.
#[derive(Debug, Clone)]
pub struct DeclaredIndex {
    /// The table indexed.
    table: String,
    /// The index's name.
    name: String,
    /// Whether it is unique.
    unique: bool,
    /// Its access method.
    method: String,
    /// Its keys, in order.
    elements: Vec<IndexElement>,
    /// The columns it includes beside its keys.
    included: Vec<String>,
    /// Its predicate, as written with its whitespace collapsed, for a partial index.
    predicate: Option<String>,
    /// Every column its keys, expressions and predicate name.
    referenced_columns: Vec<String>,
    /// Every function its key expressions and predicate call.
    called_functions: Vec<QualifiedName>,
    /// Where its name is written.
    origin: Origin,
    /// The bytes of the migration holding its definition after its name.
    definition: Range<usize>,
}

impl DeclaredIndex {
    /// The index `index` declares, read from `statement`.
    ///
    /// # Errors
    ///
    /// Returns the serialisation error of a syntax walk that fails, which the
    /// parser crate's own types never cause.
    pub fn read(index: &IndexStmt, statement: &StatementContext<'_>) -> Result<DeclaredIndex, serde_json::Error> {
        let origin = statement.origin_of_name(&index.idxname);
        let (keys, predicate) = written_keys_and_predicate(statement);
        let sites = SyntaxSites::of(index)?;
        let elements = index
            .index_params
            .iter()
            .filter_map(element_of)
            .enumerate()
            .map(|(position, element)| IndexElement {
                column: Some(element.name.as_str()).filter(|name| !name.is_empty()).map(str::to_owned),
                collation: QualifiedName::of_nodes(&element.collation),
                operator_class: QualifiedName::of_nodes(&element.opclass),
                written: keys.get(position).cloned().unwrap_or_default(),
            })
            .collect();
        Ok(DeclaredIndex {
            table: index.relation.as_ref().map_or_default(|relation| relation.relname.clone()),
            name: index.idxname.clone(),
            unique: index.unique,
            method: Some(index.access_method.as_str())
                .filter(|method| !method.is_empty())
                .unwrap_or(DEFAULT_ACCESS_METHOD)
                .to_owned(),
            elements,
            included: index
                .index_including_params
                .iter()
                .filter_map(element_of)
                .map(|element| element.name.clone())
                .collect(),
            predicate,
            referenced_columns: sites.columns().iter().filter_map(|column| column.column().map(str::to_owned)).collect(),
            called_functions: sites.calls().iter().map(|call| call.name().clone()).collect(),
            definition: statement.rest_from(statement.after_name(origin.offset(), &index.idxname)),
            origin,
        })
    }

    /// The table indexed.
    #[must_use]
    pub fn table(&self) -> &str {
        &self.table
    }

    /// The index's name.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Whether it is unique.
    #[must_use]
    pub const fn is_unique(&self) -> bool {
        self.unique
    }

    /// Its access method.
    #[must_use]
    pub fn method(&self) -> &str {
        &self.method
    }

    /// Its keys, in order.
    #[must_use]
    pub fn elements(&self) -> &[IndexElement] {
        &self.elements
    }

    /// The columns it includes beside its keys.
    #[must_use]
    pub fn included(&self) -> &[String] {
        &self.included
    }

    /// Its predicate, as written with its whitespace collapsed, for a partial index.
    #[must_use]
    pub fn predicate(&self) -> Option<&str> {
        self.predicate.as_deref()
    }

    /// Every column its keys, expressions and predicate name.
    #[must_use]
    pub fn referenced_columns(&self) -> &[String] {
        &self.referenced_columns
    }

    /// Every function its key expressions and predicate call.
    #[must_use]
    pub fn called_functions(&self) -> &[QualifiedName] {
        &self.called_functions
    }

    /// Where its name is written.
    #[must_use]
    pub const fn origin(&self) -> Origin {
        self.origin
    }

    /// The bytes of the migration holding its definition after its name.
    #[must_use]
    pub fn definition(&self) -> Range<usize> {
        self.definition.clone()
    }
}

/// The index element `node` holds.
fn element_of(node: &Node) -> Option<&IndexElem> {
    match &node.node {
        Some(NodeEnum::IndexElem(element)) => Some(element),
        Some(_) | None => None,
    }
}

/// `text` with every run of whitespace collapsed to one space.
fn collapsed(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// The keys of the index statement as written, and its predicate: the
/// parenthesised list after `on <table>`, split at its own commas, and what
/// follows `where`.
fn written_keys_and_predicate(statement: &StatementContext<'_>) -> (Vec<String>, Option<String>) {
    let migration = statement.migration();
    let text = migration.text();
    let mut keys = Vec::new();
    let mut predicate = None;
    let mut depth = 0_usize;
    let mut seen_on = false;
    let mut key_list_done = false;
    let mut key_start = None;
    for token in migration.code_tokens(statement.span()) {
        let kind = token.kind();
        let range = token.range();
        if kind == Token::On && depth == 0 {
            seen_on = true;
        } else if kind == Token::Ascii40 {
            if seen_on && !key_list_done && depth == 0 {
                key_start = Some(range.end);
            }
            depth += 1;
        } else if kind == Token::Ascii41 {
            depth = depth.saturating_sub(1);
            if depth == 0
                && let Some(start) = key_start.take()
            {
                keys.push(collapsed(&text[start..range.start]));
                key_list_done = true;
            }
        } else if kind == Token::Ascii44 && depth == 1 && !key_list_done {
            if let Some(start) = key_start.replace(range.end) {
                keys.push(collapsed(&text[start..range.start]));
            }
        } else if kind == Token::Where && depth == 0 {
            predicate = Some(collapsed(&text[statement.rest_from(range.end)]));
        }
    }
    (keys, predicate)
}
