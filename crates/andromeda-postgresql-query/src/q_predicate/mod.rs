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
//! `(attribute_name_key, value_*)`. The planner scans the typed index, fetches
//! `entity_key` from the heap, and semi-joins back to the driving table.
//!
//! Every probe correlates on `entity_key`, the surrogate the projection is keyed
//! on, so a statement that reaches entities through an alias hands over the
//! [`EntityRowRefs`](crate::entity_row_refs::EntityRowRefs) naming that alias
//! rather than the Entity URI.
//!
//! # Eligibility
//!
//! Not every predicate can use the projection. `current_attribute_values` stores
//! one value per attribute instance; it does not store sub-attributes or the
//! interior of compound values. What a path addresses is decided once, by
//! [`local_target`](crate::q_predicate::local_target::local_target):
//!
//! | Predicate shape | Compiles to |
//! |---|---|
//! | comparison or pattern with no sub-attribute and no value path | `EXISTS` on `current_attribute_values` |
//! | bare existence | `EXISTS` on `current_attribute_values`, presence alone |
//! | equality or existence with one trailing member (`attr[member]`) | both: a `languageMap` probe on the projection, disjoined with the jsonpath fallback |
//! | anything carrying a sub-attribute or a deeper value path | JSONB jsonpath fallback |
//! | ordering or pattern with any value path | JSONB jsonpath fallback |
//! | attribute IRI absent from `attribute_names` | constant `FALSE` |
//!
//! The fallback keeps the previous semantics for the shapes the projection
//! cannot answer. It is still unindexed; it is now the exception rather than
//! the rule.
//!
//! # Attribute families beyond Property and Relationship
//!
//! A scalar Property value and a Relationship object occupy a typed column each.
//! Every other clause 5.2.4 family lands in `value_object`: a
//! `LanguageProperty`'s `languageMap`, a `ListProperty`'s `valueList`, a
//! `ListRelationship`'s `objectList`, a multi-term `VocabProperty`'s `vocab`,
//! and any Property value that is an array or an object. Clause 4.9 names every
//! one of those a target value, so a Query Term is compared against
//! `value_object` by JSONB containment as well as against the typed columns —
//! see [`structured_target`](crate::q_predicate::structured_target).
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
//! probe on the `(attribute_name_key, value_uri)` index. A hop's optional
//! entity-type guard becomes a disjunction of membership tests against the
//! linked entity's `types` column, and a Relationship name absent from
//! `attribute_names` compiles to constant `FALSE` exactly as any other unknown
//! name does.
//!
//! # Temporal `q`
//!
//! A temporal query's `q` (clause 5.7.4.4 step S2) is "checked against all the
//! Attribute instances resulting from the initial filtering performed by the
//! temporal query", not against current state. The same comparisons then run
//! over the in-window `attribute_history` rows instead of the projection: the
//! two relations hold an instance's value in the same typed columns, and an
//! [`InstanceScan`](crate::q_predicate::instance_scan::InstanceScan) says which
//! one a compiled `q` reads. Each Query Term is its own existential over the
//! instances of its attribute valid within the window on the requested
//! Temporal Property, of every `datasetId` (step S8 comes later), and the
//! AND/OR tree combines them. The valid instances are read by two probes, the
//! instances inside the window and the instance in effect when it opens (see
//! [`history_window`]).
//!
//! # Disjunctions
//!
//! The Query Terms of a disjunction that each read one attribute's instances
//! share one probe, each arm keeping its attribute key beside its value test
//! (see [`instance_test`]). A disjunction of separate `EXISTS` can only be
//! evaluated as a filter on every entity row; one `EXISTS` is a semi-join the
//! planner drives from a `BitmapOr` over every arm's value index. Hops and
//! payload readings read other rows, so they stay beside the shared probe.
//!
//! # Multi-instance semantics
//!
//! `EXISTS` matches when *any* row for the attribute satisfies the predicate.
//! That is what clause 4.9 requires (p. 86: "the target value shall be any
//! Value of such instances"). The previous `jsonb_path_query_first` shape only
//! ever examined the first instance, which was a spec bug.

pub mod attribute_names;
pub mod fallback;
pub mod history_instance_document;
pub mod history_window;
pub mod in_effect_instance;
pub mod instance_scan;
pub mod instance_test;
pub mod local_target;
pub mod projection;
pub mod projection_probe;
pub mod structured_target;
pub mod traversal;
pub mod typed_column;
pub mod value_column;
