//! A possibly schema-qualified object name, as the parser reports it.

use pg_query::{Node, NodeEnum, protobuf::RangeVar};

/// An object name with the schema it is qualified with, if any.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct QualifiedName {
    /// The schema written before the name.
    schema: Option<String>,
    /// The name itself.
    name: String,
}

impl QualifiedName {
    /// An unqualified name.
    #[must_use]
    pub fn unqualified(name: impl Into<String>) -> QualifiedName {
        QualifiedName {
            schema: None,
            name: name.into(),
        }
    }

    /// The name a list of string nodes spells, `[schema, name]` or `[name]`;
    /// `None` when the list is empty or holds a node that is not a string.
    #[must_use]
    pub fn of_nodes(nodes: &[Node]) -> Option<QualifiedName> {
        let parts = nodes
            .iter()
            .map(|node| match &node.node {
                Some(NodeEnum::String(part)) => Some(part.sval.as_str()),
                Some(_) | None => None,
            })
            .collect::<Option<Vec<_>>>()?;
        QualifiedName::of_parts(&parts)
    }

    /// The name `parts` spell, the last part the name and the one before it
    /// the schema.
    #[must_use]
    pub fn of_parts(parts: &[&str]) -> Option<QualifiedName> {
        let (name, qualifiers) = parts.split_last()?;
        Some(QualifiedName {
            schema: qualifiers.last().map(|schema| (*schema).to_owned()),
            name: (*name).to_owned(),
        })
    }

    /// The relation `relation` names.
    #[must_use]
    pub fn of_relation(relation: &RangeVar) -> QualifiedName {
        QualifiedName::of_written(&relation.schemaname, &relation.relname)
    }

    /// `name` qualified with `schema`, which the parser reports empty for an
    /// unqualified name.
    #[must_use]
    pub fn of_written(schema: &str, name: &str) -> QualifiedName {
        QualifiedName {
            schema: Some(schema).filter(|schema| !schema.is_empty()).map(str::to_owned),
            name: name.to_owned(),
        }
    }

    /// The schema written before the name.
    #[must_use]
    pub fn schema(&self) -> Option<&str> {
        self.schema.as_deref()
    }

    /// The name itself.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }
}

#[cfg(test)]
mod tests {
    use crate::declared_schema::qualified_name::QualifiedName;
    use pretty_assertions::assert_eq;

    #[test]
    fn the_last_part_is_the_name_and_the_one_before_it_the_schema() {
        let name = QualifiedName::of_parts(&["db", "public", "entities"]);
        assert_eq!(name.as_ref().map(|name| (name.schema(), name.name())), Some((Some("public"), "entities")));
        assert_eq!(QualifiedName::of_parts(&[]), None);
    }
}
