//! Raw projection list grammar for the `pick` and `omit` linked-entity syntax
//! (ETSI GS CIM 009 v1.9.1 clause 4.21):
//!
//! ```text
//! ProjectionTerm = AttrName *1(LinkedEntityTerm) *(orOp ProjectionTerm)
//! LinkedEntityTerm = %x7B ProjectionTerm %x7D
//! orOp = %x7C / %x2C
//! ```
//!
//! `pick=observation{temperature,humidity}` selects the `observation`
//! relationship's own representation together with the `temperature` and
//! `humidity` attributes of the entity it links to. Braces nest to any depth,
//! following the linked-entity chain across as many relationships as the
//! client names.

use crate::error::QueryError;
use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
use std::fmt::{self, Write};

/// One term of a clause 4.21 raw (pre-expansion) projection list.
///
/// A term names one attribute and, when it constrains a linked entity's own
/// projection, carries the `{...}` sub-terms selected on the other side of
/// the relationship it names.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct RawProjectionTerm {
    /// The compact or expanded attribute name this term selects.
    pub name: AttributeName,
    /// The linked-entity sub-projection carried inside `{...}`, empty when
    /// this term selects no linked entity.
    pub linked: Vec<RawProjectionTerm>,
}

impl RawProjectionTerm {
    /// How many Relationship hops this term's clause 4.21 nesting reaches.
    ///
    /// A term selecting only members of the entity it is written against is
    /// `0`; `rel{a}` is `1`, `rel{inner{a}}` is `2`, and so on. The depth is
    /// what a `joinLevel` has to cover for the nested terms to name anything
    /// the traversal actually retrieves (clause 5.7.2.4).
    ///
    /// The walk recurses over `linked`, which the clause 4.21 grammar builds
    /// as a finite tree from a finite input, so it terminates at the deepest
    /// brace group the client wrote.
    pub fn linked_depth(&self) -> u32 {
        match self.linked.iter().map(RawProjectionTerm::linked_depth).max() {
            Some(deepest) => deepest.saturating_add(1),
            None => 0,
        }
    }
}

// `RawProjectionTerm` nests through `linked`, and rendering it requires
// walking that recursive structure and separating siblings with `,` — a shape
// `derive_more::Display`'s format-string mode cannot express. So this is a
// hand-written `impl Display` rather than a derive.
impl fmt::Display for RawProjectionTerm {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.name.as_str())?;
        if !self.linked.is_empty() {
            formatter.write_char('{')?;
            for (index, child) in self.linked.iter().enumerate() {
                if index > 0 {
                    formatter.write_char(',')?;
                }
                fmt::Display::fmt(child, formatter)?;
            }
            formatter.write_char('}')?;
        }
        Ok(())
    }
}

