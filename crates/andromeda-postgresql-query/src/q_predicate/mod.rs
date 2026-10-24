//! Compiler from NGSI-LD `q` filter expressions (ETSI GS CIM 009 v1.9.1
//! clause 4.9) to SQL predicates, shared by the entity and temporal builders.
//!
//! # Probes
//!
//! Value predicates compile to an `EXISTS` probe against the `current_attributes` rows, whose typed partial indexes are keyed `(attribute_name_key, value_*)`. The planner scans the typed index, fetches `entity_key` from the heap, and semi-joins back to the driving table.
//!
//! Every probe correlates on `entity_key`, the surrogate the rows are keyed
//! on, so a statement that reaches entities through an alias hands over the
//! [`EntityRowRefs`](crate::entity_row_refs::EntityRowRefs) naming that alias
//! rather than the Entity URI.
//!
//! # What a path addresses
//!
//! A `current_attributes` row is one top-level attribute instance: its value in
//! one typed column, or in `value_object` when it is not a single scalar, and
//! its sub-attributes as a tree of elements in `sub_attributes`, each element
//! holding a value the same way. What a path addresses is decided once, by
//! [`local_target`](crate::q_predicate::local_target::local_target):
//!
//! | Predicate shape | Compiles to |
//! |---|---|
//! | comparison or pattern with no sub-attribute and no value path | a test of the row's value columns, whose ordering, pattern and range tests also read the strings of a `LanguageProperty` row's `languageMap` |
//! | bare existence | the row's presence |
//! | one trailing member (`attr[member]`) | the target language of a `languageMap`, disjoined with the member of a compound value read out of `value_object` |
//! | a deeper value path (`attr[a.b]`) | the member of a compound value read out of `value_object` |
//! | anything carrying a sub-attribute | the same tests on the element at the end of the path, rechecked through the tree inline (see [`sub_attribute`]) |
//! | a path ending on `observedAt`, `unitCode` or a timestamp | the typed field of the instance it hangs off |
//! | attribute IRI absent from `attribute_names` | constant `FALSE` |
//!
//! A temporal `q` reads `attribute_history` rows, which hold an instance the
//! same way, its sub-attributes included, so every path compiles to the same
//! tests over the history row (see
//! [`ProbedRow`](crate::q_predicate::instance_row::ProbedRow)).
//!
//! # Attribute families beyond Property and Relationship
//!
//! A scalar Property value and a Relationship object occupy a typed column each.
//! Every other clause 5.2.4 family lands in `value_object`: a
//! `LanguageProperty`'s `languageMap`, a `ListProperty`'s `valueList`, a
//! `ListRelationship`'s `objectList`, a multi-term `VocabProperty`'s `vocab`,
//! and any Property value that is an array or an object. Clause 4.9 names every
//! one of those a target value, so a Query Term is compared against
//! `value_object` by JSONB containment as well as against the typed columns
//! (see [`structured_target`]). The operators containment cannot express
//! (ordering, pattern and range) read a `languageMap` string by string instead
//! (see [`language_term`]), since clause 4.9 makes each of its strings a String
//! target value.
//!
//! # Deleted attributes
//!
//! No predicate tests for a stored deletion marker. Current state holds no
//! tombstone (the tombstone value kind belongs to history), so an attribute
//! deleted by clause 5.6.16 is an absent row or element, and presence alone
//! answers an existence test.
//!
//! # Linked-entity traversal
//!
//! The clause 4.9 `{}` axis (`rel{attr}=="x"`) compiles to one correlated
//! `EXISTS` per hop. Each probe reads the Relationship's instances, expands
//! every object they hold (the single one in `value_uri` or `value_long_uri`,
//! several in `value_object`), joins each to `entities.id` to reach the entity
//! it names, and compiles the rest of the predicate against that entity's
//! `entity_key`, recursively, so a chain of hops nests a chain of probes.
//! Aliases are suffixed by depth (`ca_ln1`/`lo1`/`le1`, `ca_ln2`/`lo2`/`le2`,
//! and so on) so a nested probe
//! cannot shadow its enclosing one. A hop's optional entity-type guard becomes
//! a disjunction of membership tests against the linked entity's `types`
//! column, and a Relationship name absent from `attribute_names` compiles to
//! constant `FALSE` exactly as any other unknown name does.
//!
//! # Temporal `q`
//!
//! A temporal query's `q` (clause 5.7.4.4 step S2) is "checked against all the
//! Attribute instances resulting from the initial filtering performed by the
//! temporal query", not against current state. The same comparisons then run
//! over the in-window `attribute_history` rows instead: the two relations hold
//! an instance's value in the same typed columns, and an
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
//! The Query Terms of a disjunction that each read one attribute's rows share
//! one probe, each arm keeping its attribute key beside its value test (see
//! [`instance_test`]). A disjunction of separate `EXISTS` can only be evaluated
//! as a filter on every entity row; one `EXISTS` is a semi-join the planner
//! drives from a `BitmapOr` over every arm's value index. Hops and sub-attribute
//! probes read other rows, so they stay beside the shared probe.
//!
//! # Multi-instance semantics
//!
//! `EXISTS` matches when *any* row for the attribute satisfies the predicate.
//! That is what clause 4.9 requires (p. 86: "the target value shall be any
//! Value of such instances").

pub mod attribute_names;
pub mod equality_term;
pub mod extended_filter_pattern;
pub mod history_aliases;
pub mod history_window;
pub mod in_effect_instance;
pub mod instance_row;
pub mod instance_scan;
pub mod instance_test;
pub mod json_value_term;
pub mod keyed_branches;
pub mod keyed_existence;
pub mod keyed_hop;
pub mod keyed_ordering;
pub mod keyed_pattern;
pub mod keyed_term;
pub mod language_map;
pub mod language_term;
pub mod local_target;
pub mod ordering_term;
pub mod own_value_arms;
pub mod pattern_term;
pub mod projection;
pub mod projection_probe;
pub mod string_ordering;
pub mod structured_target;
pub mod sub_attribute;
pub mod traversal;
pub mod typed_column;
pub mod value_column;
pub mod value_object_member;
