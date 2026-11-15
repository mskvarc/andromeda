//! What a `q` filter means, with every clause 4.9 decision taken.
//!
//! A parsed filter is text made into a tree: it keeps the operator, the operand
//! and the path the client wrote, so it can be stored, forwarded and written
//! back. Its reading is what an execution needs instead. Lowering takes each
//! decision of clause 4.9 once (which instance member a path ends on, which
//! families a trailing group reads as a language, that `!=` is the complement
//! of `==` over every kind), and what a target of each kind makes of a term
//! derives from the term's test and polarity, so the in-memory evaluator and
//! the SQL rendering read one meaning and take none of their own. A reading
//! names no database and no entity model.

pub mod array_rule;
pub mod array_target;
pub mod comparator;
#[cfg(test)]
mod decision_table_tests;
pub mod family_reading;
pub mod filter_reading;
pub mod hop;
pub mod interval;
pub mod lowering;
#[cfg(test)]
mod lowering_tests;
pub mod operand;
pub mod order_relation;
pub mod scalar_test;
pub mod target_kind;
pub mod term_reading;
pub mod terminal;
pub mod uri_operand;
pub mod value_test;
pub mod verdict;
