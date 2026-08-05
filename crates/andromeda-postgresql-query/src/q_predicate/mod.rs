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
//! Predicates now compile to an `EXISTS` probe against the `attribute_current`
//! projection, whose typed partial indexes are keyed
//! `(attribute_name_id, value_*)`. The planner scans the typed index, fetches
//! `entity_id` from the heap, and semi-joins back to the driving table — the
//! access path migration 0025 was shaped for.
//!
//! # Eligibility
//!
//! Not every predicate can use the projection. `attribute_current` stores one
//! typed value per attribute instance; it does not store sub-attributes or the
//! interior of compound values. So:
//!
//! | Predicate shape | Compiles to |
//! |---|---|
//! | comparison or pattern with no sub-attribute and no value path | `EXISTS` on `attribute_current` |
//! | bare existence | `EXISTS` on `temporal_entity_attributes`, tombstone-checked |
//! | anything carrying a sub-attribute or value path | JSONB jsonpath fallback |
//! | attribute IRI absent from `attribute_names` | constant `FALSE` |
//!
//! The fallback keeps the previous semantics for the shapes the projection
//! cannot answer. It is still unindexed; it is now the exception rather than
//! the rule.
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
