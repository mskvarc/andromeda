//! FW area — distributed framework and the four registration modes
//! (ETSI GS CIM 009 v1.9.1, clauses 4.3.6, 4.5.4-4.5.5, 5.7, 5.11, 5.16, 6.3.15-6.3.18).
//!
//! Broker-side enforcement of registration constraints on forwarded requests,
//! the inclusive/exclusive/redirect/auxiliary distribution modes, cascade
//! limiting via the `Via` header, `contextSourceInfo` forwarding, backwards
//! compatible version negotiation, local scope, linked entity maps, and
//! snapshot locality. Concrete per-test designs live in
//! `docs/distributed-tests/fw.md`.
//!
//! Real peers (`pair`/`chain`) exercise end-to-end federation semantics; the
//! in-process mock Context Source (`ContextSourceAdapter`) captures the exact
//! forwarded request for byte-level oracles and injects faults.

mod backwards_compat;
mod backwards_compat_fallback;
mod cascade_loop;
mod context_source_info;
mod context_source_info_jsonld;
mod entity_map_split;
mod entity_map_unitary;
mod forward_constraints;
mod local_scope;
mod modes_auxiliary;
mod modes_exclusive;
mod modes_inclusive;
mod modes_redirect;
mod snapshots;
