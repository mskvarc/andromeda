//! Serialize a parsed `q` expression back into NGSI-LD query string format.

use crate::{
    filter::{
        expression::Expr,
        lexer::is_bare_uri,
        path::{AttrPath, QueryPath},
        predicate::Predicate,
        scalar::{CompareValue, Scalar},
        trailing_path::TrailingPath,
    },
    pattern::Pattern,
};
use std::fmt::Write;

/// Which spelling of a trailing `[…]` member a rendering writes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MemberSpelling {
    /// The members as the path addresses them: expanded once the filter is.
    Addressed,
    /// The members as the client wrote them, before any expansion.
    Written,
}

/// Render a parsed filter back into clause 4.9 syntax.
///
/// The result parses back to an equal expression; identifiers that would
/// confuse the tokeniser are re-quoted.
#[must_use]
pub fn serialize(expr: &Expr) -> String {
    let mut buf = String::new();
    write_expr(&mut buf, expr, MemberSpelling::Addressed);
    buf
}

/// Render a parsed filter into clause 4.9 syntax with every trailing `[…]`
/// member spelled as the client wrote it, and every other name as
/// [`serialize`] spells it.
///
/// An expanded filter addresses a compound member by its expanded name, while
/// a `LanguageProperty`'s target language and a raw JSON key are read in the
/// spelling the client wrote (clause 4.9); a filter persisted in its expanded
/// form keeps that spelling only in this second rendering, which
/// [`Expr::with_written_members`] reads back.
#[must_use]
pub fn serialize_written_members(expr: &Expr) -> String {
    let mut buf = String::new();
    write_expr(&mut buf, expr, MemberSpelling::Written);
    buf
}

/// Write the boolean structure of an expression.
fn write_expr(buf: &mut String, expr: &Expr, spelling: MemberSpelling) {
    match expr {
        Expr::And(terms) => write_terms(buf, terms, ';', spelling),
        Expr::Or(terms) => write_terms(buf, terms, '|', spelling),
        Expr::Pred(pred) => write_predicate(buf, pred, spelling),
    }
}

