//! Serialize a parsed `q` expression back into NGSI-LD query string format.

use crate::{
    filter::{
        Filter,
        attribute_path::AttributePath,
        equality_value::EqualityValue,
        parser::lexer::is_bare_uri,
        query_term::QueryTerm,
        scalar::Scalar,
        trailing_group::TrailingGroup,
        value_path::ValuePath,
    },
    pattern::Pattern,
};
use andromeda_json_literal::json_text::quoted;
use std::fmt;

/// Which spelling of a trailing `[…]` member a rendering writes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MemberSpelling {
    /// The members as the path addresses them: expanded once the filter is.
    Addressed,
    /// The members as the client wrote them, before any expansion.
    Written,
}

// The rendering walks a recursive tree and re-inserts the clause 4.9
// separators, which `derive_more::Display`'s per-variant format string cannot
// express.
impl fmt::Display for Filter {
    /// Renders a parsed filter back into clause 4.9 syntax.
    ///
    /// The result parses back to an equal expression; identifiers that would
    /// confuse the tokeniser are re-quoted.
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut text = String::new();
        write_expr(&mut text, self, MemberSpelling::Addressed);
        formatter.write_str(&text)
    }
}

impl Filter {
    /// Renders the filter into clause 4.9 syntax with every trailing `[…]`
    /// member spelled as the client wrote it, and every other name as
    /// its [`fmt::Display`] spells it.
    ///
    /// An expanded filter addresses a compound member by its expanded name, while
    /// a `LanguageProperty`'s target language and a raw JSON key are read in the
    /// spelling the client wrote (clause 4.9); a filter persisted in its expanded
    /// form keeps that spelling only in this second rendering, which
    /// [`Filter::with_written_members`] reads back.
    #[must_use]
    pub fn written_member_text(&self) -> String {
        let mut text = String::new();
        write_expr(&mut text, self, MemberSpelling::Written);
        text
    }
}

/// Write the boolean structure of an expression.
fn write_expr(buf: &mut String, expr: &Filter, spelling: MemberSpelling) {
    match expr {
        Filter::And(terms) => write_terms(buf, terms, ';', spelling),
        Filter::Or(terms) => write_terms(buf, terms, '|', spelling),
        Filter::Term(pred) => write_predicate(buf, pred, spelling),
    }
}

/// Write the operands of one boolean node, parenthesising any child the
/// grammar would otherwise re-associate.
///
/// Clause 4.9 makes `;` bind tighter than `|` and provides `QueryTermAssoc`
/// (a parenthesised group) as the only way to override that. A child therefore
/// needs its own parentheses unless the default precedence already reproduces
/// it, which is true in exactly one case: a conjunction directly inside a
/// disjunction. Without this, `(a|b);c` rendered as `a|b;c` and read back as
/// `a|(b;c)`, so a stored filter changed meaning the first time it was
/// re-serialised.
fn write_terms(buf: &mut String, terms: &[Filter], separator: char, spelling: MemberSpelling) {
    for (index, term) in terms.iter().enumerate() {
        if index > 0 {
            buf.push(separator);
        }
        if needs_parentheses(term, separator) {
            buf.push('(');
            write_expr(buf, term, spelling);
            buf.push(')');
        } else {
            write_expr(buf, term, spelling);
        }
    }
}

/// Whether a child of a node written with `separator` has to be parenthesised.
const fn needs_parentheses(term: &Filter, separator: char) -> bool {
    match term {
        // A leaf binds tighter than every operator.
        Filter::Term(_) => false,
        // `;` binds tighter than `|`, so a conjunction inside a disjunction
        // reads back unchanged. Every other pairing, including one operator
        // nested inside itself (which would otherwise flatten), does not.
        Filter::And(_) => separator == ';',
        Filter::Or(_) => true,
    }
}

