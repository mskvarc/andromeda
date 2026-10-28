//! Every function call, relation and column reference a syntax tree holds,
//! wherever in the tree it is.
//!
//! The parser's tree has some three hundred node kinds, and a call or a
//! relation may sit below almost any of them (a column default, a check, a
//! generated column, an array literal, a view). Rather than a hand-kept walk
//! that misses the node kinds it forgets, the tree is serialised as the parser
//! crate describes it and read back through a visitor that descends into every
//! map and sequence, so no node can be skipped. Only the node kinds a rule
//! reads are captured, each into a typed shape. A node held in a field typed
//! as that node (a statement's `relation`, a `call`'s function) is written
//! without its kind's name, so a relation is recognised by its `relname`
//! field and the call of `call` by the field holding it.

use crate::declared_schema::qualified_name::QualifiedName;
use serde::{
    Deserialize,
    Deserializer,
    Serialize,
    de::{DeserializeSeed, IgnoredAny, MapAccess, SeqAccess, Visitor},
};
use std::fmt;

/// What a walk of a syntax tree found.
#[derive(Debug, Default)]
pub struct SyntaxSites {
    /// Every function call, `greatest` and `least` included.
    calls: Vec<CallSite>,
    /// Every relation named in a `from`, a join, a `references` or a statement
    /// subject.
    relations: Vec<NameSite>,
    /// Every column reference.
    columns: Vec<ColumnSite>,
    /// The name of every common table expression defined.
    common_tables: Vec<String>,
}

impl SyntaxSites {
    /// Walk `node` and everything below it.
    ///
    /// # Errors
    ///
    /// Returns the serialisation error when the tree cannot be written or read
    /// back, which the parser crate's own types never cause.
    pub fn of(node: &impl Serialize) -> Result<SyntaxSites, serde_json::Error> {
        let written = serde_json::to_string(node)?;
        let mut sites = SyntaxSites::default();
        Walk { sites: &mut sites }.deserialize(&mut serde_json::Deserializer::from_str(&written))?;
        Ok(sites)
    }

    /// Every function call.
    #[must_use]
    pub fn calls(&self) -> &[CallSite] {
        &self.calls
    }

    /// Every relation named.
    #[must_use]
    pub fn relations(&self) -> &[NameSite] {
        &self.relations
    }

    /// Every column reference.
    #[must_use]
    pub fn columns(&self) -> &[ColumnSite] {
        &self.columns
    }

    /// The name of every common table expression defined.
    #[must_use]
    pub fn common_tables(&self) -> &[String] {
        &self.common_tables
    }
}

/// A function called, and where.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CallSite {
    /// The function.
    name: QualifiedName,
    /// The byte offset of the call in the parsed text, when the parser knows it.
    offset: Option<usize>,
}

impl CallSite {
    /// The function.
    #[must_use]
    pub const fn name(&self) -> &QualifiedName {
        &self.name
    }

    /// The byte offset of the call in the parsed text, when the parser knows it.
    #[must_use]
    pub const fn offset(&self) -> Option<usize> {
        self.offset
    }
}

/// A relation named, and where.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NameSite {
    /// The relation.
    name: QualifiedName,
    /// The byte offset of the name in the parsed text, when the parser knows it.
    offset: Option<usize>,
}

impl NameSite {
    /// The relation.
    #[must_use]
    pub const fn name(&self) -> &QualifiedName {
        &self.name
    }

    /// The byte offset of the name in the parsed text, when the parser knows it.
    #[must_use]
    pub const fn offset(&self) -> Option<usize> {
        self.offset
    }
}

/// A column referenced, and where.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ColumnSite {
    /// The column, last, after the relation it is qualified with, if any.
    path: Vec<String>,
    /// The byte offset of the reference in the parsed text, when known.
    offset: Option<usize>,
}

impl ColumnSite {
    /// The column's own name, the last part of the reference; `None` for `*`.
    #[must_use]
    pub fn column(&self) -> Option<&str> {
        self.path.last().map(String::as_str)
    }

    /// The byte offset of the reference in the parsed text, when known.
    #[must_use]
    pub const fn offset(&self) -> Option<usize> {
        self.offset
    }
}

/// A parser location, which is -1 when the parser does not know it.
fn offset_of(location: i32) -> Option<usize> {
    usize::try_from(location).ok()
}

/// One element of a name list: a string, or the `*` of a column reference.
#[derive(Debug, Deserialize)]
struct NamePart {
    /// The node.
    node: NamePartNode,
}

