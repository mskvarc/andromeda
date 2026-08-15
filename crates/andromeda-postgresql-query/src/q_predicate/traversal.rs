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

use crate::{attribute_name_id::AttributeNameId, q_predicate::target::QFilterTarget};
use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
use andromeda_postgresql_schema::current::{CurrentAttributeValues, Entities};
use andromeda_postgresql_seaquery::{literal::int_literal, operator::array_contains};
use sea_query::{Alias, Condition, Expr as SeaExpr, ExprTrait, JoinType, Query, SimpleExpr};

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
        // Fresh alias handles rather than clones: each names the same relation
        // the hop introduced.
        let hop = || Alias::new(self.attribute_alias.as_str());
        let linked = || Alias::new(self.linked_alias.as_str());

        // The Relationship's object is the linked Entity's URI (clause 4.5.3), so
        // the projection's `value_uri` joins to `entities.id`.
        let join_on = SeaExpr::col((linked(), Entities::Id)).equals((hop(), CurrentAttributeValues::ValueUri));

        let mut probe = Query::select();
        probe
            .expr(int_literal(1))
            .from_as(CurrentAttributeValues::Table, hop())
            .join_as(JoinType::Join, Entities::Table, linked(), join_on)
            // The correlation is added first — it binds nothing, so the
            // Relationship's dictionary key is still the probe's first bound
            // value, ahead of the guard's types and the inner predicate's operands.
            .and_where(SeaExpr::col((hop(), CurrentAttributeValues::EntityKey)).eq(outer.entity_key_column()))
            .and_where(SeaExpr::col((hop(), CurrentAttributeValues::AttributeNameId)).eq(relationship_id.get()));
        // A multi-type guard is a disjunction; adding it as one condition keeps it
        // parenthesised so a second alternative cannot escape the correlation and
        // the attribute-name test to match any entity in the tenant that carries it.
        if !guard_types.is_empty() {
            probe.and_where(type_guard(guard_types, self.linked_alias.as_str()));
        }
        // The inner predicate is added as its own condition for the same reason,
        // before any future compilation of it renders a top-level `OR`.
        probe.cond_where(inner);
        Condition::all().add(SeaExpr::exists(probe))
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
            guard.add(array_contains(types_column.clone(), SeaExpr::val(guard_type.as_str())))
        })
        .into()
}