/// Write one leaf predicate: its attribute axis, then its operator and
/// operand, which clause 4.9 writes after a hop's closing brace
/// (`rel{attr}=="x"`).
fn write_predicate(buf: &mut String, pred: &QueryTerm, spelling: MemberSpelling) {
    write_query_path(buf, pred.path(), spelling, PathHead::Query);
    match pred {
        QueryTerm::Exists(_) => {}
        QueryTerm::Equality { membership, value: rhs, .. } => {
            buf.push_str(membership.spelling());
            write_compare_value(buf, rhs);
        }
        QueryTerm::Ordering { operator: op, value: rhs, .. } => {
            buf.push_str(op.spelling());
            write_scalar(buf, rhs);
        }
        QueryTerm::MatchPattern { pattern, .. } => {
            buf.push_str("~=");
            write_pattern(buf, pattern);
        }
        QueryTerm::NotMatchPattern { pattern, .. } => {
            buf.push_str("!~=");
            write_pattern(buf, pattern);
        }
    }
}

/// Where the leading name of a path stands.
#[derive(Debug, Clone, Copy)]
enum PathHead {
    /// At the start of a Query Term.
    Query,
    /// Inside a hop's braces, where a colon ends a type guard.
    Hop,
}

/// Write an attribute axis, every hop's braces closed, its leading name
/// standing at `head`.
fn write_query_path(buf: &mut String, path: &AttributePath, spelling: MemberSpelling, head: PathHead) {
    match path {
        AttributePath::Local(local) => write_attr_path(buf, local, spelling, head),
        AttributePath::Linked {
            relationship,
            guard_types,
            inner,
        } => {
            write_leading_name(buf, relationship.as_str(), head);
            buf.push('{');
            for (index, guard_type) in guard_types.iter().enumerate() {
                if index > 0 {
                    buf.push(',');
                }
                write_hop_head_name(buf, guard_type.as_str());
            }
            if !guard_types.is_empty() {
                buf.push(':');
            }
            write_query_path(buf, inner, spelling, PathHead::Hop);
            buf.push('}');
        }
    }
}

/// Write the leading name of a path standing at `head`.
fn write_leading_name(buf: &mut String, name: &str, head: PathHead) {
    match head {
        PathHead::Query => write_ident(buf, name),
        PathHead::Hop => write_hop_head_name(buf, name),
    }
}

// The rendering quotes each name the tokeniser would otherwise split and
// writes the optional trailing group, which `derive_more::Display`'s format
// string cannot express.
impl fmt::Display for ValuePath {
    /// Renders one attribute path back into clause 4.9 syntax.
    ///
    /// The result parses back to an equal path through [`FromStr`](std::str::FromStr), which is
    /// what lets an `orderBy` term round-trip: clause 4.23.3 carries its path in
    /// the request text, so the parsed term has to be able to spell itself again.
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut text = String::new();
        write_attr_path(&mut text, self, MemberSpelling::Addressed, PathHead::Query);
        formatter.write_str(&text)
    }
}

/// Write an attribute path, including sub-attributes and compound value keys
/// in `spelling`, its attribute standing at `head`.
fn write_attr_path(buf: &mut String, path: &ValuePath, spelling: MemberSpelling, head: PathHead) {
    write_leading_name(buf, path.attribute.as_str(), head);
    for sub in &path.sub_attributes {
        buf.push('.');
        write_ident(buf, sub.as_str());
    }
    match &path.trailing {
        Some(TrailingGroup::Members(members)) => {
            let names = match spelling {
                MemberSpelling::Addressed => &members.addressed,
                MemberSpelling::Written => &members.written,
            };
            buf.push('[');
            for (index, name) in names.iter().enumerate() {
                if index > 0 {
                    buf.push('.');
                }
                write_ident(buf, name.as_str());
            }
            buf.push(']');
        }
        Some(TrailingGroup::Language(language)) => {
            buf.push('[');
            buf.push_str(language.selector().as_str());
            buf.push(']');
        }
        Some(TrailingGroup::AllLanguages) => buf.push_str("[*]"),
        None => {}
    }
}

