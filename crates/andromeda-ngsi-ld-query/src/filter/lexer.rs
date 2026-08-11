//! Tokeniser for the NGSI-LD Query Language (clause 4.9).

use crate::error::QueryError;

/// A single lexical token of a `q` expression.
#[derive(Debug, Clone, PartialEq)]
pub enum Token {
    /// A bare identifier: attribute name, IRI, or an unquoted temporal literal.
    Ident(String),
    /// A double-quoted string literal, taken verbatim after backslash-escape
    /// processing. The contents are not percent-decoded: percent-encoding is an
    /// HTTP-URI transport concern (clause 4.9), decoded once by the HTTP binding
    /// before the filter reaches this tokeniser, and a `q` carried in a JSON
    /// body (a subscription) is not percent-encoded at all.
    QuotedStr(String),
    /// A numeric literal, kept as text until the parser types it.
    NumberLit(String),
    /// The `true` keyword.
    True,
    /// The `false` keyword.
    False,
    /// `==`
    Eq,
    /// `!=`
    Neq,
    /// `>`
    Gt,
    /// `>=`
    Ge,
    /// `<`
    Lt,
    /// `<=`
    Le,
    /// `~=`
    Match,
    /// `!~=`
    NotMatch,
    /// `..`, the range separator.
    Dots,
    /// `;`, the conjunction operator.
    Semi,
    /// `|`, the disjunction operator.
    Pipe,
    /// `(`
    LParen,
    /// `)`
    RParen,
    /// `[`, opening a compound value path.
    LBracket,
    /// `]`, closing a compound value path.
    RBracket,
    /// `{`, opening a linked-entity traversal.
    LBrace,
    /// `}`, closing a linked-entity traversal.
    RBrace,
    /// `*`, the language-suppression marker inside `[*]`.
    Star,
    /// `.`, the sub-attribute separator.
    Dot,
    /// `,`, the value-list separator.
    Comma,
}

impl Token {
    /// How the clause 4.9 grammar spells this token.
    ///
    /// A parse failure quotes this rather than the Rust variant name, so the
    /// `BadRequestData` detail a client receives names the character the grammar
    /// expected. The three token kinds that carry text describe their kind
    /// instead, because their spelling is whatever the client wrote.
    pub const fn spelling(&self) -> &'static str {
        match self {
            Token::Ident(_) => "identifier",
            Token::QuotedStr(_) => "quoted string",
            Token::NumberLit(_) => "number",
            Token::True => "true",
            Token::False => "false",
            Token::Eq => "==",
            Token::Neq => "!=",
            Token::Gt => ">",
            Token::Ge => ">=",
            Token::Lt => "<",
            Token::Le => "<=",
            Token::Match => "~=",
            Token::NotMatch => "!~=",
            Token::Dots => "..",
            Token::Semi => ";",
            Token::Pipe => "|",
            Token::LParen => "(",
            Token::RParen => ")",
            Token::LBracket => "[",
            Token::RBracket => "]",
            Token::LBrace => "{",
            Token::RBrace => "}",
            Token::Star => "*",
            Token::Dot => ".",
            Token::Comma => ",",
        }
    }
}

