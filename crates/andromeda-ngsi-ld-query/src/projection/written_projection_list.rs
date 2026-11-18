//! A clause 4.21 projection list (`pick`, `omit`) as the client wrote it:
//!
//! ```text
//! ProjectionTerm = AttrName *1(LinkedEntityTerm) *(orOp ProjectionTerm)
//! LinkedEntityTerm = %x7B ProjectionTerm %x7D
//! orOp = %x7C / %x2C
//! ```
//!
//! `pick=observation{temperature,humidity}` selects the `observation` relationship's own representation together with the `temperature` and `humidity` attributes of the entity it links to. Braces nest to any depth, following the linked-entity chain across as many relationships as the client names.

use crate::projection::{
    projection_error::ProjectionError,
    written_projection_term::{WrittenProjectionTerm, write_terms},
};
use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
use derive_more::{Deref, From, Into};
use serde::{Deserialize, Serialize};
use std::{fmt, str::FromStr};

/// One clause 4.21 projection-language string: a comma/pipe-separated sequence of [`WrittenProjectionTerm`], each optionally followed by a `{...}` group of linked-entity sub-terms. Parsed from and displayed as clause 4.21 text (`|` displays as `,`); serialized as the term tree, for the reason [`WrittenProjectionTerm`] states.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, Deref, From, Into)]
#[serde(transparent)]
pub struct WrittenProjectionList(Vec<WrittenProjectionTerm>);

/// Parse a clause 4.21 projection list.
///
/// # Errors
///
/// Returns [`ProjectionError::EmptyTerm`] where a name was required but
/// none was written, [`ProjectionError::InvalidAttributeName`] for a name outside
/// the attribute-name grammar, and [`ProjectionError::UnbalancedBrace`]
/// when a brace opens without a matching close, closes without a matching
/// open, or is followed by content outside the grammar.
impl FromStr for WrittenProjectionList {
    type Err = ProjectionError;

    fn from_str(raw: &str) -> Result<WrittenProjectionList, ProjectionError> {
        let mut parser = Parser::new(raw);
        let terms = parser.parse_term_list()?;
        match parser.peek() {
            None => Ok(WrittenProjectionList(terms)),
            Some(_) => Err(ProjectionError::UnbalancedBrace { position: parser.pos }),
        }
    }
}