/// Write a name standing at the head of a hop, a type of its guard or the
/// leading name of the path inside its braces, quoting it whenever it carries
/// a colon.
///
/// Such a name needs quoting the ordinary identifier rule does not ask for. A
/// bare name holding a colon at the head of a hop opens a type guard, and the
/// guard's terminating colon is written straight after the last type, so a
/// bare type or a bare name after the guard that holds a colon of its own
/// leaves the parser two colons and no way to tell which ends the guard, a
/// case the parser rejects. Quoting closes the name and the filter reads back
/// unchanged.
fn write_hop_head_name(buf: &mut String, s: &str) {
    if s.contains(':') {
        write_quoted(buf, s);
    } else {
        write_ident(buf, s);
    }
}

/// Write an identifier, quoting it when the tokeniser would otherwise split it.
fn write_ident(buf: &mut String, s: &str) {
    if needs_quoting(s) {
        write_quoted(buf, s);
    } else {
        buf.push_str(s);
    }
}

/// Whether an identifier carries characters the tokeniser treats as structure.
fn needs_quoting(s: &str) -> bool {
    s.contains('.') || s.contains(' ') || s.contains('"')
}

/// Write the right-hand side of a comparison.
fn write_compare_value(buf: &mut String, val: &EqualityValue) {
    match val {
        EqualityValue::Scalar(s) => write_scalar(buf, s),
        EqualityValue::Range { min, max } => {
            write_scalar(buf, min);
            buf.push_str("..");
            write_scalar(buf, max);
        }
        EqualityValue::ValueList(vals) => {
            for (i, v) in vals.iter().enumerate() {
                if i > 0 {
                    buf.push(',');
                }
                write_scalar(buf, v);
            }
        }
    }
}

/// Write one scalar operand.
fn write_scalar(buf: &mut String, scalar: &Scalar) {
    match scalar {
        // A string is quoted, as the clause 4.9 grammar requires; `dateTime`,
        // `date` and `time` are its unquoted alternatives, and a quoted one
        // would read back as a String.
        Scalar::Str(_) => write_quoted(buf, &scalar.literal_text()),
        // A `URI` an expansion produced can hold characters the tokeniser reads
        // as query syntax; quoted, it reads back as the same text and compares
        // the same.
        Scalar::Uri(uri) if is_bare_uri(uri) => buf.push_str(uri),
        Scalar::Uri(uri) => write_quoted(buf, uri),
        Scalar::Number(_) | Scalar::Bool(_) | Scalar::DateTime(_) | Scalar::Date(_) | Scalar::Time(_) => buf.push_str(&scalar.literal_text()),
    }
}

/// Write a `RegExp` operand between double quotes, as written.
///
/// Clause 4.9 makes `RegExp` its own terminal rather than a `quotedStr`, and
/// the parser reads the body verbatim, so
/// escaping it here would change the pattern: `^\w+$` would become `^\\w+$`, a literal backslash followed by `w`s. The verbatim body is always safe to quote
/// again: the lexer ends a quoted token only at a `"` no backslash escapes, so
/// every `"` the body holds is still escaped, and a body ending in an odd run of
/// backslashes is no regular expression either engine compiles.
fn write_pattern(buf: &mut String, pattern: &Pattern) {
    buf.push('"');
    buf.push_str(pattern.as_str());
    buf.push('"');
}

/// Write a double-quoted string literal, escaped as IETF RFC 8259 section 7
/// requires.
///
/// Clause 4.9 makes `quotedStr` the RFC 8259 `String`, whose `unescaped`
/// production starts at U+0020, so the parser refuses a raw control
/// character. A String decoded from `"a\nb"` holds one, and is written back
/// with it escaped, or the stored, echoed and forwarded text of an accepted
/// filter would no longer parse.
fn write_quoted(buf: &mut String, s: &str) {
    buf.push_str(&quoted(s));
}

#[cfg(test)]
mod tests {
    use crate::filter::{
        Filter,
        attribute_path::AttributePath,
        equality_value::EqualityValue,
        query_term::{OrderingOperator, QueryTerm},
        scalar::Scalar,
        value_path::ValuePath,
    };

