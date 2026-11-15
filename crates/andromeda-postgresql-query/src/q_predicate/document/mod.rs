//! `q` decided over a JSONB document whose members are Properties in the form
//! their writer stored, keyed by expanded name: a registration's Context
//! Source Properties, which a `csf` filter reads (clause 4.9, Table 5.2.9-1).
//!
//! A document addresses each property as a JSON key, so no name resolves
//! against a dictionary, and every operand is bound as a parameter, so the
//! rendering is infallible. A property node is a normalized
//! `{"type": "Property", "value": ...}` or the bare value itself, a
//! sub-property is a member of its property's node, and a compound member a
//! key of the node's value. Each comparison lifts the stored JSON and the
//! operand into one domain before comparing them, so a value of another data
//! type than the operand's compares as unknown, which the asserted test reads
//! as a failure and its complement as a target that does not pass.

pub mod condition;
pub mod property_path;
pub mod written_time_value;
