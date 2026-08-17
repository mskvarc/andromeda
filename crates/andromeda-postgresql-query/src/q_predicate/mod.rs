//! Compiler from NGSI-LD `q` filter expressions (ETSI GS CIM 009 v1.9.1
//! clause 4.9) to SQL predicates, shared by the entity and temporal builders.
//!
//! # Why this exists
//!
//! Value predicates used to compile to
//! `jsonb_path_query_first(payload, '$[0]."<iri>"[0]."hasValue"[0]."@value"')`
//! with a text cast. That is a functional expression over a JSONB column: no
//! index can serve it, so every filtered query degraded to a sequential scan
//! over `entities`.
//!
//! Predicates now compile to an `EXISTS` probe against the
//! `current_attribute_values` projection, whose typed partial indexes are keyed
//! `(attribute_name_id, value_*)`. The planner scans the typed index, fetches
//! `entity_key` from the heap, and semi-joins back to the driving table.
//!
//! Every probe correlates on `entity_key`, the surrogate the projection is keyed
//! on, so a statement that reaches entities through an alias hands over that
//! alias's `entity_key` column rather than the Entity URI.
//!
//! # Eligibility
//!
//! Not every predicate can use the projection. `current_attribute_values` stores
//! one typed value per attribute instance; it does not store sub-attributes or
//! the interior of compound values. So:
//!
//! | Predicate shape | Compiles to |
//! |---|---|
//! | comparison or pattern with no sub-attribute and no value path | `EXISTS` on `current_attribute_values` |
//! | bare existence | `EXISTS` on `current_attribute_values`, presence alone |
//! | anything carrying a sub-attribute or value path | JSONB jsonpath fallback |
//! | attribute IRI absent from `attribute_names` | constant `FALSE` |
//!
//! The fallback keeps the previous semantics for the shapes the projection
//! cannot answer. It is still unindexed; it is now the exception rather than
//! the rule.
//!
//! # Deleted attributes
//!
//! No predicate tests for a stored deletion marker. The projection holds no
//! tombstone — the tombstone value kind belongs to history — so an attribute
//! deleted by clause 5.6.16 is an absent row, and row presence alone answers an
//! existence test.
//!
//! # Linked-entity traversal
//!
//! The clause 4.9 `{}` axis (`rel{attr=="x"}`) compiles to one correlated
//! `EXISTS` per hop. Each probe reads the Relationship's instances from
//! `current_attribute_values`, joins `value_uri` to `entities.id` to reach the
//! entity each instance names, and compiles the rest of the predicate against
//! that entity's `entity_key` — recursively, so a chain of hops nests a chain of
//! probes. Aliases are suffixed by depth (`cav_ln1`/`le1`, `cav_ln2`/`le2`, …)
//! so a nested probe cannot shadow its enclosing one.
//!
//! Reading the object from the projection rather than the payload keeps the
//! probe on the `(attribute_name_id, value_uri)` index. A hop's optional
//! entity-type guard becomes a disjunction of membership tests against the
//! linked entity's `types` column, and a Relationship name absent from
//! `attribute_names` compiles to constant `FALSE` exactly as any other unknown
//! name does.
//!
//! # Multi-instance semantics
//!
//! `EXISTS` matches when *any* row for the attribute satisfies the predicate.
//! That is what clause 4.9 requires (p. 86: "the target value shall be any
//! Value of such instances"). The previous `jsonb_path_query_first` shape only
//! ever examined the first instance, which was a spec bug.

pub mod attribute_names;
pub mod fallback;
pub mod projection;
pub mod target;
pub mod traversal;