    #[test]
    fn roundtrip_simple_compare() {
        let expr = "speed>50".parse::<Filter>().unwrap();
        let s = expr.to_string();
        pretty_assertions::assert_eq!(s, "speed>50");
    }

    #[test]
    fn a_bare_uri_operand_reads_back_bare_and_a_quoted_one_quoted() {
        for written in [
            "link==http://example.org/a",
            "ref!=https://sub.example.org/dir.v2/target.json",
            "ref==urn:example:release:1.5",
            "ref==urn:a,urn:b.c",
            "ref==\"http://example.org/a\"",
        ] {
            pretty_assertions::assert_eq!(written.parse::<Filter>().unwrap().to_string(), written);
        }
    }

    #[test]
    fn a_uri_holding_query_syntax_is_quoted_to_read_back_whole() {
        let Filter::Term(QueryTerm::Equality { path, membership, .. }) = "p==urn:x".parse::<Filter>().unwrap() else {
            panic!("expected an equality");
        };
        let expr = Filter::Term(QueryTerm::Equality {
            path,
            membership,
            value: EqualityValue::Scalar(Scalar::Uri("https://example.org/a;b".to_owned())),
        });
        pretty_assertions::assert_eq!(expr.to_string(), "p==\"https://example.org/a;b\"");
    }

    #[test]
    fn roundtrip_string_compare() {
        let expr = "brandName==\"Mercedes\"".parse::<Filter>().unwrap();
        let s = expr.to_string();
        pretty_assertions::assert_eq!(s, "brandName==\"Mercedes\"");
    }

    #[test]
    fn roundtrip_and() {
        let expr = "speed>50;brandName==\"Mercedes\"".parse::<Filter>().unwrap();
        let s = expr.to_string();
        pretty_assertions::assert_eq!(s, "speed>50;brandName==\"Mercedes\"");
    }

    #[test]
    fn roundtrip_range() {
        let expr = "temperature==10..20".parse::<Filter>().unwrap();
        let s = expr.to_string();
        pretty_assertions::assert_eq!(s, "temperature==10..20");
    }

    #[test]
    fn roundtrip_value_list() {
        let expr = "color==\"red\",\"blue\"".parse::<Filter>().unwrap();
        let s = expr.to_string();
        pretty_assertions::assert_eq!(s, "color==\"red\",\"blue\"");
    }

    /// Every operand shape has to survive the round trip: a filter is
    /// serialised back into clause 4.9 syntax when a subscription is stored and
    /// re-parsed when it is loaded, so a shape that does not round-trip is a
    /// subscription that silently changes meaning.
    #[test]
    fn every_scalar_and_compare_value_shape_round_trips() {
        let shapes = [
            "speed>50",
            "speed==10..20",
            "speed==10,20,30",
            "active==true",
            "colour==\"red\"",
            "colour==\"red\",\"blue\"",
            "observed>=2017-12-24T12:00:00Z",
            "observed==2017-12-24T12:00:00Z..2017-12-25T12:00:00Z",
            "day>=2017-12-24",
            "day==2017-12-24..2017-12-25",
            "moment>=12:00:00Z",
            "moment==12:00:00.500Z..13:00:00Z",
            "moment>=\"12:00:00Z\"",
            "moment==\"12:00:00Z\"..\"13:00:00Z\"",
            "name~=\"^A.*\"",
            "name!~=\"^A.*\"",
            "speed",
        ];
        for shape in shapes {
            let parsed = shape.parse::<Filter>().unwrap_or_else(|error| panic!("{shape} must parse: {error}"));
            let rendered = parsed.to_string();
            pretty_assertions::assert_eq!(rendered, shape, "serialisation of {shape}");
            let reparsed = rendered.parse::<Filter>().unwrap_or_else(|error| panic!("{rendered} must reparse: {error}"));
            pretty_assertions::assert_eq!(reparsed, parsed, "round trip of {shape}");
        }
    }