/// Write the operands of one boolean node, parenthesising any child the
/// grammar would otherwise re-associate.
///
/// Clause 4.9 makes `;` bind tighter than `|` and provides `QueryTermAssoc` —
/// a parenthesised group — as the only way to override that. A child therefore
/// needs its own parentheses unless the default precedence already reproduces
/// it, which is true in exactly one case: a conjunction directly inside a
/// disjunction. Without this, `(a|b);c` rendered as `a|b;c` and read back as
/// `a|(b;c)`, so a stored filter changed meaning the first time it was
/// re-serialised.
fn write_terms(buf: &mut String, terms: &[Expr], separator: char, spelling: MemberSpelling) {
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
const fn needs_parentheses(term: &Expr, separator: char) -> bool {
    match term {
        // A leaf binds tighter than every operator.
        Expr::Pred(_) => false,
        // `;` binds tighter than `|`, so a conjunction inside a disjunction
        // reads back unchanged. Every other pairing — including one operator
        // nested inside itself, which would otherwise flatten — does not.
        Expr::And(_) => separator == ';',
        Expr::Or(_) => true,
    }
}

/// Write one leaf predicate: its attribute axis, then its operator and
/// operand, which clause 4.9 writes after a hop's closing brace
/// (`rel{attr}=="x"`).
fn write_predicate(buf: &mut String, pred: &Predicate, spelling: MemberSpelling) {
    write_query_path(buf, pred.path(), spelling);
    match pred {
        Predicate::Exists(_) => {}
        Predicate::Equality { membership, rhs, .. } => {
            buf.push_str(membership.ngsi_token());
            write_compare_value(buf, rhs);
        }
        Predicate::Ordering { op, rhs, .. } => {
            buf.push_str(op.ngsi_token());
            write_scalar(buf, rhs);
        }
        Predicate::MatchPattern { pattern, .. } => {
            buf.push_str("~=");
            write_pattern(buf, pattern);
        }
        Predicate::NotMatchPattern { pattern, .. } => {
            buf.push_str("!~=");
            write_pattern(buf, pattern);
        }
    }
}

/// Write an attribute axis, every hop's braces closed.
fn write_query_path(buf: &mut String, path: &QueryPath, spelling: MemberSpelling) {
    match path {
        QueryPath::Local(local) => write_attr_path(buf, local, spelling),
        QueryPath::Linked {
            relationship,
            guard_types,
            inner,
        } => {
            write_ident(buf, relationship.as_str());
            buf.push('{');
            for (index, guard_type) in guard_types.iter().enumerate() {
                if index > 0 {
                    buf.push(',');
                }
                write_guard_type(buf, guard_type.as_str());
            }
            if !guard_types.is_empty() {
                buf.push(':');
            }
            write_query_path(buf, inner, spelling);
            buf.push('}');
        }
    }
}

/// Render one attribute path back into clause 4.9 syntax.
///
/// The result parses back to an equal path through
/// [`parse_attribute_path`](crate::filter::value_path::parse_attribute_path),
/// which is what lets an `orderBy` term round-trip: clause 4.23.3 carries its
/// path in the request text, so the parsed term has to be able to spell itself
/// again.
#[must_use]
pub fn serialize_attr_path(path: &AttrPath) -> String {
    let mut buffer = String::new();
    write_attr_path(&mut buffer, path, MemberSpelling::Addressed);
    buffer
}

/// Write an attribute path, including sub-attributes and compound value keys
/// in `spelling`.
fn write_attr_path(buf: &mut String, path: &AttrPath, spelling: MemberSpelling) {
    write_ident(buf, path.attr.as_str());
    for sub in &path.sub_attrs {
        buf.push('.');
        write_ident(buf, sub.as_str());
    }
    match &path.trailing {
        Some(TrailingPath::Members(members)) => {
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
        Some(TrailingPath::Language(language)) => {
            buf.push('[');
            buf.push_str(language.selector().as_str());
            buf.push(']');
        }
        Some(TrailingPath::AllLanguages) => buf.push_str("[*]"),
        None => {}
    }
}

/// Write one entity type of a hop's guard, quoting it whenever it carries a
/// colon.
///
/// A guard type needs quoting the ordinary identifier rule does not ask for: the
/// guard's terminating colon is written straight after the last type, so a bare
/// type that contains one of its own leaves the parser two colons and no way to
/// tell which ends the guard — the parser rejects exactly that. Quoting closes
/// the type before the terminator and the filter reads back unchanged.
fn write_guard_type(buf: &mut String, s: &str) {
    if s.contains(':') {
        write_quoted(buf, s);
    } else {
        write_ident(buf, s);
    }
}

/// Write an identifier, quoting it when the tokeniser would otherwise split it.
fn write_ident(buf: &mut String, s: &str) {
    if needs_quoting(s) {
        let _ = write!(buf, "\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""));
    } else {
        buf.push_str(s);
    }
}

/// Whether an identifier carries characters the tokeniser treats as structure.
fn needs_quoting(s: &str) -> bool {
    s.contains('.') || s.contains(' ') || s.contains('"')
}

/// Write the right-hand side of a comparison.
fn write_compare_value(buf: &mut String, val: &CompareValue) {
    match val {
        CompareValue::Scalar(s) => write_scalar(buf, s),
        CompareValue::Range { min, max } => {
            write_scalar(buf, min);
            buf.push_str("..");
            write_scalar(buf, max);
        }
        CompareValue::ValueList(vals) => {
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
/// [`crate::filter::operand::parse_pattern_string`] reads the body verbatim, so
/// escaping it here would change the pattern: `^\w+$` would become `^\\w+$`, a literal backslash followed by `w`s. The verbatim body is always safe to quote
/// again: the lexer ends a quoted token only at a `"` no backslash escapes, so
/// every `"` the body holds is still escaped, and a body ending in an odd run of
/// backslashes is no regular expression either engine compiles.
fn write_pattern(buf: &mut String, pattern: &Pattern) {
    buf.push('"');
    buf.push_str(pattern.as_str());
    buf.push('"');
}

/// Write a double-quoted, escaped string literal.
fn write_quoted(buf: &mut String, s: &str) {
    let _ = write!(buf, "\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::filter::{parser::parse, predicate::OrderingOp};

    #[test]
    fn roundtrip_simple_compare() {
        let expr = parse("speed>50").unwrap();
        let s = serialize(&expr);
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
            pretty_assertions::assert_eq!(serialize(&parse(written).unwrap()), written);
        }
    }

    #[test]
    fn a_uri_holding_query_syntax_is_quoted_to_read_back_whole() {
        let Expr::Pred(Predicate::Equality { path, membership, .. }) = parse("p==urn:x").unwrap() else {
            panic!("expected an equality");
        };
        let expr = Expr::Pred(Predicate::Equality {
            path,
            membership,
            rhs: CompareValue::Scalar(Scalar::Uri("https://example.org/a;b".to_owned())),
        });
        pretty_assertions::assert_eq!(serialize(&expr), "p==\"https://example.org/a;b\"");
    }

    #[test]
    fn roundtrip_string_compare() {
        let expr = parse("brandName==\"Mercedes\"").unwrap();
        let s = serialize(&expr);
        pretty_assertions::assert_eq!(s, "brandName==\"Mercedes\"");
    }

    #[test]
    fn roundtrip_and() {
        let expr = parse("speed>50;brandName==\"Mercedes\"").unwrap();
        let s = serialize(&expr);
        pretty_assertions::assert_eq!(s, "speed>50;brandName==\"Mercedes\"");
    }

    #[test]
    fn roundtrip_range() {
        let expr = parse("temperature==10..20").unwrap();
        let s = serialize(&expr);
        pretty_assertions::assert_eq!(s, "temperature==10..20");
    }

    #[test]
    fn roundtrip_value_list() {
        let expr = parse("color==\"red\",\"blue\"").unwrap();
        let s = serialize(&expr);
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
            let parsed = parse(shape).unwrap_or_else(|error| panic!("{shape} must parse: {error}"));
            let rendered = serialize(&parsed);
            pretty_assertions::assert_eq!(rendered, shape, "serialisation of {shape}");
            let reparsed = parse(&rendered).unwrap_or_else(|error| panic!("{rendered} must reparse: {error}"));
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
            let parsed = parse(expression).unwrap_or_else(|error| panic!("{expression} must parse: {error}"));
            let rendered = serialize(&parsed);
            let reparsed = parse(&rendered).unwrap_or_else(|error| panic!("{rendered} must reparse: {error}"));
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
            let parsed = parse(input).unwrap_or_else(|error| panic!("{input} must parse: {error}"));
            let rendered = serialize(&parsed);
            pretty_assertions::assert_eq!(rendered, expected, "serialisation of {input}");
            let reparsed = parse(&rendered).unwrap_or_else(|error| panic!("{rendered} must reparse: {error}"));
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
            let parsed = parse(shape).unwrap_or_else(|error| panic!("{shape} must parse: {error}"));
            let rendered = serialize(&parsed);
            pretty_assertions::assert_eq!(rendered, shape, "serialisation of {shape}");
            let reparsed = parse(&rendered).unwrap_or_else(|error| panic!("{rendered} must reparse: {error}"));
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
            let parsed = parse(shape).unwrap_or_else(|error| panic!("{shape} must parse: {error}"));
            let rendered = serialize(&parsed);
            pretty_assertions::assert_eq!(rendered, shape, "serialisation of {shape}");
            let reparsed = parse(&rendered).unwrap_or_else(|error| panic!("{rendered} must reparse: {error}"));
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
        let expr = parse("ref==\"urn:x:a%2Fb\"").expect("parses");
        let rendered = serialize(&expr);
        pretty_assertions::assert_eq!(rendered, "ref==\"urn:x:a%2Fb\"");
        let reparsed = parse(&rendered).expect("reparses");
        pretty_assertions::assert_eq!(reparsed, expr);
    }

    #[test]
    fn serialize_expanded_iri() {
        let expr = Expr::Pred(Predicate::Ordering {
            path: QueryPath::Local(AttrPath::simple("https://example.com/speed".parse().expect("valid attribute name"))),
            op: OrderingOp::Greater,
            rhs: Scalar::Number(50.into()),
        });
        let s = serialize(&expr);
        // The IRI must be quoted because it contains a dot
        pretty_assertions::assert_eq!(s, "\"https://example.com/speed\">50");
        let parsed = parse(&s).unwrap();
        match parsed {
            Expr::Pred(Predicate::Ordering { path, .. }) => {
                pretty_assertions::assert_eq!(path.terminal().attr.as_str(), "https://example.com/speed");
            }
            other @ (Expr::And(_) | Expr::Or(_) | Expr::Pred(_)) => panic!("expected Ordering, got {other:?}"),
        }
    }

    /// Clause 4.9: a bare `time` operand and a quoted `quotedStr` in the same
    /// form are different data types, and each reads back as the one it is.
    #[test]
    fn a_bare_time_and_a_quoted_time_form_string_keep_their_data_types() {
        let operand = |written: &str| {
            let Expr::Pred(Predicate::Equality {
                rhs: CompareValue::Scalar(scalar),
                ..
            }) = parse(&serialize(&parse(written).unwrap())).unwrap()
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
            let expr = parse(written).unwrap();
            pretty_assertions::assert_eq!(serialize(&expr), written);
            pretty_assertions::assert_eq!(parse(&serialize(&expr)).unwrap(), expr, "{written}");
            pretty_assertions::assert_eq!(serialize_written_members(&expr), written);
        }
    }
}
