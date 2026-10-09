//! Where the target element of a local `q` path is held in current state.
//!
//! A top-level attribute instance is a `current_attributes` row and its
//! sub-attribute tree is the row's `sub_attributes`, an array whose elements
//! name their parent by position (0 for the instance itself). A path
//! `a.b.c` is therefore the row of `a` and a chain of two elements, `b` under
//! the instance and `c` under `b`. The chain is rechecked inline, one unnest of
//! the array per level joined to the level above by the parent position, so
//! the recheck costs no function call per row.

use crate::{
    attribute_name_key::AttributeNameKey,
    dictionary::resolved_names::ResolvedNames,
    instance_expiry::instance_stands,
    q_predicate::{
        instance_row::{ElementLevel, InstanceRow},
        projection_probe::projection_column,
        sub_attribute::member_field::MemberField,
    },
};
use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
use andromeda_ngsi_ld_query::filter::path::AttrPath;
use andromeda_postgresql_schema::current::{CurrentAttributes, SubAttribute};
use andromeda_postgresql_seaquery::unnest::{exists_in_positional_tree, first_in_positional_tree};
use sea_query::{Alias, Condition, Expr as SeaExpr, ExprTrait, SimpleExpr};
use std::iter::once;

/// The `parent` an element under the top-level instance itself carries.
const INSTANCE_PARENT: i64 = 0;

/// The stored location of a path's target element.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ElementPath {
    /// The dictionary key of the top-level attribute.
    pub attribute: AttributeNameKey,
    /// The dictionary key of each sub-attribute on the way down, outermost
    /// first; empty for the top-level instance itself.
    pub chain: Vec<AttributeNameKey>,
    /// The member of the last instance the path ends on, if it ends on one.
    pub field: Option<MemberField>,
}

impl ElementPath {
    /// The element `path` addresses, or `None` when one of its names was never
    /// recorded: no instance can then hold it.
    #[must_use]
    pub fn resolve(path: &AttrPath, attributes: &ResolvedNames<AttributeName>) -> Option<ElementPath> {
        let attribute = attributes.get(&path.attr)?;
        let (names, field) = match path.sub_attrs.split_last() {
            Some((last, above)) => match MemberField::of(last) {
                Some(field) => (above, Some(field)),
                None => (path.sub_attrs.as_slice(), None),
            },
            None => (path.sub_attrs.as_slice(), None),
        };
        let chain = names.iter().map(|name| attributes.get(name)).collect::<Option<Vec<_>>>()?;
        Some(ElementPath { attribute, chain, field })
    }

    /// The instance whose value (or member) the path's target is: the row for
    /// an attribute or a member of it, else the last element of the chain.
    #[must_use]
    pub const fn holder(&self) -> InstanceRow {
        match self.chain.len().checked_sub(1) {
            Some(last) => InstanceRow::Element(ElementLevel::at(last)),
            None => InstanceRow::Current,
        }
    }

    /// The name path the equality keys hash, the top-level attribute first.
    #[must_use]
    pub fn key_path(&self) -> Vec<i32> {
        once(self.attribute).chain(self.chain.iter().copied()).map(AttributeNameKey::get).collect()
    }

    /// The condition on a row of the attribute that holds when the target
    /// element exists and `value`, read on [`ElementPath::holder`], holds.
    ///
    /// A row's own value is tested on the row. A chain is unnested level by
    /// level, the first level under the instance (`parent = 0`) and each
    /// further one under the element above it, and an element past its clause
    /// 4.22 `expiresAt` holds nothing, like a row.
    #[must_use]
    pub fn condition(&self, value: Condition) -> Condition {
        if self.chain.is_empty() {
            return value;
        }
        Condition::all().add(sub_attributes().is_not_null()).add(exists_in_positional_tree(
            sub_attributes(),
            &self.aliases(),
            &SubAttribute::Parent,
            self.chain_filter(value),
        ))
    }

    /// The first element, in written order, at the end of the chain of a row
    /// of the attribute, as a `sub_attribute` value; `None` for a path with no
    /// chain, whose target the row itself holds.
    #[must_use]
    pub fn first_element(&self) -> Option<SimpleExpr> {
        (!self.chain.is_empty())
            .then(|| first_in_positional_tree(sub_attributes(), &self.aliases(), &SubAttribute::Parent, self.chain_filter(Condition::all())))
    }

