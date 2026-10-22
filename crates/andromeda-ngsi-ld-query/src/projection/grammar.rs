//! Raw projection list grammar for the `pick` and `omit` linked-entity syntax (ETSI GS CIM 009 v1.9.1 clause 4.21):
//!
//! ```text
//! ProjectionTerm = AttrName *1(LinkedEntityTerm) *(orOp ProjectionTerm)
//! LinkedEntityTerm = %x7B ProjectionTerm %x7D
//! orOp = %x7C / %x2C
//! ```
//!
//! `pick=observation{temperature,humidity}` selects the `observation` relationship's own representation together with the `temperature` and `humidity` attributes of the entity it links to. Braces nest to any depth, following the linked-entity chain across as many relationships as the client names.

use crate::{error::QueryError, projection::term::ProjectionTerm};
use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
use andromeda_ngsi_ld_vocabulary::{compact::properties as compact_properties, keyword};
use derive_more::{Deref, From, Into};
use serde::{Deserialize, Serialize};
use std::{
    fmt::{self, Write},
    str::FromStr,
};

/// One term of a clause 4.21 raw (pre-expansion) projection list: one attribute and, when it constrains a linked entity's own projection, the `{...}` sub-terms selected on the other side of the relationship it names.
///
/// Serialized as the tree `{"name": ..., "linked": [...]}` rather than as clause 4.21 text: a stored subscription keeps its terms expanded (clause 5.5.7), and an expanded IRI may hold the `,` and `|` the grammar separates terms with, which the clause 4.9 `AttrName` a client writes never does.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RawProjectionTerm {
    /// The compact or expanded attribute name this term selects.
    pub name: AttributeName,
    /// The linked-entity sub-projection carried inside `{...}`, empty when the term selects no linked entity.
    pub linked: Vec<RawProjectionTerm>,
}

impl RawProjectionTerm {
    /// How many Relationship hops this term's clause 4.21 nesting reaches: `0` for a term selecting only members of its own entity, `1` for `rel{a}`, `2` for `rel{inner{a}}`. It is what a `joinLevel` has to cover for the nested terms to name anything the traversal retrieves (clause 5.7.2.4).
    pub fn linked_depth(&self) -> u32 {
        match self.linked.iter().map(RawProjectionTerm::linked_depth).max() {
            Some(deepest) => deepest.saturating_add(1),
            None => 0,
        }
    }
}

// Rendering walks the recursive `linked` structure and separates siblings with `,`, a shape `derive_more::Display`'s format-string mode cannot express.
impl fmt::Display for RawProjectionTerm {
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
impl TryFrom<&ProjectionTerm> for RawProjectionTerm {
    type Error = QueryError;

    fn try_from(term: &ProjectionTerm) -> Result<RawProjectionTerm, QueryError> {
        let (name, linked) = match term {
            ProjectionTerm::Id => (keyword::ID_ALIAS, &[][..]),
            ProjectionTerm::Type => (keyword::TYPE_ALIAS, &[][..]),
            ProjectionTerm::Scope => (compact_properties::SCOPE, &[][..]),
            ProjectionTerm::Attribute { iri, linked } => (iri.as_str(), linked.as_slice()),
        };
        Ok(RawProjectionTerm {
            name: name.parse().map_err(QueryError::AttributeNameInvalid)?,
            linked: linked.iter().map(RawProjectionTerm::try_from).collect::<Result<_, _>>()?,
        })
    }
}

/// Write `terms` as clause 4.21 text, separated by `,`.
fn write_terms(formatter: &mut fmt::Formatter<'_>, terms: &[RawProjectionTerm]) -> fmt::Result {
    for (index, term) in terms.iter().enumerate() {
        if index > 0 {
            formatter.write_char(',')?;
        }
        fmt::Display::fmt(term, formatter)?;
    }
    Ok(())
}

/// One clause 4.21 projection-language string: a comma/pipe-separated sequence of [`RawProjectionTerm`], each optionally followed by a `{...}` group of linked-entity sub-terms. Parsed from and displayed as clause 4.21 text (`|` displays as `,`); serialized as the term tree, for the reason [`RawProjectionTerm`] states.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, Deref, From, Into)]
#[serde(transparent)]
pub struct ProjectionList(Vec<RawProjectionTerm>);

/// Parse a clause 4.21 projection list.
///
/// # Errors
///
/// Returns [`QueryError::ProjectionEmptyTerm`] where a name was required but
/// none was written, [`QueryError::AttributeNameInvalid`] for a name outside
/// the attribute-name grammar, and [`QueryError::ProjectionUnbalancedBrace`]
/// when a brace opens without a matching close, closes without a matching
/// open, or is followed by content outside the grammar.
impl FromStr for ProjectionList {
    type Err = QueryError;

