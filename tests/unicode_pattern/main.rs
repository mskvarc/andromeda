//! Unicode-aware regular expressions (clause 4.9 `RegExp`), end to end.
//!
//! Clause 4.9 hands `RegExp` to IEEE 1003.2, and the broker admits two
//! Unicode-aware extensions beside it. The case-insensitive option -- set for
//! the rest of a pattern as `(?i)`, scoped to a group as `(?i:…)`, cleared again
//! as `(?-i)` -- folds case by Unicode simple case folding, so a Slovenian,
//! Czech, German or Greek name compares case-insensitively exactly as an ASCII
//! one does. The shorthand classes `\d`, `\w` and `\s` and their negations
//! classify by Unicode, so `\w` holds `č` as it holds `c`.
//!
//! The broker evaluates a pattern with two engines: the `regex` crate in memory
//! (subscriptions, residual filters) and PostgreSQL in the store (queries,
//! discovery, forwarding coverage). PostgreSQL's own `(?i)` folds and its own
//! shorthand classifies only ASCII under the `COLLATE "C"` every stored value
//! carries, so the store is never handed the client's text: it evaluates a
//! restatement with every fold and every class written out as ranges, which a
//! registration persists beside its `information`. This suite pins that the two
//! engines agree character for character, and that every place the store
//! evaluates a pattern evaluates the restatement:
//!
//! - `engine_agreement`: both engines over a corpus of admitted patterns, over
//!   every letter of the Latin, Greek and Cyrillic blocks under `(?i)`, and over
//!   every code point under each shorthand class, with controls showing what
//!   PostgreSQL makes of the client's text itself.
//! - `entity_query`: `q` and `idPattern` on the Core API.
//! - `temporal_query`: the same on the Temporal API.
//! - `subscription`: the in-memory engine answering a subscription's `q` as the
//!   store answers the query carrying it.
//! - `registration`: a registered `idPattern` in discovery and in forwarding,
//!   the restatement the registration stores, and its refresh after a
//!   rendering change.
//! - `refusal`: the options that stay refused, the repetition bound, and a
//!   shorthand class repeated past the compiled size.
//!
//! Naming convention: `unicode_pattern_<area>_<scenario>`.

#![allow(clippy::expect_used, clippy::unwrap_used, reason = "integration test setup uses explicit assertions")]

mod engine_agreement;
mod entity_query;
mod fixture;
mod refusal;
mod registration;
mod subscription;
mod temporal_query;