    /// The alias of each level of the chain.
    fn aliases(&self) -> Vec<Alias> {
        (0..self.chain.len()).map(|depth| ElementLevel::at(depth).alias()).collect()
    }

    /// The elements of the chain by name, standing, the first one under the
    /// instance itself, and `value` on the last.
    fn chain_filter(&self, value: Condition) -> Condition {
        let named = self.chain.iter().enumerate().fold(Condition::all(), |named, (depth, key)| {
            let level = ElementLevel::at(depth);
            named
                .add(SeaExpr::expr(level.field(SubAttribute::AttributeNameKey)).eq(key.get()))
                .add(instance_stands(level.field(SubAttribute::ExpiresAt)))
        });
        let first = SeaExpr::expr(ElementLevel::at(0).field(SubAttribute::Parent)).eq(INSTANCE_PARENT);
        Condition::all().add(first).add(named).add(value)
    }
}

/// The probed row's `sub_attributes`.
fn sub_attributes() -> SimpleExpr {
    projection_column(CurrentAttributes::SubAttributes)
}

#[cfg(test)]
mod tests {
    use crate::{
        attribute_name_key::AttributeNameKey,
        dictionary::resolved_names::ResolvedNames,
        q_predicate::{
            instance_row::{ElementLevel, InstanceRow},
            sub_attribute::{element_path::ElementPath, member_field::MemberField},
        },
    };
    use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
    use andromeda_ngsi_ld_query::filter::{expression::Expr, parser::parse, path::QueryPath};
    use pretty_assertions::assert_eq;
    use sea_query::{Condition, PostgresQueryBuilder, Query};

    fn names() -> ResolvedNames<AttributeName> {
        ResolvedNames::from_pairs(
            [("a", 2), ("b", 5), ("c", 9)].map(|(name, key)| (format!("https://example.com/{name}").parse().expect("valid name"), AttributeNameKey::from(key))),
        )
    }

    fn element_path(q: &str) -> Option<ElementPath> {
        let Expr::Pred(pred) = parse(q).expect("parse q") else {
            panic!("a single predicate");
        };
        let QueryPath::Local(path) = pred.path() else {
            panic!("a local path");
        };
        ElementPath::resolve(path, &names())
    }

    #[test]
    fn a_path_resolves_to_its_attribute_and_chain() {
        let path = element_path("\"https://example.com/a\".\"https://example.com/b\".\"https://example.com/c\"==3").expect("resolved");
        assert_eq!(path.key_path(), vec![2, 5, 9]);
        assert_eq!(path.holder(), InstanceRow::Element(ElementLevel::at(1)));
        assert_eq!(path.field, None);
    }

    #[test]
    fn a_path_ending_on_a_member_is_held_by_the_instance_above_it() {
        let path = element_path("\"https://example.com/a\".\"https://uri.etsi.org/ngsi-ld/observedAt\">\"2024-01-01T00:00:00Z\"").expect("resolved");
        assert_eq!(path.chain, Vec::new());
        assert_eq!(path.holder(), InstanceRow::Current);
        assert_eq!(path.field, Some(MemberField::ObservedAt));
    }

    #[test]
    fn an_unrecorded_name_on_the_way_resolves_nothing() {
        assert_eq!(element_path("\"https://example.com/a\".\"https://example.com/z\"==3"), None);
    }

    #[test]
    fn a_sub_attribute_recheck_joins_one_unnest_per_level() {
        let path = element_path("\"https://example.com/a\".\"https://example.com/b\".\"https://example.com/c\"==3").expect("resolved");
        let sql = Query::select()
            .cond_where(path.condition(Condition::all()))
            .to_owned()
            .to_string(PostgresQueryBuilder);
        assert!(sql.contains(r#""ca"."sub_attributes" IS NOT NULL"#), "{sql}");
        assert!(
            sql.contains(r#"FROM UNNEST("ca"."sub_attributes") WITH ORDINALITY AS "l0" JOIN UNNEST("ca"."sub_attributes") AS "l1" ON "l1"."parent" = "l0"."ordinality" WHERE "l0"."parent" = 0"#),
            "{sql}"
        );
        assert!(
            sql.contains(r#""l0"."attribute_name_key" = 5"#) && sql.contains(r#""l1"."attribute_name_key" = 9"#),
            "{sql}"
        );
        assert!(!sql.contains("sub_eq_keys("), "{sql}");
    }
}
