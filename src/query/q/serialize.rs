//! Serialize a parsed `q` expression back into NGSI-LD query string format.

use crate::query::q::ast::*;
use std::fmt::Write;

pub fn serialize(expr: &Expr) -> String {
    let mut buf = String::new();
    write_expr(&mut buf, expr);
    buf
}

fn write_expr(buf: &mut String, expr: &Expr) {
    match expr {
        Expr::And(terms) => {
            for (i, term) in terms.iter().enumerate() {
                if i > 0 {
                    buf.push(';');
                }
                write_expr(buf, term);
            }
        }
        Expr::Or(terms) => {
            for (i, term) in terms.iter().enumerate() {
                if i > 0 {
                    buf.push('|');
                }
                write_expr(buf, term);
            }
        }
        Expr::Pred(pred) => write_predicate(buf, pred),
    }
}

fn write_predicate(buf: &mut String, pred: &Predicate) {
    match pred {
        Predicate::Exists(path) => write_attr_path(buf, path),
        Predicate::Compare { path, op, rhs } => {
            write_attr_path(buf, path);
            buf.push_str(op_str(*op));
            write_compare_value(buf, rhs);
        }
        Predicate::MatchPattern { path, pattern } => {
            write_attr_path(buf, path);
            buf.push_str("~=");
            write_quoted(buf, pattern);
        }
        Predicate::NotMatchPattern { path, pattern } => {
            write_attr_path(buf, path);
            buf.push_str("!~=");
            write_quoted(buf, pattern);
        }
    }
}

fn write_attr_path(buf: &mut String, path: &AttrPath) {
    write_ident(buf, &path.attr);
    for sub in &path.sub_attrs {
        buf.push('.');
        write_ident(buf, sub);
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

fn write_ident(buf: &mut String, s: &str) {
    // If the identifier contains characters that would confuse the tokenizer
    // (like dots in URIs), we need to quote it
    if needs_quoting(s) {
        let _ = write!(buf, "\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""));
    } else {
        buf.push_str(s);
    }
}

fn needs_quoting(s: &str) -> bool {
    s.contains('.') || s.contains(' ') || s.contains('"')
}

fn op_str(op: CmpOp) -> &'static str {
    match op {
        CmpOp::Equal => "==",
        CmpOp::Unequal => "!=",
        CmpOp::Greater => ">",
        CmpOp::GreaterEq => ">=",
        CmpOp::Less => "<",
        CmpOp::LessEq => "<=",
    }
}

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

fn write_scalar(buf: &mut String, scalar: &Scalar) {
    match scalar {
        Scalar::Number(n) => {
            let _ = write!(buf, "{}", n);
        }
        Scalar::Bool(b) => {
            let _ = write!(buf, "{}", b);
        }
        Scalar::Str(s) => write_quoted(buf, s),
        Scalar::DateTime(s) | Scalar::Date(s) | Scalar::Time(s) => {
            // Datetimes are unquoted identifiers in the grammar
            buf.push_str(s);
        }
    }
}

fn write_quoted(buf: &mut String, s: &str) {
    let _ = write!(buf, "\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::query::q::parse;

    #[test]
    fn roundtrip_simple_compare() {
        let expr = parse("speed>50").unwrap();
        let s = serialize(&expr);
        assert_eq!(s, "speed>50");
    }

    #[test]
    fn roundtrip_string_compare() {
        let expr = parse("brandName==\"Mercedes\"").unwrap();
        let s = serialize(&expr);
        assert_eq!(s, "brandName==\"Mercedes\"");
    }

    #[test]
    fn roundtrip_and() {
        let expr = parse("speed>50;brandName==\"Mercedes\"").unwrap();
        let s = serialize(&expr);
        assert_eq!(s, "speed>50;brandName==\"Mercedes\"");
    }

    #[test]
    fn roundtrip_range() {
        let expr = parse("temperature==10..20").unwrap();
        let s = serialize(&expr);
        assert_eq!(s, "temperature==10..20");
    }

    #[test]
    fn roundtrip_value_list() {
        let expr = parse("color==\"red\",\"blue\"").unwrap();
        let s = serialize(&expr);
        assert_eq!(s, "color==\"red\",\"blue\"");
    }

    #[test]
    fn serialize_expanded_iri() {
        // After expansion, attrs become full IRIs with dots
        let expr = Expr::Pred(Predicate::Compare {
            path: AttrPath::simple("https://example.com/speed"),
            op: CmpOp::Greater,
            rhs: CompareValue::Scalar(Scalar::Number(50.into())),
        });
        let s = serialize(&expr);
        // The IRI must be quoted because it contains a dot
        assert_eq!(s, "\"https://example.com/speed\">50");
        // And it should parse back
        let parsed = parse(&s).unwrap();
        match parsed {
            Expr::Pred(Predicate::Compare { path, .. }) => {
                assert_eq!(path.attr, "https://example.com/speed");
            }
            _ => panic!("expected Compare"),
        }
    }
}