// `derive_more::Display`'s format-string mode cannot join the terms of a `Vec` with `,`.
impl fmt::Display for WrittenProjectionList {
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
    fn parse_term_list(&mut self) -> Result<Vec<WrittenProjectionTerm>, ProjectionError> {
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
    fn parse_term(&mut self) -> Result<WrittenProjectionTerm, ProjectionError> {
        let start = self.pos;
        while let Some(c) = self.peek() {
            if matches!(c, '{' | '}' | ',' | '|') {
                break;
            }
            self.advance();
        }
        let name = self.input[start..self.pos].trim_matches(|c: char| c.is_ascii_whitespace());
        if name.is_empty() {
            return Err(ProjectionError::EmptyTerm { position: start });
        }
        let name = name.parse::<AttributeName>().map_err(ProjectionError::InvalidAttributeName)?;
        let linked = if self.peek() == Some('{') {
            self.parse_linked_entity_term()?
        } else {
            Vec::new()
        };
        Ok(WrittenProjectionTerm { name, linked })
    }

    /// Parses `%x7B ProjectionTerm %x7D` once the next character is known to be `{`. An unclosed brace is reported at the `{` that opened it, the character actually missing its counterpart, rather than at the end of input, which names nothing a client wrote.
    fn parse_linked_entity_term(&mut self) -> Result<Vec<WrittenProjectionTerm>, ProjectionError> {
        let brace_position = self.pos;
        self.advance();
        match self.peek() {
            None => return Err(ProjectionError::UnbalancedBrace { position: brace_position }),
            Some('}') => return Err(ProjectionError::EmptyTerm { position: self.pos }),
            Some(_) => {}
        }
        let children = self.parse_term_list()?;
        match self.peek() {
            Some('}') => {
                self.advance();
                Ok(children)
            }
            Some(_) | None => Err(ProjectionError::UnbalancedBrace { position: brace_position }),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::projection::{
        projection_error::ProjectionError,
        written_projection_list::WrittenProjectionList,
        written_projection_term::WrittenProjectionTerm,
    };
    use serde_json::json;

    /// The terms one clause 4.21 string parses to.
    fn parsed(raw: &str) -> Result<Vec<WrittenProjectionTerm>, ProjectionError> {
        raw.parse::<WrittenProjectionList>().map(Vec::from)
    }

    fn leaf(name: &str) -> WrittenProjectionTerm {
        WrittenProjectionTerm {
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
            vec![WrittenProjectionTerm {
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
            vec![WrittenProjectionTerm {
                name: "a".parse().expect("name"),
                linked: vec![WrittenProjectionTerm {
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
                WrittenProjectionTerm {
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
            vec![WrittenProjectionTerm {
                name: "a".parse().expect("name"),
                linked: vec![leaf("id"), leaf("type")]
            }]
        );
    }

    #[test]
    fn an_opening_brace_never_closed_is_rejected_at_the_braces_own_position() {
        let error = parsed("a{").expect_err("must reject");
        assert!(matches!(error, ProjectionError::UnbalancedBrace { position: 1 }), "got {error:?}");
    }

    #[test]
    fn a_closing_brace_with_no_matching_open_is_rejected_at_its_position() {
        let error = parsed("a}b").expect_err("must reject");
        assert!(matches!(error, ProjectionError::UnbalancedBrace { position: 1 }), "got {error:?}");
    }

    #[test]
    fn an_empty_brace_group_is_rejected_as_an_empty_term() {
        let error = parsed("a{}").expect_err("must reject");
        assert!(matches!(error, ProjectionError::EmptyTerm { .. }), "got {error:?}");
    }

    #[test]
    fn a_leading_comma_is_rejected_as_an_empty_term() {
        let error = parsed(",a").expect_err("must reject");
        assert!(matches!(error, ProjectionError::EmptyTerm { .. }), "got {error:?}");
    }

    #[test]
    fn a_doubled_comma_is_rejected_as_an_empty_term() {
        let error = parsed("a,,b").expect_err("must reject");
        assert!(matches!(error, ProjectionError::EmptyTerm { .. }), "got {error:?}");
    }

    #[test]
    fn an_empty_input_is_rejected_as_an_empty_term() {
        let error = parsed("").expect_err("must reject");
        assert!(matches!(error, ProjectionError::EmptyTerm { .. }), "got {error:?}");
    }

    #[test]
    fn trailing_content_after_a_closed_brace_group_is_rejected_at_the_garbage_position() {
        let error = parsed("a{b,c}d").expect_err("must reject");
        assert!(matches!(error, ProjectionError::UnbalancedBrace { position: 6 }), "got {error:?}");
    }

    /// A stored projection selector is re-serialised whenever it is read back, so the grammar must round-trip through its own `Display` rendering.
    #[test]
    fn a_projection_list_round_trips_through_text() {
        for raw in ["owner{name},speed", "observation{temperature,humidity},name", "a{b{c,d}},e"] {
            let list: WrittenProjectionList = raw.parse().expect("valid projection list");
            pretty_assertions::assert_eq!(list.to_string(), raw);
        }
    }

    #[test]
    fn a_pipe_separated_list_displays_with_commas() {
        let list: WrittenProjectionList = "a|b".parse().expect("valid projection list");
        pretty_assertions::assert_eq!(list.to_string(), "a,b");
    }

    #[test]
    fn an_unbalanced_list_is_refused() {
        let error = "a{".parse::<WrittenProjectionList>().expect_err("must reject");
        assert!(matches!(error, ProjectionError::UnbalancedBrace { .. }), "got {error:?}");
    }

    /// The stored form is the term tree, not clause 4.21 text.
    #[test]
    fn a_projection_list_serializes_as_a_term_tree() {
        let list: WrittenProjectionList = "owner{name}".parse().expect("valid projection list");
        pretty_assertions::assert_eq!(
            serde_json::to_value(&list).expect("serializes"),
            json!([{"name": "owner", "linked": [{"name": "name", "linked": []}]}])
        );
    }

    #[test]
    fn a_projection_list_deserializes_from_a_term_tree() {
        let list: WrittenProjectionList =
            serde_json::from_value(json!([{"name": "owner", "linked": [{"name": "name", "linked": []}]}, {"name": "speed", "linked": []}]))
                .expect("deserializes");
        pretty_assertions::assert_eq!(list, "owner{name},speed".parse::<WrittenProjectionList>().expect("valid projection list"));
    }

    /// An expanded IRI may hold the `,` the grammar separates terms with; the
    /// tree keeps it one name.
    #[test]
    fn an_expanded_name_holding_a_comma_survives_storage() {
        let list = WrittenProjectionList::from(vec![WrittenProjectionTerm {
            name: "https://example.com/a,b".parse().expect("valid attribute name"),
            linked: vec![leaf("https://example.com/c")],
        }]);
        let stored = serde_json::to_value(&list).expect("serializes");
        pretty_assertions::assert_eq!(serde_json::from_value::<WrittenProjectionList>(stored).expect("deserializes"), list);
    }
}