/// The node kinds a name list holds.
#[derive(Debug, Deserialize)]
enum NamePartNode {
    /// A name.
    String(NameString),
    /// The `*` of `relation.*`.
    AStar(IgnoredAny),
}

/// A string node.
#[derive(Debug, Deserialize)]
struct NameString {
    /// The string.
    sval: String,
}

/// The strings of `parts`, a `*` dropped.
fn strings_of(parts: Vec<NamePart>) -> Vec<String> {
    parts
        .into_iter()
        .filter_map(|part| match part.node {
            NamePartNode::String(name) => Some(name.sval),
            NamePartNode::AStar(_) => None,
        })
        .collect()
}

/// A `ColumnRef` node: a column reference.
#[derive(Debug, Deserialize)]
struct ColumnRefShape {
    /// The qualified column.
    fields: Vec<NamePart>,
    /// Where it is.
    location: i32,
}

/// The node kinds, or the fields holding one, the walk captures.
#[derive(Debug, Clone, Copy, PartialEq, Eq, strum::EnumString)]
enum CapturedNode {
    /// A function call.
    FuncCall,
    /// The function call of a `call` statement.
    #[strum(serialize = "funccall")]
    CalledFunction,
    /// `greatest` or `least`.
    MinMaxExpr,
    /// A column reference.
    ColumnRef,
    /// A common table expression.
    CommonTableExpr,
}

/// The fields of a relation name a map is recognised by.
#[derive(Debug, Clone, Copy, PartialEq, Eq, strum::EnumString)]
#[strum(serialize_all = "lowercase")]
enum RelationField {
    /// The schema, empty when unqualified.
    Schemaname,
    /// The relation.
    Relname,
    /// Where the name is.
    Location,
}

/// The `op` of a `MinMaxExpr` that means `greatest`; any other means `least`.
const GREATEST: i32 = 1;

/// The visitor that descends into everything. The traits are implemented by
/// hand: a visitor recording into shared state is not a derivable shape.
struct Walk<'sites> {
    /// Where findings are recorded.
    sites: &'sites mut SyntaxSites,
}

impl<'de> DeserializeSeed<'de> for Walk<'_> {
    type Value = ();

    fn deserialize<D: Deserializer<'de>>(self, deserializer: D) -> Result<(), D::Error> {
        deserializer.deserialize_any(self)
    }
}

impl<'de> Visitor<'de> for Walk<'_> {
    type Value = ();

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a parser syntax tree")
    }

    fn visit_bool<E>(self, _: bool) -> Result<(), E> {
        Ok(())
    }

    fn visit_i64<E>(self, _: i64) -> Result<(), E> {
        Ok(())
    }

    fn visit_u64<E>(self, _: u64) -> Result<(), E> {
        Ok(())
    }

    fn visit_f64<E>(self, _: f64) -> Result<(), E> {
        Ok(())
    }

    fn visit_str<E>(self, _: &str) -> Result<(), E> {
        Ok(())
    }

    fn visit_unit<E>(self) -> Result<(), E> {
        Ok(())
    }

    fn visit_none<E>(self) -> Result<(), E> {
        Ok(())
    }

    fn visit_some<D: Deserializer<'de>>(self, deserializer: D) -> Result<(), D::Error> {
        deserializer.deserialize_any(self)
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut sequence: A) -> Result<(), A::Error> {
        while sequence.next_element_seed(Walk { sites: &mut *self.sites })?.is_some() {}
        Ok(())
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<(), A::Error> {
        let mut schema = String::new();
        let mut relation = None;
        let mut location = -1;
        while let Some(key) = map.next_key::<String>()? {
            if let Ok(field) = key.parse::<RelationField>() {
                match field {
                    RelationField::Schemaname => schema = map.next_value()?,
                    RelationField::Relname => relation = Some(map.next_value::<String>()?),
                    RelationField::Location => location = map.next_value()?,
                }
                continue;
            }
            match key.parse::<CapturedNode>() {
                Ok(CapturedNode::ColumnRef) => {
                    let column: ColumnRefShape = map.next_value()?;
                    self.sites.columns.push(ColumnSite {
                        path: strings_of(column.fields),
                        offset: offset_of(column.location),
                    });
                }
                Ok(node @ (CapturedNode::FuncCall | CapturedNode::CalledFunction | CapturedNode::MinMaxExpr | CapturedNode::CommonTableExpr)) => {
                    map.next_value_seed(Capture { node, sites: &mut *self.sites })?;
                }
                Err(_) => map.next_value_seed(Walk { sites: &mut *self.sites })?,
            }
        }
        if let Some(relation) = relation {
            self.sites.relations.push(NameSite {
                name: QualifiedName::of_written(&schema, &relation),
                offset: offset_of(location),
            });
        }
        Ok(())
    }
}

