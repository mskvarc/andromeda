//! The contract between a domain enum and the PostgreSQL enum it is stored as.
//!
//! Five columns in the tenant schema are declared as PostgreSQL enum types, and
//! the store maps each of them through the domain enum's `Display` / `FromStr`
//! rather than through a `sqlx::Type` derive — a derive would put a persistence
//! attribute on a specification type. What replaces the type-level guarantee is
//! the test below, which reads the migrations that declare the labels and
//! asserts that the enum and the schema name exactly the same set.
//!
//! It lives here because this is the only layer that can see both sides. The
//! same assertions used to sit in the model crate against a hand-copied list of
//! labels, which could not fail when the migration changed.

#[cfg(test)]
mod tests {
    use andromeda_ngsi_ld_model::{
        jsonld_context::ContextKind,
        registration::{CsrStatus, ForwardingMode},
        snapshot::SnapshotStatus,
        subscription::SubscriptionStatus,
    };
    use std::{
        fmt::{Debug, Display},
        fs,
        path::PathBuf,
        str::FromStr,
    };
    use strum::IntoEnumIterator;

    /// Every tenant migration, in the order the migrator applies them.
    ///
    /// Read from the directory rather than embedded one file at a time. An
    /// enum's labels are whatever `CREATE TYPE` declares plus whatever any
    /// later migration adds with `ALTER TYPE ... ADD VALUE`, and a reader that
    /// names a single file cannot see the second kind — so it would report a
    /// label as undeclared long after the schema had it.
    fn tenant_migrations() -> Vec<String> {
        let directory = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("migrations/tenant");
        let mut paths: Vec<PathBuf> = fs::read_dir(&directory)
            .unwrap_or_else(|error| unreachable!("{} is readable: {error}", directory.display()))
            .map(|entry| entry.unwrap_or_else(|error| unreachable!("directory entry is readable: {error}")).path())
            .filter(|path| path.extension().is_some_and(|extension| extension == "sql"))
            .collect();
        paths.sort();
        paths
            .iter()
            .map(|path| fs::read_to_string(path).unwrap_or_else(|error| unreachable!("{} is readable: {error}", path.display())))
            .collect()
    }

    /// The labels an enum type carries once every migration has been applied.
    ///
    /// A deliberately small reader rather than a SQL parser: these migrations
    /// are written by hand and the only two shapes they have ever had are the
    /// two matched below.
    fn labels_of(enum_type: &str) -> Vec<String> {
        let mut labels = Vec::new();
        for migration in tenant_migrations() {
            labels.extend(created_labels(&migration, enum_type));
            labels.extend(added_labels(&migration, enum_type));
        }
        labels
    }

    /// The labels one `CREATE TYPE <name> AS ENUM (...)` declares.
    fn created_labels(migration: &str, enum_type: &str) -> Vec<String> {
        let header = format!("CREATE TYPE {enum_type} AS ENUM (");
        let Some(offset) = migration.find(&header) else {
            return Vec::new();
        };
        let body = &migration[offset + header.len()..];
        let end = body.find(");").unwrap_or_else(|| unreachable!("the {enum_type} declaration is terminated"));
        quoted_tokens(&body[..end])
    }

    /// The labels a migration adds with `ALTER TYPE <name> ADD VALUE`.
    ///
    /// Each statement adds exactly one label, optionally behind
    /// `IF NOT EXISTS`, so only the first quoted token of each is a label.
    fn added_labels(migration: &str, enum_type: &str) -> Vec<String> {
        let header = format!("ALTER TYPE {enum_type} ADD VALUE");
        migration
            .match_indices(&header)
            .filter_map(|(offset, _)| {
                let statement = &migration[offset + header.len()..];
                let end = statement.find(';')?;
                quoted_tokens(&statement[..end]).into_iter().next()
            })
            .collect()
    }

    /// Every single-quoted token in a fragment of SQL, in order.
    ///
    /// Splitting on the quote rather than on the comma is what lets one reader
    /// serve both statement shapes: the odd-numbered pieces are exactly the
    /// quoted runs, whatever separates them.
    fn quoted_tokens(fragment: &str) -> Vec<String> {
        fragment.split('\'').skip(1).step_by(2).map(ToOwned::to_owned).collect()
    }

    /// Assert that a domain enum and its PostgreSQL type name the same labels.
    ///
    /// Both directions: a variant with no label would fail in a production
    /// write, and a label with no variant would fail on the read of a row an
    /// older build wrote.
    fn assert_round_trips<T>(enum_type: &str)
    where
        T: IntoEnumIterator + FromStr + Display + PartialEq + Debug,
    {
        let declared = labels_of(enum_type);
        assert!(!declared.is_empty(), "{enum_type} declares no labels");

        let mut rendered_by_variant = Vec::new();
        for variant in T::iter() {
            let rendered = variant.to_string();
            assert!(declared.contains(&rendered), "{enum_type} has no label '{rendered}'; it declares {declared:?}");
            let parsed = T::from_str(&rendered).unwrap_or_else(|_| unreachable!("'{rendered}' parses back"));
            assert_eq!(parsed, variant, "{enum_type} label '{rendered}' does not round-trip");
            rendered_by_variant.push(rendered);
        }

        for label in &declared {
            assert!(
                rendered_by_variant.contains(label),
                "{enum_type} declares '{label}', which no variant renders; a stored row carrying it cannot be read"
            );
        }
    }

    #[test]
    fn subscription_status_matches_the_migration() {
        assert_round_trips::<SubscriptionStatus>("subscription_status");
    }

    #[test]
    fn snapshot_status_matches_the_migration() {
        assert_round_trips::<SnapshotStatus>("snapshot_status");
    }

    #[test]
    fn csr_mode_matches_the_migration() {
        assert_round_trips::<ForwardingMode>("csr_mode");
    }

    #[test]
    fn csr_status_matches_the_migration() {
        assert_round_trips::<CsrStatus>("csr_status");
    }

    #[test]
    fn jsonld_context_kind_matches_the_migration() {
        assert_round_trips::<ContextKind>("jsonld_context_kind");
    }

    #[test]
    fn an_undeclared_label_is_not_silently_accepted() {
        // The guard on the guard: if the reader ever stops finding labels, the
        // assertions above would pass vacuously.
        assert_eq!(labels_of("csr_status"), vec!["ok".to_string(), "failed".to_string()]);
    }

    #[test]
    fn a_label_added_by_a_later_migration_is_read() {
        // The shape `labels_of` exists to catch, exercised against a fixture
        // rather than the tree, because no migration adds a value today and the
        // reader would otherwise be untested until one does.
        let migration = "ALTER TYPE csr_status ADD VALUE IF NOT EXISTS 'degraded';\nALTER TYPE csr_status ADD VALUE 'unknown';\n";
        assert_eq!(added_labels(migration, "csr_status"), vec!["degraded".to_string(), "unknown".to_string()]);
        assert!(added_labels(migration, "snapshot_status").is_empty());
    }
}