    #[test]
    fn exact_numeric_operands_round_trip_by_mathematical_value() {
        for expression in [
            "speed==18446744073709551616",
            "speed==0.100000000000000005",
            "speed>-2.5e-400",
            "speed==1e2..1.00000000000000000001e2",
            "speed==0.100000000000000004,0.100000000000000005",
        ] {
            let parsed = expression.parse::<Filter>().unwrap_or_else(|error| panic!("{expression} must parse: {error}"));
            let rendered = parsed.to_string();
            let reparsed = rendered.parse::<Filter>().unwrap_or_else(|error| panic!("{rendered} must reparse: {error}"));
            pretty_assertions::assert_eq!(reparsed, parsed, "mathematical-value round trip of {expression} through {rendered}");
        }
    }

    /// Clause 4.9's `QueryTermAssoc` is the only way to override the default precedence, so a group the client wrote has to come back as a group: `(a|b);c` must not become `a|b;c`, which is `a|(b;c)`. A stored subscription is re-serialised whenever it is read back or updated.
    #[test]
    fn an_explicit_group_survives_the_round_trip() {
        let shapes = [
            ("(a|b);c", "(a|b);c"),
            ("a;(b|c)", "a;(b|c)"),
            ("((a|b);c)|d", "(a|b);c|d"),
            ("(a;b)|c", "a;b|c"),
            ("a|b;c", "a|b;c"),
            ("(a|b)|c", "(a|b)|c"),
            ("(a;b);c", "(a;b);c"),
        ];
        for (input, expected) in shapes {
            let parsed = input.parse::<Filter>().unwrap_or_else(|error| panic!("{input} must parse: {error}"));
            let rendered = parsed.to_string();
            pretty_assertions::assert_eq!(rendered, expected, "serialisation of {input}");
            let reparsed = rendered.parse::<Filter>().unwrap_or_else(|error| panic!("{rendered} must reparse: {error}"));
            pretty_assertions::assert_eq!(reparsed, parsed, "round trip of {input}");
        }
    }

    /// A clause 4.9 linked-entity traversal has to come back the way it went in,
    /// braces, type guard and all: a subscription's `q` is serialised when it is
    /// stored and re-parsed when it is loaded, so a traversal that did not
    /// round-trip would be a subscription that silently changed meaning.
    ///
    /// The expanded shapes matter as much as the compact ones. Expansion turns
    /// every name into an IRI, which carries dots and is therefore re-quoted,
    /// and the guard's terminating colon then stands alone between two quoted
    /// strings rather than glued to an identifier.
    #[test]
    fn a_linked_entity_traversal_round_trips_with_and_without_its_guard() {
        let shapes = [
            "isParkedIn{category}==\"commercial\"",
            "isParkedIn{Building,Store:category}==\"x\"",
            "a{b{c}}==5",
            "a{b}",
            "a{prefix:name}==\"v\"",
            "sensor{Device:humidity}==40",
            "r{T1:s{T2:a}}==1",
            "\"https://example.com/sensor\"{\"https://example.com/Device\":\"https://example.com/humidity\"}==40",
            // A prefixed attribute name at the head of a hop is quoted, before a
            // guard or after one, so its colon cannot be read as one that ends
            // a guard.
            "a{\"prefix:name\"}==\"v\"",
            "rel{Store:\"ex:category\"}==\"x\"",
            "rel{\"ex:link\"{name}}==\"x\"",
            "speed>50;isParkedIn{category}==\"x\"",
            "\"https://example.com/isParkedIn\"{\"https://example.com/Building\",\"https://example.com/Store\":\"https://example.com/category\"}==\"x\"",
            "\"https://example.com/rel\"{Building,Store:\"https://example.com/category\"}==\"x\"",
            "rel{Building,Store:\"https://example.com/category\"}==\"x\"",
            "a{b[c.d]}>=1",
            "a{b{c}}~=\"^n\"",
            // A prefixed guard type is quoted so its own colon cannot be read as
            // the one that ends the guard.
            "rel{Building,\"ex:Store\":category}==\"x\"",
            "rel{\"ex:A\",\"ex:B\":category}==\"x\"",
        ];
        for shape in shapes {
            let parsed = shape.parse::<Filter>().unwrap_or_else(|error| panic!("{shape} must parse: {error}"));
            let rendered = parsed.to_string();
            pretty_assertions::assert_eq!(rendered, shape, "serialisation of {shape}");
            let reparsed = rendered.parse::<Filter>().unwrap_or_else(|error| panic!("{rendered} must reparse: {error}"));
            pretty_assertions::assert_eq!(reparsed, parsed, "round trip of {shape}");
        }
    }

