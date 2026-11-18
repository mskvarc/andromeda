//! One term of a clause 4.21 projection list (`pick`, `omit`) as the client wrote it, with the linked-entity terms nested in it.

use crate::{projection::expanded_projection_term::ExpandedProjectionTerm, query_error::QueryError};
use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
use andromeda_ngsi_ld_vocabulary::{compact::properties as compact_properties, keyword};
use serde::{Deserialize, Serialize};
use std::fmt::{self, Write};

/// One term of a clause 4.21 raw (pre-expansion) projection list: one attribute and, when it constrains a linked entity's own projection, the `{...}` sub-terms selected on the other side of the relationship it names.
///
/// Serialized as the tree `{"name": ..., "linked": [...]}` rather than as clause 4.21 text: a stored subscription keeps its terms expanded (clause 5.5.7), and an expanded IRI may hold the `,` and `|` the grammar separates terms with, which the clause 4.9 `AttrName` a client writes never does.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WrittenProjectionTerm {
    /// The compact or expanded attribute name this term selects.
    pub name: AttributeName,
    /// The linked-entity sub-projection carried inside `{...}`, empty when the term selects no linked entity.
    pub linked: Vec<WrittenProjectionTerm>,
}

impl WrittenProjectionTerm {
    /// How many Relationship hops this term's clause 4.21 nesting reaches: `0` for a term selecting only members of its own entity, `1` for `rel{a}`, `2` for `rel{inner{a}}`. It is what a `joinLevel` has to cover for the nested terms to name anything the traversal retrieves (clause 5.7.2.4).
    pub fn linked_depth(&self) -> u32 {
        match self.linked.iter().map(WrittenProjectionTerm::linked_depth).max() {
            Some(deepest) => deepest.saturating_add(1),
            None => 0,
        }
    }
}

// Rendering walks the recursive `linked` structure and separates siblings with `,`, a shape `derive_more::Display`'s format-string mode cannot express.
impl fmt::Display for WrittenProjectionTerm {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.name.as_str())?;
        if !self.linked.is_empty() {
            formatter.write_char('{')?;
            write_terms(formatter, &self.linked)?;
            formatter.write_char('}')?;
        }
        Ok(())
    }
}

/// The raw term an expanded projection term is written back as: a keyword
/// keeps its clause 4.21 spelling and an attribute is named by its IRI.
impl TryFrom<&ExpandedProjectionTerm> for WrittenProjectionTerm {
    type Error = QueryError;

    fn try_from(term: &ExpandedProjectionTerm) -> Result<WrittenProjectionTerm, QueryError> {
        let (name, linked) = match term {
            ExpandedProjectionTerm::Id => (keyword::ID_ALIAS, &[][..]),
            ExpandedProjectionTerm::Type => (keyword::TYPE_ALIAS, &[][..]),
            ExpandedProjectionTerm::Scope => (compact_properties::SCOPE, &[][..]),
            ExpandedProjectionTerm::Attribute { iri, linked } => (iri.as_str(), linked.as_slice()),
        };
        Ok(WrittenProjectionTerm {
            name: name.parse().map_err(QueryError::AttributeNameInvalid)?,
            linked: linked.iter().map(WrittenProjectionTerm::try_from).collect::<Result<_, _>>()?,
        })
    }
}

/// Write `terms` as clause 4.21 text, separated by `,`.
pub(super) fn write_terms(formatter: &mut fmt::Formatter<'_>, terms: &[WrittenProjectionTerm]) -> fmt::Result {
    for (index, term) in terms.iter().enumerate() {
        if index > 0 {
            formatter.write_char(',')?;
        }
        fmt::Display::fmt(term, formatter)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::projection::{
        expanded_projection_term::ExpandedProjectionTerm,
        written_projection_list::WrittenProjectionList,
        written_projection_term::WrittenProjectionTerm,
    };
    use iri_rs::IriBuf;

    /// Clause 5.7.2.4 measures a projection against `joinLevel` by how many Relationship hops its nesting crosses.
    #[test]
    fn linked_depth_counts_the_relationship_hops_the_nesting_crosses() {
        for (raw, expected) in [("temperature", 0), ("a{b}", 1), ("a{b{c}}", 2), ("a{b{c{d}}}", 3)] {
            let terms = Vec::from(raw.parse::<WrittenProjectionList>().expect("valid projection list"));
            pretty_assertions::assert_eq!(terms[0].linked_depth(), expected, "depth of {raw}");
        }
    }

    /// A group's depth is the depth of its deepest member, not of its first:
    /// `a{b,c{d}}` reaches two hops through `c` even though `b` reaches one.
    #[test]
    fn linked_depth_of_a_group_is_the_depth_of_its_deepest_member() {
        let terms = Vec::from("a{b,c{d}}".parse::<WrittenProjectionList>().expect("valid projection list"));
        pretty_assertions::assert_eq!(terms[0].linked_depth(), 2);
    }

    /// An expanded term is written back as the raw tree it came from, the
    /// keywords spelt as clause 4.21 spells them.
    #[test]
    fn an_expanded_term_converts_back_to_its_raw_tree() {
        let term = ExpandedProjectionTerm::Attribute {
            iri: IriBuf::new("https://example.com/owner".to_owned()).expect("valid IRI"),
            linked: vec![ExpandedProjectionTerm::Id],
        };
        pretty_assertions::assert_eq!(
            WrittenProjectionTerm::try_from(&term).expect("converts"),
            WrittenProjectionTerm {
                name: "https://example.com/owner".parse().expect("valid attribute name"),
                linked: vec![WrittenProjectionTerm {
                    name: "id".parse().expect("valid attribute name"),
                    linked: Vec::new()
                }]
            }
        );
    }
}
