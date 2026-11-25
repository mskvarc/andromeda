//! The tenant registry of a platform database, read by this build's registry:
//! the tenants v1.0.0 registered keep the database names it stored, a tenant
//! registered afterwards is named by this build, and a new tenant's database
//! is a copy of the tenant template the registry prepares when it starts. A
//! tenant opened registers each column history skips chunks on exactly once.

mod chunk_skipping_registration;
mod stored_database_names;
mod template_copies;
mod template_lifecycle;
