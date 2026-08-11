//! The clause 4.9 linked-entity traversal axis compiled to a correlated probe.
//!
//! `rel{attr=="x"}` asks about an attribute of the entity a Relationship points
//! at, so answering it means leaving the row being filtered. One hop becomes one
//! `EXISTS`: the Relationship's instances are read from `attribute_current`,
//! `value_uri` is joined to `entities.id` to reach the entity each instance
//! names, and the rest of the predicate is compiled against that entity. Hops
//! nest, so the probes nest with them.
//!
//! This lives apart from the comparison rendering next door because it changes
//! *which row* is being asked about rather than *what is asked* of it — the two
//! change for unrelated reasons.

use crate::{attribute_name_id::AttributeNameId, operator::any_eq, q_predicate::target::QFilterTarget};
use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
use andromeda_ngsi_ld_vocabulary::expanded::properties::NULL_URI;
use sea_query::{Alias, Condition, Expr as SeaExpr, SimpleExpr};

/// The `entities` column a hop joins the Relationship's object to.
const LINKED_ENTITY_ID: &str = "id";

/// The `entities` column the inner predicate's JSONB fallback reads.
const LINKED_ENTITY_PAYLOAD: &str = "payload";

/// The `entities` column a hop's type guard tests, a `text[]` of expanded type
/// IRIs (migration `0005_entities.sql`).
const LINKED_ENTITY_TYPES: &str = "types";

/// One hop of a traversal: the relations its probe introduces, and the probe
/// itself.
///
/// The aliases are suffixed by the hop's depth so a nested traversal's probe
/// cannot shadow the one enclosing it, and they are decided once here rather
/// than by the caller, because the caller needs the *same* aliases twice — once
/// to compile the inner predicate against the entity this hop lands on, and
/// again to render the probe around it.
pub(crate) struct LinkedHop {
    /// Alias of this hop's `attribute_current` scan.
    attribute_alias: String,
    /// Alias of the `entities` row this hop lands on.
    linked_alias: String,
}

impl LinkedHop {
    /// The hop `depth` levels into a traversal, counting from one.
    pub(crate) fn at_depth(depth: usize) -> LinkedHop {
        LinkedHop {
            attribute_alias: format!("ac_ln{depth}"),
            linked_alias: format!("le{depth}"),
        }
    }

    /// Where a predicate compiled *inside* this hop reads its entity.
    pub(crate) fn inner_target(&self) -> QFilterTarget {
        QFilterTarget::correlated(
            (Alias::new(self.linked_alias.as_str()), Alias::new(LINKED_ENTITY_ID)),
            (Alias::new(self.linked_alias.as_str()), Alias::new(LINKED_ENTITY_PAYLOAD)),
        )
    }

    /// Wrap an already-compiled inner predicate in this hop's probe.
    ///
    /// The probe is an `EXISTS` over the Relationship's instances in
    /// `attribute_current`, joined to the entity each one names, correlated
    /// against `outer`. Reading the object from `attribute_current.value_uri`
    /// rather than the payload keeps the probe on the
    /// `(attribute_name_id, value_uri)` index, and the join to `entities.id` is
    /// the primary key.
    ///
    /// A tombstoned Relationship — whose object was replaced by
    /// `urn:ngsi-ld:null` — names no entity, so it is excluded before the join
    /// rather than left to fail it.
    pub(crate) fn probe(&self, relationship_id: AttributeNameId, guard_types: &[AttributeName], outer: &QFilterTarget, inner: Condition) -> Condition {
        let attribute_alias = self.attribute_alias.as_str();
        let linked_alias = self.linked_alias.as_str();
        // The markers are positional over the expressions collected below, not bind
        // placeholders: sea-query renders each expression in turn, so the guard and
        // the inner condition contribute their own binds where they are written.
        //
        // Both substituted conditions are parenthesised. A multi-type guard renders
        // as a bare `a OR b`, and spliced unparenthesised into this `AND` chain its
        // second disjunct would escape the correlation, the attribute-name test and
        // the tombstone guard entirely — the probe would match whenever *any* entity
        // in the tenant carried the second type. The inner condition is wrapped for
        // the same reason, before some future compilation of it renders a top-level
        // `OR` too.
        let mut operands: Vec<SimpleExpr> = vec![SeaExpr::val(relationship_id.get()).into()];
        let guard = if guard_types.is_empty() {
            String::new()
        } else {
            operands.push(type_guard(guard_types, linked_alias));
            format!(" AND (${})", operands.len())
        };
        let inner_marker = format!("(${})", operands.len() + 1);
        operands.push(inner.into());

        let sql = format!(
            "EXISTS (SELECT 1 FROM \"attribute_current\" AS \"{attribute_alias}\" \
             JOIN \"entities\" AS \"{linked_alias}\" ON \"{linked_alias}\".\"{LINKED_ENTITY_ID}\" = \"{attribute_alias}\".\"value_uri\" \
             WHERE \"{attribute_alias}\".\"entity_id\" = {corr} \
             AND \"{attribute_alias}\".\"attribute_name_id\" = $1 \
             AND \"{attribute_alias}\".\"value_uri\" IS DISTINCT FROM '{null_uri}'{guard} \
             AND {inner_marker})",
            corr = outer.entity_id_column(),
            null_uri = NULL_URI.as_str(),
        );
        Condition::all().add(SeaExpr::cust_with_exprs(sql, operands))
    }
}

/// The hop's entity-type constraint against the linked entity's `types` column.
///
/// Clause 4.9 lists the guard's types as alternatives, so the constraint is a
/// disjunction of array-membership tests — the same leaf shape a `type`
/// selection renders (clause 4.17), built through the same [`any_eq`] helper so
/// the two cannot drift.
///
/// An array-overlap operator would state the disjunction in one term, but it
/// cannot be written here: `&& ARRAY[$1, $2]` puts the placeholders inside
/// square brackets, and sea-query's tokeniser reads a bracketed span as a
/// quoted identifier and copies it out verbatim, leaving the markers
/// unsubstituted and silently aliased onto other binds. The cost of the
/// disjunction is that it cannot use the GIN index on `entities.types` the way
/// `&&` would: each arm is a separate membership test, so a guarded hop filters
/// the rows the join already produced rather than being driven by that index.
fn type_guard(guard_types: &[AttributeName], linked_alias: &str) -> SimpleExpr {
    let types_column = SeaExpr::col((Alias::new(linked_alias), Alias::new(LINKED_ENTITY_TYPES)));
    guard_types
        .iter()
        .fold(Condition::any(), |guard, guard_type| {
            guard.add(any_eq(SeaExpr::val(guard_type.as_str()).into(), types_column.clone().into()))
        })
        .into()
}
