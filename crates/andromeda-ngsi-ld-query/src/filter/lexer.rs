//! Tokeniser for the NGSI-LD Query Language (clause 4.9).

use crate::{error::QueryError, filter::json_string::scan_quoted_literal};

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
    #[must_use]
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
/// # Errors
///
/// Returns [`QueryError::QUnterminatedString`] when a double-quoted literal is
/// left open, and [`QueryError::QUnexpectedChar`] when a character can start no
/// token of the clause 4.9 grammar.
pub fn tokenize(input: &str) -> Result<Vec<Token>, QueryError> {
    let chars: Vec<char> = input.chars().collect();
    let mut tokens = Vec::new();
    let mut cursor = 0;

    while cursor < chars.len() {
        if matches!(chars[cursor], ' ' | '\t' | '\n' | '\r') {
            cursor += 1;
            continue;
        }
        let (token, next) = scan_token(&chars, cursor)?;
        tokens.push(token);
        cursor = next;
    }

    Ok(tokens)
}

/// Scan the single token that starts at `start`, returning it together with the
/// index that follows it.
///
/// The multi-character operators are matched before their prefixes, because
/// clause 4.9 spells `!~=` with a `!` that does not open a `!=`.
fn scan_token(chars: &[char], start: usize) -> Result<(Token, usize), QueryError> {
    match chars[start] {
        ';' => Ok((Token::Semi, start + 1)),
        '|' => Ok((Token::Pipe, start + 1)),
        '(' => Ok((Token::LParen, start + 1)),
        ')' => Ok((Token::RParen, start + 1)),
        '[' => Ok((Token::LBracket, start + 1)),
        ']' => Ok((Token::RBracket, start + 1)),
        '{' => Ok((Token::LBrace, start + 1)),
        '}' => Ok((Token::RBrace, start + 1)),
        '*' => Ok((Token::Star, start + 1)),
        ',' => Ok((Token::Comma, start + 1)),
        '=' if is_at(chars, start + 1, '=') => Ok((Token::Eq, start + 2)),
        '!' if is_at(chars, start + 1, '~') && is_at(chars, start + 2, '=') => Ok((Token::NotMatch, start + 3)),
        '!' if is_at(chars, start + 1, '=') => Ok((Token::Neq, start + 2)),
        '~' if is_at(chars, start + 1, '=') => Ok((Token::Match, start + 2)),
        '>' if is_at(chars, start + 1, '=') => Ok((Token::Ge, start + 2)),
        '>' => Ok((Token::Gt, start + 1)),
        '<' if is_at(chars, start + 1, '=') => Ok((Token::Le, start + 2)),
        '<' => Ok((Token::Lt, start + 1)),
        '.' if is_at(chars, start + 1, '.') => Ok((Token::Dots, start + 2)),
        '.' => Ok((Token::Dot, start + 1)),
        '"' => scan_quoted_string(chars, start),
        '-' if starts_digit(chars, start + 1) => Ok(scan_signed_number(chars, start)),
        digit if digit.is_ascii_digit() => Ok(scan_digit_run(chars, start)),
        name if name.is_alphanumeric() || matches!(name, '_' | ':' | '/') => Ok(scan_identifier(chars, start)),
        other => Err(QueryError::QUnexpectedChar { ch: other, position: start }),
    }
}

/// Whether `chars` holds `expected` at `index`.
fn is_at(chars: &[char], index: usize, expected: char) -> bool {
    chars.get(index) == Some(&expected)
}

/// Whether `chars` holds an ASCII digit at `index`.
fn starts_digit(chars: &[char], index: usize) -> bool {
    chars.get(index).is_some_and(char::is_ascii_digit)
}

/// Advance past a run of ASCII digits, returning the index that follows it.
fn consume_digits(chars: &[char], from: usize) -> usize {
    let mut cursor = from;
    while starts_digit(chars, cursor) {
        cursor += 1;
    }
    cursor
}

/// Scan a double-quoted literal, keeping its body verbatim.
///
/// Clause 4.9 gives a `quotedStr` and a `RegExp` the same lexical shape and
/// different grammars, and the tokeniser cannot see which production the
/// literal stands in. The escape rules therefore belong to whichever operand
/// parser consumes the token, not here.
fn scan_quoted_string(chars: &[char], start: usize) -> Result<(Token, usize), QueryError> {
    let (body, next) = scan_quoted_literal(chars, start)?;
    Ok((Token::QuotedStr(body), next))
}

/// Consume the optional fraction and exponent of a numeric literal, starting at
/// the index that follows its integer digits.
///
/// A `.` is only a decimal point when a digit follows it and it does not open
/// the `..` range separator, which otherwise would be swallowed by `1..5`.
fn scan_number_tail(chars: &[char], from: usize) -> usize {
    let mut cursor = from;
    if is_at(chars, cursor, '.') && !is_at(chars, cursor + 1, '.') && starts_digit(chars, cursor + 1) {
        cursor = consume_digits(chars, cursor + 1);
    }
    if is_at(chars, cursor, 'e') || is_at(chars, cursor, 'E') {
        cursor += 1;
        if is_at(chars, cursor, '+') || is_at(chars, cursor, '-') {
            cursor += 1;
        }
        cursor = consume_digits(chars, cursor);
    }
    cursor
}

/// Scan a numeric literal that opens with a `-` sign.
fn scan_signed_number(chars: &[char], start: usize) -> (Token, usize) {
    let end = scan_number_tail(chars, consume_digits(chars, start + 1));
    (Token::NumberLit(chars[start..end].iter().collect()), end)
}