/// Parse a clause 4.21 raw projection list: a comma/pipe-separated sequence
/// of [`RawProjectionTerm`], each optionally followed by a `{...}` group of
/// linked-entity sub-terms.
///
/// # Errors
///
/// Returns [`QueryError::ProjectionEmptyTerm`] where a name was required but
/// none was written, and [`QueryError::ProjectionUnbalancedBrace`] when a brace
/// opens without a matching close, closes without a matching open, or is
/// followed by content outside the grammar.
pub fn parse_projection_list(raw: &str) -> Result<Vec<RawProjectionTerm>, QueryError> {
    let mut parser = Parser::new(raw);
    let terms = parser.parse_term_list()?;
    match parser.peek() {
        None => Ok(terms),
        Some(_) => Err(QueryError::ProjectionUnbalancedBrace { position: parser.pos }),
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

    /// Parses `ProjectionTerm *(orOp ProjectionTerm)`: one or more terms
    /// separated by `,` or `|`, stopping at the first character that is
    /// neither a separator nor the start of another term.
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

    /// Parses `%x7B ProjectionTerm %x7D`, having already confirmed the next
    /// character is `{`.
    ///
    /// An unclosed brace is reported at the position of the `{` that opened
    /// it — the character actually missing its counterpart — rather than at
    /// the end of input, which names nothing a client wrote.
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
        projection::grammar::{RawProjectionTerm, parse_projection_list},
    };

    fn leaf(name: &str) -> RawProjectionTerm {
        RawProjectionTerm {
            name: name.parse().expect("valid attribute name"),
            linked: Vec::new(),
        }
    }

    #[test]
    fn a_single_name_parses_to_one_term_with_no_linked_entity() {
        let terms = parse_projection_list("temperature").expect("valid projection list");
        pretty_assertions::assert_eq!(terms, vec![leaf("temperature")]);
    }

    #[test]
    fn a_comma_separated_list_yields_one_term_per_name_with_no_nesting() {
        let terms = parse_projection_list("temperature,humidity").expect("valid projection list");
        pretty_assertions::assert_eq!(terms, vec![leaf("temperature"), leaf("humidity")]);
    }

    #[test]
    fn the_pipe_or_operator_separates_terms_the_same_as_a_comma() {
        let terms = parse_projection_list("a|b").expect("valid projection list");
        pretty_assertions::assert_eq!(terms, vec![leaf("a"), leaf("b")]);
    }

    #[test]
    fn braces_after_a_name_carry_the_linked_entitys_own_projection() {
        let terms = parse_projection_list("observation{temperature,humidity}").expect("valid projection list");
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
        let terms = parse_projection_list("a{b{c}}").expect("valid projection list");
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
        let terms = parse_projection_list("a{b},c").expect("valid projection list");
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
        let terms = parse_projection_list("a{id,type}").expect("valid projection list");
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
        let error = parse_projection_list("a{").expect_err("must reject");
        assert!(matches!(error, QueryError::ProjectionUnbalancedBrace { position: 1 }), "got {error:?}");
    }

    #[test]
    fn a_closing_brace_with_no_matching_open_is_rejected_at_its_position() {
        let error = parse_projection_list("a}b").expect_err("must reject");
        assert!(matches!(error, QueryError::ProjectionUnbalancedBrace { position: 1 }), "got {error:?}");
    }

    #[test]
    fn an_empty_brace_group_is_rejected_as_an_empty_term() {
        let error = parse_projection_list("a{}").expect_err("must reject");
        assert!(matches!(error, QueryError::ProjectionEmptyTerm { .. }), "got {error:?}");
    }

    #[test]
    fn a_leading_comma_is_rejected_as_an_empty_term() {
        let error = parse_projection_list(",a").expect_err("must reject");
        assert!(matches!(error, QueryError::ProjectionEmptyTerm { .. }), "got {error:?}");
    }

    #[test]
    fn a_doubled_comma_is_rejected_as_an_empty_term() {
        let error = parse_projection_list("a,,b").expect_err("must reject");
        assert!(matches!(error, QueryError::ProjectionEmptyTerm { .. }), "got {error:?}");
    }

    #[test]
    fn an_empty_input_is_rejected_as_an_empty_term() {
        let error = parse_projection_list("").expect_err("must reject");
        assert!(matches!(error, QueryError::ProjectionEmptyTerm { .. }), "got {error:?}");
    }

    #[test]
    fn trailing_content_after_a_closed_brace_group_is_rejected_at_the_garbage_position() {
        let error = parse_projection_list("a{b,c}d").expect_err("must reject");
        assert!(matches!(error, QueryError::ProjectionUnbalancedBrace { position: 6 }), "got {error:?}");
    }

    /// Clause 5.7.2.4 measures a projection against `joinLevel` by how many
    /// Relationship hops its nesting crosses: a flat term crosses none, and
    /// each brace group crosses one more.
    #[test]
    fn linked_depth_counts_the_relationship_hops_the_nesting_crosses() {
        for (raw, expected) in [("temperature", 0), ("a{b}", 1), ("a{b{c}}", 2), ("a{b{c{d}}}", 3)] {
            let terms = parse_projection_list(raw).expect("valid projection list");
            pretty_assertions::assert_eq!(terms[0].linked_depth(), expected, "depth of {raw}");
        }
    }

    /// A group's depth is the depth of its deepest member, not of its first:
    /// `a{b,c{d}}` reaches two hops through `c` even though `b` reaches one.
    #[test]
    fn linked_depth_of_a_group_is_the_depth_of_its_deepest_member() {
        let terms = parse_projection_list("a{b,c{d}}").expect("valid projection list");
        pretty_assertions::assert_eq!(terms[0].linked_depth(), 2);
    }

    /// A stored projection selector is re-serialised whenever it is read back
    /// (clause 4.21 `pick`/`omit`), so the grammar must round-trip through
    /// its own `Display` rendering.
    #[test]
    fn a_projection_list_round_trips_through_its_clause_4_21_text() {
        let parsed = parse_projection_list("observation{temperature,humidity},name").expect("valid projection list");
        let rendered = parsed.iter().map(RawProjectionTerm::to_string).collect::<Vec<_>>().join(",");
        let reparsed = parse_projection_list(&rendered).expect("rendered text re-parses");
        pretty_assertions::assert_eq!(parsed, reparsed);
    }
}