    /// A pattern operand is written back as the client wrote it, backslashes
    /// and escaped quotes included, because the parser reads it verbatim: a
    /// stored Subscription's `q` is serialised and re-parsed, and doubling the
    /// backslashes turned `\w` into a literal backslash followed by `w`.
    #[test]
    fn a_pattern_operand_round_trips_with_its_backslashes_as_written() {
        for shape in [
            r#"name~="^\w+$""#,
            r#"name~="^example\.com$""#,
            r#"name!~="\d{4}-\d{2}""#,
            r#"name~="a\"b""#,
            r#"name~="back\\slash""#,
            r#"name~="[\s\-]""#,
        ] {
            let parsed = shape.parse::<Filter>().unwrap_or_else(|error| panic!("{shape} must parse: {error}"));
            let rendered = parsed.to_string();
            pretty_assertions::assert_eq!(rendered, shape, "serialisation of {shape}");
            let reparsed = rendered.parse::<Filter>().unwrap_or_else(|error| panic!("{rendered} must reparse: {error}"));
            pretty_assertions::assert_eq!(reparsed, parsed, "round trip of {shape}");
        }
    }

    /// A percent sequence inside a quoted operand is neither decoded on parse
    /// nor re-encoded on serialise, so a stored filter round-trips byte for
    /// byte. A quoted operand is a JSON String (clause 4.9), not percent-encoded
    /// data; decoding it would rewrite `a%2Fb` into `a/b` and silently change a
    /// stored subscription's meaning when it is read back and re-serialised.
    #[test]
    fn a_percent_sequence_in_a_quoted_operand_round_trips_unchanged() {
        let expr = "ref==\"urn:x:a%2Fb\"".parse::<Filter>().expect("parses");
        let rendered = expr.to_string();
        pretty_assertions::assert_eq!(rendered, "ref==\"urn:x:a%2Fb\"");
        let reparsed = rendered.parse::<Filter>().expect("reparses");
        pretty_assertions::assert_eq!(reparsed, expr);
    }

    #[test]
    fn serialize_expanded_iri() {
        let expr = Filter::Term(QueryTerm::Ordering {
            path: AttributePath::Local(ValuePath::simple("https://example.com/speed".parse().expect("valid attribute name"))),
            operator: OrderingOperator::Greater,
            value: Scalar::Number(50.into()),
        });
        let s = expr.to_string();
        // The IRI must be quoted because it contains a dot
        pretty_assertions::assert_eq!(s, "\"https://example.com/speed\">50");
        let parsed = s.parse::<Filter>().unwrap();
        match parsed {
            Filter::Term(QueryTerm::Ordering { path, .. }) => {
                pretty_assertions::assert_eq!(path.terminal().attribute.as_str(), "https://example.com/speed");
            }
            other @ (Filter::And(_) | Filter::Or(_) | Filter::Term(_)) => panic!("expected Ordering, got {other:?}"),
        }
    }

    /// Clause 4.9: a bare `time` operand and a quoted `quotedStr` in the same
    /// form are different data types, and each reads back as the one it is.
    #[test]
    fn a_bare_time_and_a_quoted_time_form_string_keep_their_data_types() {
        let operand = |written: &str| {
            let Filter::Term(QueryTerm::Equality {
                value: EqualityValue::Scalar(scalar),
                ..
            }) = (written.parse::<Filter>().unwrap().to_string()).parse::<Filter>().unwrap()
            else {
                panic!("{written} is no single equality");
            };
            scalar
        };
        assert!(matches!(operand("moment==12:00:00Z"), Scalar::Time(_)));
        assert!(matches!(operand("moment==\"12:00:00Z\""), Scalar::Str(_)));
    }