/// Scan a token that opens with an ASCII digit.
///
/// Clause 4.9 writes its `dateTime`, `date` and `time` operands unquoted, and
/// each of the three opens with a digit run that a `Number` cannot continue
/// with: RFC 8259's `number` production carries neither `-` after its integer
/// digits nor `:` at all, and clause 4.6.2 makes every `AttrName` open with a
/// letter, so a digit run followed by either separator is a clause 4.6.3
/// literal rather than a number followed by punctuation. Without the `:` case
/// the `time` production had no unquoted spelling at all: `q=readingTime==\
/// 12:00:00Z` lexed as the number `12` followed by an unexpected identifier.
fn scan_digit_run(chars: &[char], start: usize) -> (Token, usize) {
    let after_digits = consume_digits(chars, start);
    let opens_time_value = (is_at(chars, after_digits, '-') || is_at(chars, after_digits, ':')) && starts_digit(chars, after_digits + 1);
    if opens_time_value {
        return scan_time_value(chars, start, after_digits);
    }
    let end = scan_number_tail(chars, after_digits);
    (Token::NumberLit(chars[start..end].iter().collect()), end)
}

/// Consume a clause 4.6.3 time value literal as one identifier, stopping before
/// the `..` range separator so that `2017-12-24..2017-12-25` stays a range.
fn scan_time_value(chars: &[char], start: usize, from: usize) -> (Token, usize) {
    let mut cursor = from;
    while chars.get(cursor).copied().is_some_and(is_temporal_char) {
        if is_at(chars, cursor, '.') && is_at(chars, cursor + 1, '.') {
            break;
        }
        cursor += 1;
    }
    (Token::Ident(chars[start..cursor].iter().collect()), cursor)
}

/// Whether `current` may appear inside an unquoted clause 4.6.3 time value.
///
/// Wider than the conformant forms on purpose: a numeric zone offset is
/// consumed into the literal so that the operand parser can refuse it as a
/// malformed `dateTime` or `time`, rather than the tokeniser splitting it and
/// reporting an unexpected character at some position inside it.
fn is_temporal_char(current: char) -> bool {
    current.is_alphanumeric() || matches!(current, '-' | ':' | '.' | '+')
}

/// Scan a bare identifier — an attribute name, a prefixed name, or an IRI a
/// client wrote unquoted — and recognise the two boolean keywords.
fn scan_identifier(chars: &[char], start: usize) -> (Token, usize) {
    let mut cursor = start;
    while chars.get(cursor).copied().is_some_and(is_identifier_char) {
        cursor += 1;
    }
    let word: String = chars[start..cursor].iter().collect();
    let token = match word.as_str() {
        "true" => Token::True,
        "false" => Token::False,
        // `&str` is unbounded, so a catch-all is the only way to close this
        // match; every other word is an identifier.
        _ => Token::Ident(word),
    };
    (token, cursor)
}

/// Whether `current` may appear inside a bare identifier.
///
/// `.` is excluded because clause 4.9 makes it the sub-attribute separator.
fn is_identifier_char(current: char) -> bool {
    current.is_alphanumeric() || matches!(current, '_' | ':' | '/' | '-' | '#')
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

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

    /// Clause 4.9's `ComparableValue` admits a bare `time` beside `dateTime`
    /// and `date`, so a time of day survives the tokeniser whole. It used to
    /// split — `12:00:00Z` lexed as the number `12` followed by the identifier
    /// `:00:00Z` — which left the `time` production with no unquoted spelling
    /// at all and made every such filter a `BadRequestData`.
    #[test]
    fn a_bare_time_of_day_is_one_literal() {
        assert_eq!(lex("12:00:00Z"), vec![ident("12:00:00Z")]);
        assert_eq!(lex("12:00:00.500000Z"), vec![ident("12:00:00.500000Z")]);
        assert_eq!(lex("\"12:00:00Z\""), vec![Token::QuotedStr("12:00:00Z".to_string())]);
    }

    /// A digit run that no clause 4.6.3 separator continues is still the
    /// `Number` production of clause 4.9.
    #[test]
    fn a_digit_run_that_no_separator_continues_is_still_a_number() {
        assert_eq!(lex("12"), vec![number("12")]);
        assert_eq!(lex("12.5"), vec![number("12.5")]);
        assert_eq!(lex("12..20"), vec![number("12"), Token::Dots, number("20")]);
    }

    /// A time-of-day range stops at the range separator exactly as a date range
    /// does: the `..` is clause 4.9's `dots`, not a fractional part.
    #[test]
    fn a_time_of_day_range_stops_at_the_range_separator() {
        assert_eq!(lex("09:00:00Z..17:00:00Z"), vec![ident("09:00:00Z"), Token::Dots, ident("17:00:00Z")]);
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
    fn a_backslash_defers_the_next_character_without_resolving_it() {
        // The token carries the body as written. Clause 4.9 gives a `quotedStr`
        // and a `RegExp` the same lexical shape and different grammars, so which
        // escapes mean what is settled by the operand parser that knows the
        // production, not here. The escaped quote still does not end the token.
        assert_eq!(lex(r#""say \"hi\"""#), vec![Token::QuotedStr(r#"say \"hi\""#.to_string())]);
        assert_eq!(lex(r#""back\\slash""#), vec![Token::QuotedStr(r"back\\slash".to_string())]);
        assert_eq!(lex(r#""^example\.com$""#), vec![Token::QuotedStr(r"^example\.com$".to_string())]);
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