/// Split a `q` expression into tokens.
///
/// Returns a typed [`QueryError`] variant when a character cannot start any
/// token or a quoted literal is left open.
pub fn tokenize(input: &str) -> Result<Vec<Token>, QueryError> {
    let mut tokens = Vec::new();
    let chars: Vec<char> = input.chars().collect();
    let mut i = 0;

    while i < chars.len() {
        match chars[i] {
            ' ' | '\t' | '\n' | '\r' => i += 1,
            ';' => {
                tokens.push(Token::Semi);
                i += 1;
            }
            '|' => {
                tokens.push(Token::Pipe);
                i += 1;
            }
            '(' => {
                tokens.push(Token::LParen);
                i += 1;
            }
            ')' => {
                tokens.push(Token::RParen);
                i += 1;
            }
            '[' => {
                tokens.push(Token::LBracket);
                i += 1;
            }
            ']' => {
                tokens.push(Token::RBracket);
                i += 1;
            }
            '{' => {
                tokens.push(Token::LBrace);
                i += 1;
            }
            '}' => {
                tokens.push(Token::RBrace);
                i += 1;
            }
            '*' => {
                tokens.push(Token::Star);
                i += 1;
            }
            ',' => {
                tokens.push(Token::Comma);
                i += 1;
            }
            '=' if i + 1 < chars.len() && chars[i + 1] == '=' => {
                tokens.push(Token::Eq);
                i += 2;
            }
            '!' if i + 2 < chars.len() && chars[i + 1] == '~' && chars[i + 2] == '=' => {
                tokens.push(Token::NotMatch);
                i += 3;
            }
            '!' if i + 1 < chars.len() && chars[i + 1] == '=' => {
                tokens.push(Token::Neq);
                i += 2;
            }
            '~' if i + 1 < chars.len() && chars[i + 1] == '=' => {
                tokens.push(Token::Match);
                i += 2;
            }
            '>' if i + 1 < chars.len() && chars[i + 1] == '=' => {
                tokens.push(Token::Ge);
                i += 2;
            }
            '>' => {
                tokens.push(Token::Gt);
                i += 1;
            }
            '<' if i + 1 < chars.len() && chars[i + 1] == '=' => {
                tokens.push(Token::Le);
                i += 2;
            }
            '<' => {
                tokens.push(Token::Lt);
                i += 1;
            }
            '.' if i + 1 < chars.len() && chars[i + 1] == '.' => {
                tokens.push(Token::Dots);
                i += 2;
            }
            '.' => {
                tokens.push(Token::Dot);
                i += 1;
            }
            '"' => {
                i += 1;
                let mut s = String::new();
                while i < chars.len() && chars[i] != '"' {
                    if chars[i] == '\\' && i + 1 < chars.len() {
                        s.push(chars[i + 1]);
                        i += 2;
                    } else {
                        s.push(chars[i]);
                        i += 1;
                    }
                }
                if i >= chars.len() {
                    return Err(QueryError::QUnterminatedString);
                }
                i += 1;
                tokens.push(Token::QuotedStr(s));
            }
            c if c == '-' && i + 1 < chars.len() && chars[i + 1].is_ascii_digit() => {
                let start = i;
                i += 1;
                while i < chars.len() && chars[i].is_ascii_digit() {
                    i += 1;
                }
                if i < chars.len() && chars[i] == '.' && !(i + 1 < chars.len() && chars[i + 1] == '.') && i + 1 < chars.len() && chars[i + 1].is_ascii_digit() {
                    i += 1;
                    while i < chars.len() && chars[i].is_ascii_digit() {
                        i += 1;
                    }
                }
                if i < chars.len() && (chars[i] == 'e' || chars[i] == 'E') {
                    i += 1;
                    if i < chars.len() && (chars[i] == '+' || chars[i] == '-') {
                        i += 1;
                    }
                    while i < chars.len() && chars[i].is_ascii_digit() {
                        i += 1;
                    }
                }
                let num_str: String = chars[start..i].iter().collect();
                tokens.push(Token::NumberLit(num_str));
            }
            c if c.is_ascii_digit() => {
                let start = i;
                while i < chars.len() && chars[i].is_ascii_digit() {
                    i += 1;
                }
                // Check if this looks like a datetime/date (digits followed by '-')
                // e.g. 2017-12-24T12:00:00Z
                if i < chars.len() && chars[i] == '-' && i + 1 < chars.len() && chars[i + 1].is_ascii_digit() {
                    // Looks like a date-like token, consume as identifier
                    while i < chars.len()
                        && (chars[i].is_alphanumeric()
                            || chars[i] == '-'
                            || chars[i] == ':'
                            || chars[i] == '.'
                            || chars[i] == '+'
                            || chars[i] == 'T'
                            || chars[i] == 'Z')
                    {
                        if chars[i] == '.' && i + 1 < chars.len() && chars[i + 1] == '.' {
                            break;
                        }
                        i += 1;
                    }
                    let word: String = chars[start..i].iter().collect();
                    tokens.push(Token::Ident(word));
                } else {
                    if i < chars.len()
                        && chars[i] == '.'
                        && !(i + 1 < chars.len() && chars[i + 1] == '.')
                        && i + 1 < chars.len()
                        && chars[i + 1].is_ascii_digit()
                    {
                        i += 1;
                        while i < chars.len() && chars[i].is_ascii_digit() {
                            i += 1;
                        }
                    }
                    if i < chars.len() && (chars[i] == 'e' || chars[i] == 'E') {
                        i += 1;
                        if i < chars.len() && (chars[i] == '+' || chars[i] == '-') {
                            i += 1;
                        }
                        while i < chars.len() && chars[i].is_ascii_digit() {
                            i += 1;
                        }
                    }
                    let num_str: String = chars[start..i].iter().collect();
                    tokens.push(Token::NumberLit(num_str));
                }
            }
            c if c.is_alphanumeric() || c == '_' || c == ':' || c == '/' => {
                let start = i;
                while i < chars.len()
                    && (chars[i].is_alphanumeric() || chars[i] == '_' || chars[i] == ':' || chars[i] == '/' || chars[i] == '-' || chars[i] == '#')
                {
                    if chars[i] == '.' && i + 1 < chars.len() && chars[i + 1] == '.' {
                        break;
                    }
                    if chars[i] == '.' {
                        break;
                    }
                    i += 1;
                }
                let word: String = chars[start..i].iter().collect();
                match word.as_str() {
                    "true" => tokens.push(Token::True),
                    "false" => tokens.push(Token::False),
                    // `&str` is unbounded, so a catch-all is the only way to
                    // close this match; every other word is an identifier.
                    _ => tokens.push(Token::Ident(word)),
                }
            }
            other => {
                return Err(QueryError::QUnexpectedChar { ch: other, position: i });
            }
        }
    }

    Ok(tokens)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lex(input: &str) -> Vec<Token> {
        tokenize(input).expect("tokenises")
    }

    fn ident(text: &str) -> Token {
        Token::Ident(text.to_string())
    }

    fn number(text: &str) -> Token {
        Token::NumberLit(text.to_string())
    }

    #[test]
    fn every_operator_of_clause_four_nine_lexes_to_its_own_token() {
        // The two-and three-character operators have to win over their prefixes:
        // `!~=` is not `!=` followed by `~=`, and `>=` is not `>` followed by `=`.
        assert_eq!(
            lex("== != > >= < <= ~= !~="),
            vec![Token::Eq, Token::Neq, Token::Gt, Token::Ge, Token::Lt, Token::Le, Token::Match, Token::NotMatch]
        );
    }

    #[test]
    fn every_structural_character_lexes_to_its_own_token() {
        assert_eq!(
            lex(";|()[]{},"),
            vec![
                Token::Semi,
                Token::Pipe,
                Token::LParen,
                Token::RParen,
                Token::LBracket,
                Token::RBracket,
                Token::LBrace,
                Token::RBrace,
                Token::Comma
            ]
        );
    }

    /// Clause 4.9 `LinkedEntityRelation = AttrName %x7B LinkedEntityPath %x7D`.
    /// The braces are structure; the colon that terminates a type guard is not,
    /// because `:` is a legal identifier character (a prefixed name carries
    /// one), so it stays glued to the identifier the parser splits it from.
    #[test]
    fn a_linked_entity_traversal_lexes_into_its_braces() {
        assert_eq!(
            lex("isParkedIn{Building,Store:category==\"x\"}"),
            vec![
                ident("isParkedIn"),
                Token::LBrace,
                ident("Building"),
                Token::Comma,
                ident("Store:category"),
                Token::Eq,
                Token::QuotedStr("x".to_string()),
                Token::RBrace,
            ]
        );
    }

    #[test]
    fn whitespace_separates_tokens_without_producing_one() {
        assert_eq!(lex(" \t\n\r speed \t "), vec![ident("speed")]);
        assert_eq!(lex(""), Vec::new());
    }

    #[test]
    fn the_range_separator_wins_over_the_sub_attribute_separator() {
        // `1..5` is a range, not `1.` `.5`; `a.b` is a sub-attribute path.
        assert_eq!(lex("1..5"), vec![number("1"), Token::Dots, number("5")]);
        assert_eq!(lex("a.b"), vec![ident("a"), Token::Dot, ident("b")]);
    }

    #[test]
    fn a_fractional_number_does_not_swallow_the_range_separator() {
        assert_eq!(lex("1.5..2.5"), vec![number("1.5"), Token::Dots, number("2.5")]);
    }

    #[test]
    fn numbers_carry_their_sign_fraction_and_exponent() {
        assert_eq!(lex("-3"), vec![number("-3")]);
        assert_eq!(lex("-3.25"), vec![number("-3.25")]);
        assert_eq!(lex("1e3"), vec![number("1e3")]);
        assert_eq!(lex("1.5E-3"), vec![number("1.5E-3")]);
        assert_eq!(lex("-1.5e+3"), vec![number("-1.5e+3")]);
    }

    #[test]
    fn true_and_false_are_keywords_rather_than_identifiers() {
        assert_eq!(lex("true false truthy"), vec![Token::True, Token::False, ident("truthy")]);
    }

    #[test]
    fn a_date_like_literal_is_read_as_one_identifier() {
        // Clause 4.6.3 operands are unquoted, so the digit branch has to hand a
        // whole RFC 3339 date-time to the parser rather than a number followed
        // by punctuation.
        assert_eq!(lex("2017-12-24T12:00:00Z"), vec![ident("2017-12-24T12:00:00Z")]);
        assert_eq!(lex("2017-12-24"), vec![ident("2017-12-24")]);
        assert_eq!(lex("2017-12-24T12:00:00+01:00"), vec![ident("2017-12-24T12:00:00+01:00")]);
    }

    #[test]
    fn a_date_range_stops_at_the_range_separator() {
        assert_eq!(lex("2017-12-24..2017-12-25"), vec![ident("2017-12-24"), Token::Dots, ident("2017-12-25")]);
    }

    /// A bare time of day does not survive the tokeniser: a leading digit run
    /// stays a number unless a `-` follows it, so `12:00:00Z` splits. This is
    /// why `serialize` quotes a `Scalar::Time` operand — the constraint is
    /// recorded here rather than left to be rediscovered.
    #[test]
    fn a_bare_time_of_day_splits_because_the_digit_branch_requires_a_dash() {
        assert_eq!(lex("12:00:00Z"), vec![number("12"), ident(":00:00Z")]);
        assert_eq!(lex("\"12:00:00Z\""), vec![Token::QuotedStr("12:00:00Z".to_string())]);
    }

    #[test]
    fn an_identifier_carries_the_punctuation_a_compact_name_needs() {
        assert_eq!(lex("ngsi-ld:speed_1"), vec![ident("ngsi-ld:speed_1")]);
        assert_eq!(lex("urn:ngsi-ld:Vehicle:A4567"), vec![ident("urn:ngsi-ld:Vehicle:A4567")]);
    }

    /// `.` is the sub-attribute separator (clause 4.9), which outranks its role
    /// inside a hostname: an absolute IRI written unquoted splits at every dot.
    /// That is not a limitation in practice, because an absolute IRI reaches the
    /// AST through term expansion rather than through this tokeniser — what a
    /// client writes is a compact name.
    #[test]
    fn a_dotted_iri_splits_because_the_dot_separates_sub_attributes() {
        assert_eq!(
            lex("https://example.com/Vehicle#speed"),
            vec![ident("https://example"), Token::Dot, ident("com/Vehicle#speed")]
        );
    }

    #[test]
    fn a_quoted_literal_is_taken_verbatim_without_a_second_percent_decode() {
        // Clause 4.9: a quoted operand is a JSON String (RFC 8259 String
        // production), not percent-encoded data. Percent-encoding is purely an
        // HTTP-URI transport concern -- the spec requires it "whenever the
        // string acting as a filter is part of the HTTP binding's URI", and the
        // HTTP binding decodes the whole `q` once before it reaches this
        // tokeniser. A `q` carried in a JSON subscription body is not
        // percent-encoded at all. A second decode here would rewrite `a%2Fb`
        // into `a/b`, corrupting a URN like `urn:x:a%2Fb` and breaking
        // parse->serialize idempotency.
        assert_eq!(lex("\"a%20b\""), vec![Token::QuotedStr("a%20b".to_string())]);
        assert_eq!(lex("\"a%3Bb\""), vec![Token::QuotedStr("a%3Bb".to_string())]);
        assert_eq!(lex("\"urn:x:a%2Fb\""), vec![Token::QuotedStr("urn:x:a%2Fb".to_string())]);
    }

    #[test]
    fn a_backslash_escapes_the_next_character_inside_a_quoted_literal() {
        assert_eq!(lex(r#""say \"hi\"""#), vec![Token::QuotedStr("say \"hi\"".to_string())]);
        assert_eq!(lex(r#""back\\slash""#), vec![Token::QuotedStr(r"back\slash".to_string())]);
    }

    #[test]
    fn an_empty_quoted_literal_is_a_token() {
        assert_eq!(lex("\"\""), vec![Token::QuotedStr(String::new())]);
    }

    #[test]
    fn a_quoted_literal_left_open_is_rejected() {
        let error = tokenize("name==\"unterminated").expect_err("must reject");
        assert!(matches!(error, QueryError::QUnterminatedString), "got {error:?}");
    }

    #[test]
    fn a_character_that_starts_no_token_is_rejected_with_its_position() {
        let error = tokenize("speed==@").expect_err("must reject");
        assert!(matches!(error, QueryError::QUnexpectedChar { ch: '@', position: 7 }), "got {error:?}");
    }

    #[test]
    fn a_whole_predicate_lexes_into_its_parts() {
        assert_eq!(
            lex("temperature.observedAt>=2017-12-24T12:00:00Z;name~=\"^A.*\""),
            vec![
                ident("temperature"),
                Token::Dot,
                ident("observedAt"),
                Token::Ge,
                ident("2017-12-24T12:00:00Z"),
                Token::Semi,
                ident("name"),
                Token::Match,
                Token::QuotedStr("^A.*".to_string()),
            ]
        );
    }

    #[test]
    fn a_compound_value_path_lexes_into_its_brackets() {
        assert_eq!(
            lex("address[city]==\"Berlin\""),
            vec![
                ident("address"),
                Token::LBracket,
                ident("city"),
                Token::RBracket,
                Token::Eq,
                Token::QuotedStr("Berlin".to_string()),
            ]
        );
    }
}
