//! Which attribute names a compiled `q` filter references by dictionary key.

use crate::q_predicate::sub_attribute::member_field::MemberField;
use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
use andromeda_ngsi_ld_query::filter::{expression::Expr, path::QueryPath};

/// Attribute names a `q` expression reads by dictionary key.
///
/// The attribute of each term and every sub-attribute on its path are keyed:
/// a row and each element of its `sub_attributes` name their attribute by
/// key. A path ending on a member of an instance (`observedAt`, `unitCode`, a
/// timestamp) reads a field rather than an element, and value-path segments
/// are navigated inside a `value_object`, so neither is resolved. A
/// linked-entity traversal contributes the Relationship each hop descends
/// through as well as the terminal attribute, because the hop probes that
/// Relationship's instances by dictionary key.
pub fn referenced_attribute_names(expr: &Expr, out: &mut Vec<AttributeName>) {
    match expr {
        Expr::And(terms) | Expr::Or(terms) => {
            for term in terms {
                referenced_attribute_names(term, out);
            }
        }
        Expr::Pred(pred) => path_attribute_names(pred.path(), out),
    }
}

/// Collect the dictionary-resolvable names one attribute axis mentions.
fn path_attribute_names(path: &QueryPath, out: &mut Vec<AttributeName>) {
    match path {
        QueryPath::Local(local) => {
            // clone: the caller owns the resolved list, which outlives the expression
            out.push(local.attr.clone());
            out.extend(local.sub_attrs.iter().filter(|name| MemberField::of(name).is_none()).cloned());
        }
        QueryPath::Linked { relationship, inner, .. } => {
            // clone: as above
            out.push(relationship.clone());
            path_attribute_names(inner, out);
        }
    }
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::q_predicate::attribute_names::referenced_attribute_names;
    use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
    use andromeda_ngsi_ld_query::filter::parser;
    use pretty_assertions::assert_eq;

    fn collected(q: &str) -> Vec<String> {
        let expr = parser::parse(q).expect("parse q");
        let mut names = Vec::new();
        referenced_attribute_names(&expr, &mut names);
        names.sort();
        names.iter().map(AttributeName::as_str).map(str::to_string).collect()
    }

    #[test]
    fn every_predicate_target_of_a_boolean_expression_is_collected() {
        assert_eq!(
            collected("\"https://example.com/a\">1;\"https://example.com/b\"<2|\"https://example.com/c\"==\"x\""),
            vec!["https://example.com/a", "https://example.com/b", "https://example.com/c"]
        );
    }

    /// A member of an instance is a field of it, so it has no dictionary row
    /// to resolve; a sub-attribute is an element keyed by its name.
    #[test]
    fn sub_attribute_segments_are_collected_and_member_segments_are_not() {
        assert_eq!(
            collected("\"https://example.com/temperature\".\"https://uri.etsi.org/ngsi-ld/observedAt\">=\"2024-01-01T00:00:00Z\""),
            vec!["https://example.com/temperature"]
        );
        assert_eq!(
            collected("\"https://example.com/temperature\".\"https://example.com/accuracy\".\"https://example.com/method\"==\"x\""),
            vec!["https://example.com/accuracy", "https://example.com/method", "https://example.com/temperature"]
        );
    }

    #[test]
    fn every_relationship_a_traversal_descends_is_collected_with_the_terminal_attribute() {
        // Clause 4.9: each hop probes the instances of its
        // Relationship by dictionary key, exactly as the terminal comparison
        // does.
        assert_eq!(
            collected("\"https://example.com/outer\"{\"https://example.com/inner\"{\"https://example.com/zone\"}}==\"north\""),
            vec!["https://example.com/inner", "https://example.com/outer", "https://example.com/zone"]
        );
    }
}