/// The visitor of a node whose own fields are captured and whose children are
/// walked.
struct Capture<'sites> {
    /// The node kind.
    node: CapturedNode,
    /// Where findings are recorded.
    sites: &'sites mut SyntaxSites,
}

impl<'de> DeserializeSeed<'de> for Capture<'_> {
    type Value = ();

    fn deserialize<D: Deserializer<'de>>(self, deserializer: D) -> Result<(), D::Error> {
        deserializer.deserialize_map(self)
    }
}

impl<'de> Visitor<'de> for Capture<'_> {
    type Value = ();

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a captured syntax tree node")
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<(), A::Error> {
        let mut name = Vec::new();
        let mut location = -1;
        let mut op = 0;
        while let Some(key) = map.next_key::<String>()? {
            match (self.node, key.as_str()) {
                (CapturedNode::FuncCall | CapturedNode::CalledFunction, "funcname") => name = strings_of(map.next_value()?),
                (CapturedNode::CommonTableExpr, "ctename") => name = vec![map.next_value()?],
                (CapturedNode::MinMaxExpr, "op") => op = map.next_value()?,
                (CapturedNode::FuncCall | CapturedNode::CalledFunction | CapturedNode::MinMaxExpr, "location") => location = map.next_value()?,
                (_, _) => map.next_value_seed(Walk { sites: &mut *self.sites })?,
            }
        }
        let offset = offset_of(location);
        match self.node {
            CapturedNode::FuncCall | CapturedNode::CalledFunction => {
                let parts: Vec<&str> = name.iter().map(String::as_str).collect();
                if let Some(name) = QualifiedName::of_parts(&parts) {
                    self.sites.calls.push(CallSite { name, offset });
                }
            }
            CapturedNode::MinMaxExpr => self.sites.calls.push(CallSite {
                name: QualifiedName::unqualified(if op == GREATEST { "greatest" } else { "least" }),
                offset,
            }),
            CapturedNode::CommonTableExpr => self.sites.common_tables.extend(name),
            CapturedNode::ColumnRef => {}
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crate::declared_schema::syntax_walk::SyntaxSites;
    use pretty_assertions::assert_eq;

    /// What a walk of the first statement of `sql` finds, as text.
    fn walked(sql: &str) -> (Vec<String>, Vec<String>, Vec<String>, Vec<String>) {
        let parsed = pg_query::parse(sql, 0).ok();
        let node = parsed
            .and_then(|parsed| parsed.protobuf.stmts.into_iter().next())
            .and_then(|statement| statement.stmt)
            .and_then(|statement| statement.node);
        let Some(Ok(sites)) = node.as_ref().map(SyntaxSites::of) else {
            return Default::default();
        };
        let written = |schema: Option<&str>, name: &str| schema.map_or_else(|| name.to_owned(), |schema| format!("{schema}.{name}"));
        (
            sites.calls().iter().map(|call| written(call.name().schema(), call.name().name())).collect(),
            sites
                .relations()
                .iter()
                .map(|relation| written(relation.name().schema(), relation.name().name()))
                .collect(),
            sites.columns().iter().filter_map(|column| column.column().map(str::to_owned)).collect(),
            sites.common_tables().to_vec(),
        )
    }

    #[test]
    fn calls_relations_and_columns_are_found_at_any_depth() {
        let (calls, relations, columns, common_tables) =
            walked("with c as (select 1) select greatest(a.x, 1), s.f(array[lower(y)]) from public.t a, c where b in (select count(*) from u)");
        assert_eq!(calls, ["greatest", "lower", "s.f", "count"]);
        assert_eq!(relations, ["public.t", "c", "u"]);
        assert_eq!(columns, ["x", "y", "b"]);
        assert_eq!(common_tables, ["c"]);
    }

    #[test]
    fn a_call_in_a_column_default_and_a_check_is_found() {
        let (calls, relations, ..) =
            walked("create table t (a xid8 default pg_current_xact_id(), b text check (octet_length(b) <= 8), c bigint references r (c))");
        assert_eq!(calls, ["pg_current_xact_id", "octet_length"]);
        assert_eq!(relations, ["t", "r"]);
    }
}
