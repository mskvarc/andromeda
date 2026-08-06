//! Serialize a parsed `q` expression back into NGSI-LD query string format.

use crate::filter::{
    expression::Expr,
    path::AttrPath,
    predicate::Predicate,
    scalar::{CompareValue, Scalar},
};
use std::fmt::Write;

/// Render a parsed filter back into clause 4.9 syntax.
///
/// The result parses back to an equal expression; identifiers that would
/// confuse the tokeniser are re-quoted.
pub fn serialize(expr: &Expr) -> String {
    let mut buf = String::new();
    write_expr(&mut buf, expr);
    buf
}

/// Write the boolean structure of an expression.
fn write_expr(buf: &mut String, expr: &Expr) {
    match expr {
        Expr::And(terms) => write_terms(buf, terms, ';'),
        Expr::Or(terms) => write_terms(buf, terms, '|'),
        Expr::Pred(pred) => write_predicate(buf, pred),
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
fn write_terms(buf: &mut String, terms: &[Expr], separator: char) {
    for (index, term) in terms.iter().enumerate() {
        if index > 0 {
            buf.push(separator);
        }
        if needs_parentheses(term, separator) {
            buf.push('(');
            write_expr(buf, term);
            buf.push(')');
        } else {
            write_expr(buf, term);
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

/// Write one leaf predicate.
fn write_predicate(buf: &mut String, pred: &Predicate) {
    match pred {
        Predicate::Exists(path) => write_attr_path(buf, path),
        Predicate::Compare { path, op, rhs } => {
            write_attr_path(buf, path);
            buf.push_str(op.ngsi_token());
            write_compare_value(buf, rhs);
        }
        Predicate::MatchPattern { path, pattern } => {
            write_attr_path(buf, path);
            buf.push_str("~=");
            write_quoted(buf, pattern.as_str());
        }
        Predicate::NotMatchPattern { path, pattern } => {
            write_attr_path(buf, path);
            buf.push_str("!~=");
            write_quoted(buf, pattern.as_str());
        }
    }
}

/// Write an attribute path, including sub-attributes and compound value keys.
fn write_attr_path(buf: &mut String, path: &AttrPath) {
    write_ident(buf, path.attr.as_str());
    for sub in &path.sub_attrs {
        buf.push('.');
        write_ident(buf, sub.as_str());
    }
    if !path.value_path.is_empty() {
        buf.push('[');
        for (i, vp) in path.value_path.iter().enumerate() {
            if i > 0 {
                buf.push('.');
            }
            write_ident(buf, vp);
        }
        buf.push(']');
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
        // A string is quoted, as the clause 4.9 grammar requires. A time of day
        // is quoted too, for a tokeniser reason rather than a grammatical one:
        // a bare literal that starts with digits is read as a number unless a
        // `-` follows, so `12:00:00Z` would come back as `12` and a stray
        // identifier. A date and a date-time both start `dddd-` and read back
        // whole.
        Scalar::Str(_) | Scalar::Time(_) => write_quoted(buf, &scalar.literal_text()),
        Scalar::Number(_) | Scalar::Bool(_) | Scalar::DateTime(_) | Scalar::Date(_) => buf.push_str(&scalar.literal_text()),
    }
}

/// Write a double-quoted, escaped string literal.
fn write_quoted(buf: &mut String, s: &str) {
    let _ = write!(buf, "\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::filter::{parser::parse, predicate::CmpOp};

    #[test]
    fn roundtrip_simple_compare() {
        let expr = parse("speed>50").unwrap();
        let s = serialize(&expr);
        pretty_assertions::assert_eq!(s, "speed>50");
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
            "moment>=\"12:00:00Z\"",
            "moment==\"12:00:00Z\"..\"13:00:00Z\"",
            "name~=\"^A.*\"",
            "name!~=\"^A.*\"",
            "speed",
        ];
        for shape in shapes {
            let parsed = parse(shape).unwrap_or_else(|error| unreachable!("{shape} must parse: {error}"));
            let rendered = serialize(&parsed);
            pretty_assertions::assert_eq!(rendered, shape, "serialisation of {shape}");
            let reparsed = parse(&rendered).unwrap_or_else(|error| unreachable!("{rendered} must reparse: {error}"));
            pretty_assertions::assert_eq!(reparsed, parsed, "round trip of {shape}");
        }
    }

    /// Clause 4.9's `QueryTermAssoc` is the only way to override the default
    /// precedence, so a group the client wrote has to come back as a group. A
    /// stored subscription is re-serialised whenever it is read back or
    /// updated, and this used to drop the parentheses: `(a|b);c` became
    /// `a|b;c`, which is `a|(b;c)`.
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
            let parsed = parse(input).unwrap_or_else(|error| unreachable!("{input} must parse: {error}"));
            let rendered = serialize(&parsed);
            pretty_assertions::assert_eq!(rendered, expected, "serialisation of {input}");
            let reparsed = parse(&rendered).unwrap_or_else(|error| unreachable!("{rendered} must reparse: {error}"));
            pretty_assertions::assert_eq!(reparsed, parsed, "round trip of {input}");
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
        // After expansion, attrs become full IRIs with dots
        let expr = Expr::Pred(Predicate::Compare {
            path: AttrPath::simple("https://example.com/speed".parse().expect("valid attribute name")),
            op: CmpOp::Greater,
            rhs: CompareValue::Scalar(Scalar::Number(50.into())),
        });
        let s = serialize(&expr);
        // The IRI must be quoted because it contains a dot
        pretty_assertions::assert_eq!(s, "\"https://example.com/speed\">50");
        // And it should parse back
        let parsed = parse(&s).unwrap();
        match parsed {
            Expr::Pred(Predicate::Compare { path, .. }) => {
                pretty_assertions::assert_eq!(path.attr.as_str(), "https://example.com/speed");
            }
            other => unreachable!("expected Compare, got {other:?}"),
        }
    }
}
