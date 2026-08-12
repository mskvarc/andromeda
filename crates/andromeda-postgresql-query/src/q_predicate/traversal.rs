//! The clause 4.9 linked-entity traversal axis compiled to a correlated probe.
//!
//! `rel{attr=="x"}` asks about an attribute of the entity a Relationship points
//! at, so answering it means leaving the row being filtered. One hop becomes one
//! `EXISTS`: the Relationship's instances are read from
//! `current_attribute_values`, `value_uri` is joined to `entities.id` to reach
//! the entity each instance names, and the rest of the predicate is compiled
//! against that entity. Hops nest, so the probes nest with them.
//!
//! The join stays on `entities.id` because a Relationship's object is the
//! Entity's URI (clause 4.5.3); it is the surrogate `entity_key` of the row that
//! join lands on that the inner predicate then correlates against.
//!
//! This lives apart from the comparison rendering next door because it changes
//! *which row* is being asked about rather than *what is asked* of it — the two
//! change for unrelated reasons.

use crate::{
    attribute_name_id::AttributeNameId,
    column_reference::{qualified, quoted},
    operator::array_contains,
    q_predicate::target::QFilterTarget,
    schema::{CurrentAttributeValues, Entities},
};
use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
use sea_query::{Alias, Condition, Expr as SeaExpr, SimpleExpr};

/// One hop of a traversal: the relations its probe introduces, and the probe
/// itself.
///
/// The aliases are suffixed by the hop's depth so a nested traversal's probe
/// cannot shadow the one enclosing it, and they are decided once here rather
/// than by the caller, because the caller needs the *same* aliases twice — once
/// to compile the inner predicate against the entity this hop lands on, and
/// again to render the probe around it.
pub(crate) struct LinkedHop {
    /// Alias of this hop's `current_attribute_values` scan.
    attribute_alias: String,
    /// Alias of the `entities` row this hop lands on.
    linked_alias: String,
}

impl LinkedHop {
    /// The hop `depth` levels into a traversal, counting from one.
    pub(crate) fn at_depth(depth: usize) -> LinkedHop {
        LinkedHop {
            attribute_alias: format!("cav_ln{depth}"),
            linked_alias: format!("le{depth}"),
        }
    }

    /// Where a predicate compiled *inside* this hop reads its entity.
    pub(crate) fn inner_target(&self) -> QFilterTarget {
        QFilterTarget::correlated(
            (Alias::new(self.linked_alias.as_str()), Entities::EntityKey),
            (Alias::new(self.linked_alias.as_str()), Entities::Payload),
        )
    }

    /// Wrap an already-compiled inner predicate in this hop's probe.
    ///
    /// The probe is an `EXISTS` over the Relationship's instances in
    /// `current_attribute_values`, joined to the entity each one names,
    /// correlated against `outer`. Reading the object from `value_uri` rather
    /// than the payload keeps the probe on the `(attribute_name_id, value_uri)`
    /// index, and the join to `entities.id` is that table's unique external key.
    ///
    /// A deleted Relationship needs no exclusion: the projection holds no
    /// tombstone row, so an instance that is present names an entity.
    pub(crate) fn probe(&self, relationship_id: AttributeNameId, guard_types: &[AttributeName], outer: &QFilterTarget, inner: Condition) -> Condition {
        let attribute_alias = self.attribute_alias.as_str();
        let linked_alias = self.linked_alias.as_str();
        // The markers are positional over the expressions collected below, not bind
        // placeholders: sea-query renders each expression in turn, so the guard and
        // the inner condition contribute their own binds where they are written.
        //
        // Both substituted conditions are parenthesised. A multi-type guard renders
        // as a bare `a OR b`, and spliced unparenthesised into this `AND` chain its
        // second disjunct would escape the correlation and the attribute-name test
        // entirely — the probe would match whenever *any* entity in the tenant
        // carried the second type. The inner condition is wrapped for the same
        // reason, before some future compilation of it renders a top-level `OR` too.
        let mut operands: Vec<SimpleExpr> = vec![SeaExpr::val(relationship_id.get()).into()];
        let guard = if guard_types.is_empty() {
            String::new()
        } else {
            operands.push(type_guard(guard_types, linked_alias));
            format!(" AND (${})", operands.len())
        };
        let inner_marker = format!("(${})", operands.len() + 1);
        operands.push(inner.into());

        let hop_alias = Alias::new(attribute_alias);
        let linked = Alias::new(linked_alias);
        let sql = format!(
            "EXISTS (SELECT 1 FROM {projection} AS \"{attribute_alias}\" \
             JOIN {entities} AS \"{linked_alias}\" ON {linked_id} = {object} \
             WHERE {hop_key} = {corr} \
             AND {hop_attr} = $1{guard} \
             AND {inner_marker})",
            projection = quoted(&CurrentAttributeValues::Table),
            entities = quoted(&Entities::Table),
            linked_id = qualified(&linked, &Entities::Id),
            object = qualified(&hop_alias, &CurrentAttributeValues::ValueUri),
            hop_key = qualified(&hop_alias, &CurrentAttributeValues::EntityKey),
            hop_attr = qualified(&hop_alias, &CurrentAttributeValues::AttributeNameId),
            corr = outer.entity_key_column(),
        );
        Condition::all().add(SeaExpr::cust_with_exprs(sql, operands))
    }
}

/// The hop's entity-type constraint against the linked entity's `types` column.
///
/// Clause 4.9 lists the guard's types as alternatives, so the constraint is a
/// disjunction of array-membership tests — the same leaf shape a `type`
/// selection renders (clause 4.17), built through the same [`array_contains`]
/// helper so the two cannot drift.
///
/// An array-overlap operator would state the disjunction in one term instead of
/// one arm per type, but it cannot be written here: `&& ARRAY[$1, $2]` puts the
/// placeholders inside square brackets, and sea-query's tokeniser reads a
/// bracketed span as a quoted identifier and copies it out verbatim, leaving the
/// markers unsubstituted and silently aliased onto other binds. Each arm is
/// still a containment test the `entities.types` GIN index answers, so the cost
/// of the disjunction is an extra index probe per type rather than a scan.
fn type_guard(guard_types: &[AttributeName], linked_alias: &str) -> SimpleExpr {
    let types_column = SeaExpr::col((Alias::new(linked_alias), Entities::Types));
    guard_types
        .iter()
        .fold(Condition::any(), |guard, guard_type| {
            guard.add(array_contains(types_column.clone().into(), SeaExpr::val(guard_type.as_str()).into()))
        })
        .into()
}
