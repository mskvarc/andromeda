//! PROV area — provision forwarding across Context Source Registrations
//! (ETSI GS CIM 009 v1.9.1, clauses 5.6.1–5.6.21).
//!
//! Every provision operation issued to broker A that matches a registration is
//! split into a locally handled remainder and one forwarded copy per matching
//! Context Source. Exclusive and redirect registrations own their attributes and
//! strip them from the local fragment; inclusive registrations forward a copy and
//! also keep the local write; an operation the registration does not advertise
//! yields `409 Conflict` (bare when it is the only work, keyed by
//! `registrationId` inside a `207 Multi-Status` when other work participates).
//!
//! `T-MOCK`/`T-MOCK-N` rows register an in-process [`ContextSourceAdapter`] as the
//! peer and assert the exact forwarded body, headers and path counts; `T-PAIR`
//! rows register real broker B and assert the end-to-end split by reading each
//! broker locally.

mod append;
mod batch_create;
mod batch_delete;
mod batch_merge;
mod batch_update;
mod batch_upsert;
mod create;
mod delete_attr;
mod delete_entity;
mod forwarding;
mod merge;
mod partial_update;
mod purge;
mod purge_entity_map;
mod replace_attr;
mod replace_entity;
mod temporal_attr;
mod temporal_evolution;
mod temporal_payloads;
mod temporal_upsert;
mod update_attrs;
