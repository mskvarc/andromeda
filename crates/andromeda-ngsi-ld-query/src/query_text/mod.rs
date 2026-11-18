//! The text of a query-language parameter as the parsers read it.
//!
//! The module owns the fault a parser carries while it reads, the whitespace sets the languages skip, and the productions two or more languages share.

pub(crate) mod disjunctive_form;
pub(crate) mod grammar_fault;
pub(crate) mod or_operator;
pub(crate) mod whitespace;