    /// A target language only a tag or range spells, and the `[*]` marker,
    /// read back as the group they were.
    #[test]
    fn a_target_language_and_the_every_language_marker_round_trip() {
        for written in ["label[en-GB]==\"x\"", "label[de-*-DE]==\"x\"", "label[*]==\"x\"", "label[zh-Hant-TW]"] {
            let expr = written.parse::<Filter>().unwrap();
            pretty_assertions::assert_eq!(expr.to_string(), written);
            pretty_assertions::assert_eq!(expr.to_string().parse::<Filter>().unwrap(), expr, "{written}");
            pretty_assertions::assert_eq!(expr.written_member_text(), written);
        }
    }

    /// IETF RFC 8259 section 7 escapes a control character in its short form
    /// where the grammar has one and as a `\u` escape otherwise, so an escape
    /// spelled another way reads back in that spelling.
    #[test]
    fn a_string_holding_a_control_character_is_written_with_it_escaped() {
        for (written, rendered) in [
            (r#"name=="a\nb""#, r#"name=="a\nb""#),
            (r#"name=="a\tb""#, r#"name=="a\tb""#),
            (r#"name=="a\bb""#, r#"name=="a\bb""#),
            (r#"name=="a\fb""#, r#"name=="a\fb""#),
            (r#"name=="a\rb""#, r#"name=="a\rb""#),
            (r#"name=="a\u0001b""#, r#"name=="a\u0001b""#),
            (r#"name=="a\u001Fb""#, r#"name=="a\u001fb""#),
            (r#"name=="a\u000Ab""#, r#"name=="a\nb""#),
            (r#"name=="a\nb","c\td""#, r#"name=="a\nb","c\td""#),
            (r#"name=="a\nb".."c\td""#, r#"name=="a\nb".."c\td""#),
            (r#"name>"a\u0007b""#, r#"name>"a\u0007b""#),
        ] {
            let parsed = written.parse::<Filter>().unwrap_or_else(|error| panic!("{written} must parse: {error}"));
            let serialised = parsed.to_string();
            pretty_assertions::assert_eq!(serialised, rendered, "serialisation of {written}");
            let reparsed = serialised
                .parse::<Filter>()
                .unwrap_or_else(|error| panic!("{serialised} must reparse: {error}"));
            pretty_assertions::assert_eq!(reparsed, parsed, "round trip of {written}");
        }
    }

    /// U+0000 is left out: no stored value can hold it, so no String carries
    /// it to the renderer.
    #[test]
    fn every_control_character_a_string_can_hold_reads_back_through_its_rendering() {
        let Filter::Term(QueryTerm::Equality { path, membership, .. }) = "name==\"x\"".parse::<Filter>().unwrap() else {
            panic!("expected an equality");
        };
        for code_point in 0x01_u32..0x20 {
            let control = char::from_u32(code_point).unwrap();
            let expr = Filter::Term(QueryTerm::Equality {
                path: path.clone(),
                membership,
                value: EqualityValue::Scalar(Scalar::Str(format!("a{control}b"))),
            });
            let rendered = expr.to_string();
            assert!(
                rendered.chars().all(|character| u32::from(character) >= 0x20),
                "U+{code_point:04X} must be written as an escape, got {rendered:?}"
            );
            let reparsed = rendered.parse::<Filter>().unwrap_or_else(|error| panic!("{rendered:?} must reparse: {error}"));
            pretty_assertions::assert_eq!(reparsed, expr, "round trip of U+{code_point:04X}");
            pretty_assertions::assert_eq!(expr.written_member_text(), rendered, "written members of U+{code_point:04X}");
        }
    }
}
