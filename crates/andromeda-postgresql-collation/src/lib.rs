//! The ICU collation objects one tenant's PostgreSQL database holds.
//!
//! Clause 4.23.1 lets a request name any ICU collation, and clause 4.23.3's own
//! examples use the keyword forms IETF RFC 6067 defines (`und-u-ks-identic`,
//! `de-u-co-phonebk`). PostgreSQL pre-imports only one object per ICU locale,
//! named `<locale>-x-icu`, so a keyword form has no object until one is created
//! and `ORDER BY … COLLATE "de-u-co-phonebk"` fails the statement outright.
//!
//! Provisioning belongs to neither store that needs it. The entity query orders
//! by any member (clause 4.23.3) and the temporal query orders by `id` (clause
//! 5.7.4.3); both sort under a client collation, both run on one tenant
//! database, and whichever of them is asked first is the one that has to create
//! the object. The concern is therefore owned here, once, rather than copied
//! into each store. Each store keeps its own record of what it has created, so
//! a collation both of them order by is created once by each; the statement is
//! `IF NOT EXISTS`, so the second create is a no-op. Two creates that run
//! concurrently both pass that check, and the later one is refused on the
//! catalog's unique index; that refusal means the object exists, so it counts
//! as created.
//!
//! Whether the locale names a collation ICU holds is decided here too, and only
//! here. ICU's set of known languages is its own, so no copy of it kept in the
//! broker process would agree with it; the statement is therefore issued under
//! `icu_validation_level = error`, which refuses an unknown locale instead of
//! creating an object that silently sorts by the root collation, and the
//! refusal is reported as the `BadRequestData` clauses 5.7.2.4 and 5.7.4.4 ask
//! for.

pub mod provisioned_collations;

#[cfg(test)]
andromeda_architecture_checks::dependency_boundary! {
    allow_workspace_dependencies: [
        "andromeda-ngsi-ld-query",
        "andromeda-postgresql-pool",
        "andromeda-postgresql-seaquery",
    ],
    allow_workspace_dev_dependencies: [
        "andromeda-postgresql-pool",
    ],
    allow_production_dependencies: ["sqlx"],
}