    fn from_str(raw: &str) -> Result<ProjectionList, QueryError> {
        let mut parser = Parser::new(raw);
        let terms = parser.parse_term_list()?;
        match parser.peek() {
            None => Ok(ProjectionList(terms)),
            Some(_) => Err(QueryError::ProjectionUnbalancedBrace { position: parser.pos }),
        }
    }
}

// `derive_more::Display`'s format-string mode cannot join the terms of a `Vec` with `,`.
impl fmt::Display for ProjectionList {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write_terms(formatter, &self.0)
    }
}

/// Cursor over a clause 4.21 projection list.
struct Parser<'a> {
    /// The list being consumed.
    input: &'a str,
    /// Byte offset of the next character to consume.
    pos: usize,
}

impl<'a> Parser<'a> {
    /// Start at the beginning of a projection list.
    const fn new(input: &'a str) -> Parser<'a> {
        Parser { input, pos: 0 }
    }

    /// The next character without consuming it.
    fn peek(&self) -> Option<char> {
        self.input[self.pos..].chars().next()
    }

    /// Consume the next character.
    fn advance(&mut self) {
        if let Some(c) = self.peek() {
            self.pos += c.len_utf8();
        }
    }

    /// Parses `ProjectionTerm *(orOp ProjectionTerm)`, stopping at the first character that is neither a separator nor the start of another term.
    fn parse_term_list(&mut self) -> Result<Vec<RawProjectionTerm>, QueryError> {
        let mut terms = Vec::new();
        loop {
            terms.push(self.parse_term()?);
            match self.peek() {
                Some('|' | ',') => self.advance(),
                Some(_) | None => break,
            }
        }
        Ok(terms)
    }

    /// Parses one `AttrName *1(LinkedEntityTerm)`.
    fn parse_term(&mut self) -> Result<RawProjectionTerm, QueryError> {
        let start = self.pos;
        while let Some(c) = self.peek() {
            if matches!(c, '{' | '}' | ',' | '|') {
                break;
            }
            self.advance();
        }
        let name = self.input[start..self.pos].trim_matches(|c: char| c.is_ascii_whitespace());
        if name.is_empty() {
            return Err(QueryError::ProjectionEmptyTerm { position: start });
        }
        let name = name.parse::<AttributeName>().map_err(QueryError::AttributeNameInvalid)?;
        let linked = if self.peek() == Some('{') {
            self.parse_linked_entity_term()?
        } else {
            Vec::new()
        };
        Ok(RawProjectionTerm { name, linked })
    }

    /// Parses `%x7B ProjectionTerm %x7D` once the next character is known to be `{`. An unclosed brace is reported at the `{` that opened it, the character actually missing its counterpart, rather than at the end of input, which names nothing a client wrote.
    fn parse_linked_entity_term(&mut self) -> Result<Vec<RawProjectionTerm>, QueryError> {
        let brace_position = self.pos;
        self.advance();
        match self.peek() {
            None => return Err(QueryError::ProjectionUnbalancedBrace { position: brace_position }),
            Some('}') => return Err(QueryError::ProjectionEmptyTerm { position: self.pos }),
            Some(_) => {}
        }
        let children = self.parse_term_list()?;
        match self.peek() {
            Some('}') => {
                self.advance();
                Ok(children)
            }
            Some(_) | None => Err(QueryError::ProjectionUnbalancedBrace { position: brace_position }),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        error::QueryError,
        projection::{
            grammar::{ProjectionList, RawProjectionTerm},
            term::ProjectionTerm,
        },
    };
    use iri_rs::IriBuf;
    use serde_json::json;

    /// The terms one clause 4.21 string parses to.
    fn parsed(raw: &str) -> Result<Vec<RawProjectionTerm>, QueryError> {
        raw.parse::<ProjectionList>().map(Vec::from)
    }

    fn leaf(name: &str) -> RawProjectionTerm {
        RawProjectionTerm {
            name: name.parse().expect("valid attribute name"),
            linked: Vec::new(),
        }
    }

    #[test]
    fn a_single_name_parses_to_one_term_with_no_linked_entity() {
        let terms = parsed("temperature").expect("valid projection list");
        pretty_assertions::assert_eq!(terms, vec![leaf("temperature")]);
    }

    #[test]
    fn a_comma_separated_list_yields_one_term_per_name_with_no_nesting() {
        let terms = parsed("temperature,humidity").expect("valid projection list");
        pretty_assertions::assert_eq!(terms, vec![leaf("temperature"), leaf("humidity")]);
    }

    #[test]
    fn the_pipe_or_operator_separates_terms_the_same_as_a_comma() {
        let terms = parsed("a|b").expect("valid projection list");
        pretty_assertions::assert_eq!(terms, vec![leaf("a"), leaf("b")]);
    }

    #[test]
    fn braces_after_a_name_carry_the_linked_entitys_own_projection() {
        let terms = parsed("observation{temperature,humidity}").expect("valid projection list");
        pretty_assertions::assert_eq!(
            terms,
            vec![RawProjectionTerm {
                name: "observation".parse().expect("name"),
                linked: vec![leaf("temperature"), leaf("humidity")]
            }]
        );
    }

    #[test]
    fn braces_nest_to_at_least_two_levels_deep() {
        let terms = parsed("a{b{c}}").expect("valid projection list");
        pretty_assertions::assert_eq!(
            terms,
            vec![RawProjectionTerm {
                name: "a".parse().expect("name"),
                linked: vec![RawProjectionTerm {
                    name: "b".parse().expect("name"),
                    linked: vec![leaf("c")]
                }]
            }]
        );
    }

    #[test]
    fn a_nested_term_may_be_followed_by_a_plain_sibling_term() {
        let terms = parsed("a{b},c").expect("valid projection list");
        pretty_assertions::assert_eq!(
            terms,
            vec![
                RawProjectionTerm {
                    name: "a".parse().expect("name"),
                    linked: vec![leaf("b")]
                },
                leaf("c")
            ]
        );
    }

    #[test]
    fn ngsi_ld_keywords_are_legal_attribute_names_inside_braces() {
        let terms = parsed("a{id,type}").expect("valid projection list");
        pretty_assertions::assert_eq!(
            terms,
            vec![RawProjectionTerm {
                name: "a".parse().expect("name"),
                linked: vec![leaf("id"), leaf("type")]
            }]
        );
    }

    #[test]
    fn an_opening_brace_never_closed_is_rejected_at_the_braces_own_position() {
        let error = parsed("a{").expect_err("must reject");
        assert!(matches!(error, QueryError::ProjectionUnbalancedBrace { position: 1 }), "got {error:?}");
    }

    #[test]
    fn a_closing_brace_with_no_matching_open_is_rejected_at_its_position() {
        let error = parsed("a}b").expect_err("must reject");
        assert!(matches!(error, QueryError::ProjectionUnbalancedBrace { position: 1 }), "got {error:?}");
    }

    #[test]
    fn an_empty_brace_group_is_rejected_as_an_empty_term() {
        let error = parsed("a{}").expect_err("must reject");
        assert!(matches!(error, QueryError::ProjectionEmptyTerm { .. }), "got {error:?}");
    }

    #[test]
    fn a_leading_comma_is_rejected_as_an_empty_term() {
        let error = parsed(",a").expect_err("must reject");
        assert!(matches!(error, QueryError::ProjectionEmptyTerm { .. }), "got {error:?}");
    }

    #[test]
    fn a_doubled_comma_is_rejected_as_an_empty_term() {
        let error = parsed("a,,b").expect_err("must reject");
        assert!(matches!(error, QueryError::ProjectionEmptyTerm { .. }), "got {error:?}");
    }

    #[test]
    fn an_empty_input_is_rejected_as_an_empty_term() {
        let error = parsed("").expect_err("must reject");
        assert!(matches!(error, QueryError::ProjectionEmptyTerm { .. }), "got {error:?}");
    }

    #[test]
    fn trailing_content_after_a_closed_brace_group_is_rejected_at_the_garbage_position() {
        let error = parsed("a{b,c}d").expect_err("must reject");
        assert!(matches!(error, QueryError::ProjectionUnbalancedBrace { position: 6 }), "got {error:?}");
    }

    /// Clause 5.7.2.4 measures a projection against `joinLevel` by how many Relationship hops its nesting crosses.
    #[test]
    fn linked_depth_counts_the_relationship_hops_the_nesting_crosses() {
        for (raw, expected) in [("temperature", 0), ("a{b}", 1), ("a{b{c}}", 2), ("a{b{c{d}}}", 3)] {
            let terms = parsed(raw).expect("valid projection list");
            pretty_assertions::assert_eq!(terms[0].linked_depth(), expected, "depth of {raw}");
        }
    }

    /// A group's depth is the depth of its deepest member, not of its first:
    /// `a{b,c{d}}` reaches two hops through `c` even though `b` reaches one.
    #[test]
    fn linked_depth_of_a_group_is_the_depth_of_its_deepest_member() {
        let terms = parsed("a{b,c{d}}").expect("valid projection list");
        pretty_assertions::assert_eq!(terms[0].linked_depth(), 2);
    }

    /// A stored projection selector is re-serialised whenever it is read back, so the grammar must round-trip through its own `Display` rendering.
    #[test]
    fn a_projection_list_round_trips_through_text() {
        for raw in ["owner{name},speed", "observation{temperature,humidity},name", "a{b{c,d}},e"] {
            let list: ProjectionList = raw.parse().expect("valid projection list");
            pretty_assertions::assert_eq!(list.to_string(), raw);
        }
    }

    #[test]
    fn a_pipe_separated_list_displays_with_commas() {
        let list: ProjectionList = "a|b".parse().expect("valid projection list");
        pretty_assertions::assert_eq!(list.to_string(), "a,b");
    }

    #[test]
    fn an_unbalanced_list_is_refused() {
        let error = "a{".parse::<ProjectionList>().expect_err("must reject");
        assert!(matches!(error, QueryError::ProjectionUnbalancedBrace { .. }), "got {error:?}");
    }

    /// The stored form is the term tree, not clause 4.21 text.
    #[test]
    fn a_projection_list_serializes_as_a_term_tree() {
        let list: ProjectionList = "owner{name}".parse().expect("valid projection list");
        pretty_assertions::assert_eq!(
            serde_json::to_value(&list).expect("serializes"),
            json!([{"name": "owner", "linked": [{"name": "name", "linked": []}]}])
        );
    }

    #[test]
    fn a_projection_list_deserializes_from_a_term_tree() {
        let list: ProjectionList =
            serde_json::from_value(json!([{"name": "owner", "linked": [{"name": "name", "linked": []}]}, {"name": "speed", "linked": []}]))
                .expect("deserializes");
        pretty_assertions::assert_eq!(list, "owner{name},speed".parse::<ProjectionList>().expect("valid projection list"));
    }

    /// An expanded IRI may hold the `,` the grammar separates terms with; the
    /// tree keeps it one name.
    #[test]
    fn an_expanded_name_holding_a_comma_survives_storage() {
        let list = ProjectionList::from(vec![RawProjectionTerm {
            name: "https://example.com/a,b".parse().expect("valid attribute name"),
            linked: vec![leaf("https://example.com/c")],
        }]);
        let stored = serde_json::to_value(&list).expect("serializes");
        pretty_assertions::assert_eq!(serde_json::from_value::<ProjectionList>(stored).expect("deserializes"), list);
    }

    /// An expanded term is written back as the raw tree it came from, the
    /// keywords spelt as clause 4.21 spells them.
    #[test]
    fn an_expanded_term_converts_back_to_its_raw_tree() {
        let term = ProjectionTerm::Attribute {
            iri: IriBuf::new("https://example.com/owner".to_owned()).expect("valid IRI"),
            linked: vec![ProjectionTerm::Id],
        };
        pretty_assertions::assert_eq!(
            RawProjectionTerm::try_from(&term).expect("converts"),
            RawProjectionTerm {
                name: "https://example.com/owner".parse().expect("valid attribute name"),
                linked: vec![leaf("id")]
            }
        );
    }
}
