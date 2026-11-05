//! Which tenants behind the server's extension versions a wake pass loads, and
//! in what order.
//!
//! A tenant is owed a load when one of its extensions is short of the version
//! the server ships and its last open did not already try against that
//! version. An installed extension is short when it is below the shipped
//! version; an absent one only when the open installs it (`pg_repack`), not
//! when the migrations create it. One attempt per server version keeps a
//! tenant whose role may not update or install an extension from being loaded
//! on every pass; a newer server version earns it another.
//!
//! The order is furthest behind first. A tenant no open has recorded yet comes
//! before every recorded one: each recorded version was written by a broker
//! that keeps this record, so an unrecorded tenant was last opened before such
//! a broker ran, and is at least as far behind. Among recorded tenants the
//! older `TimescaleDB` goes first, since its library is the one a later image
//! stops shipping, which leaves the tenant unopenable; then the older
//! `PostGIS`; then the older `pg_repack`, an absent one first; then the tenant
//! with more committed transactions, whose first request is the likelier to
//! come soon.

use crate::{
    recreated_extension::RecreatedExtension,
    schema_contract::{
        server_requirements::{ExtensionPresence, ExtensionVersion},
        versioned_extension::VersionedExtension,
    },
    tracked_extension::{AbsentInstall, TrackedExtension},
};
use andromeda_ngsi_ld_lexicon::tenant::TenantName;
use std::cmp::Reverse;

/// What the record holds of one extension in one tenant database.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct RecordedExtension {
    /// What the database held after its last open.
    pub(super) installed: ExtensionPresence,
    /// The version the server shipped at that open, `None` when it shipped
    /// none.
    pub(super) reconciled_against: Option<ExtensionVersion>,
}

/// One extension of one tenant database, against what the server ships now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct ExtensionLag {
    /// The record of the tenant's last open, `None` when no open has written
    /// one yet.
    pub(super) recorded: Option<RecordedExtension>,
    /// The version the server ships now, `None` when it ships none.
    pub(super) shipped: Option<ExtensionVersion>,
}

impl ExtensionLag {
    /// Whether an open is owed to bring this extension to the shipped version,
    /// where `absent_install` says what creates it while it is absent.
    ///
    /// An extension the server ships none of has nothing to reach. One the
    /// database does not hold is owed an open only when the open installs it,
    /// once per shipped version; the migrations create the others. An
    /// unrecorded extension may be anything, and is owed the open that finds
    /// out.
    fn owes_attempt(self, absent_install: AbsentInstall) -> bool {
        match (self.recorded, self.shipped) {
            (_, None) => false,
            (None, Some(_)) => true,
            (
                Some(RecordedExtension {
                    installed: ExtensionPresence::Absent,
                    reconciled_against,
                }),
                Some(shipped),
            ) => match absent_install {
                AbsentInstall::CreatedByMigrations => false,
                AbsentInstall::InstalledByOpen => reconciled_against != Some(shipped),
            },
            (
                Some(RecordedExtension {
                    installed: ExtensionPresence::Installed(installed),
                    reconciled_against,
                }),
                Some(shipped),
            ) => installed < shipped && reconciled_against != Some(shipped),
        }
    }

    /// The installed version the order compares, `None` when not recorded as
    /// installed, which sorts first.
    const fn installed(self) -> Option<ExtensionVersion> {
        match self.recorded {
            Some(RecordedExtension {
                installed: ExtensionPresence::Installed(version),
                ..
            }) => Some(version),
            Some(RecordedExtension {
                installed: ExtensionPresence::Absent,
                ..
            })
            | None => None,
        }
    }
}

/// One active named tenant with no wake pending, as the platform database
/// sees its extensions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct CatchUpCandidate {
    /// The tenant.
    pub(super) name: TenantName,
    /// Its `TimescaleDB`.
    pub(super) timescaledb: ExtensionLag,
    /// Its `PostGIS`.
    pub(super) postgis: ExtensionLag,
    /// Its `pg_repack`.
    pub(super) pg_repack: ExtensionLag,
    /// The transactions its database committed, as `pg_stat_database` counts
    /// them, zero when the statistics hold no row for it.
    pub(super) committed_transactions: i64,
}

/// Whether a tenant has any record yet. Declared in sort order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Recording {
    /// Some extension has no row: no open of a broker that keeps the record
    /// has written one.
    Unrecorded,
    /// Every extension has a row.
    Recorded,
}

/// The key a candidate sorts by. Field order is the comparison order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct CatchUpRank {
    /// Unrecorded tenants first.
    recording: Recording,
    /// The older `TimescaleDB` first.
    timescaledb: Option<ExtensionVersion>,
    /// Then the older `PostGIS`.
    postgis: Option<ExtensionVersion>,
    /// Then the older `pg_repack`.
    pg_repack: Option<ExtensionVersion>,
    /// Then the busier database.
    committed_transactions: Reverse<i64>,
}

impl CatchUpCandidate {
    /// Each extension's lag beside the extension it is the lag of.
    const fn lags(&self) -> [(TrackedExtension, ExtensionLag); 3] {
        [
            (TrackedExtension::Updated(VersionedExtension::TimescaleDb), self.timescaledb),
            (TrackedExtension::Updated(VersionedExtension::PostGis), self.postgis),
            (TrackedExtension::Recreated(RecreatedExtension::PgRepack), self.pg_repack),
        ]
    }

    /// Whether an open is owed to bring any of its extensions to the shipped
    /// version.
    fn owes_attempt(&self) -> bool {
        self.lags().into_iter().any(|(extension, lag)| lag.owes_attempt(extension.absent_install()))
    }

    /// Where the candidate sorts.
    fn rank(&self) -> CatchUpRank {
        let recording = if self.lags().iter().all(|(_, lag)| lag.recorded.is_some()) {
            Recording::Recorded
        } else {
            Recording::Unrecorded
        };
        CatchUpRank {
            recording,
            timescaledb: self.timescaledb.installed(),
            postgis: self.postgis.installed(),
            pg_repack: self.pg_repack.installed(),
            committed_transactions: Reverse(self.committed_transactions),
        }
    }
}

/// The candidates owed a load, furthest behind first.
pub(super) fn catch_up_order(candidates: Vec<CatchUpCandidate>) -> Vec<TenantName> {
    let mut owed: Vec<CatchUpCandidate> = candidates.into_iter().filter(CatchUpCandidate::owes_attempt).collect();
    owed.sort_by_key(CatchUpCandidate::rank);
    owed.into_iter().map(|candidate| candidate.name).collect()
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::{
        schema_contract::server_requirements::{ExtensionPresence, ExtensionVersion},
        tenant_extension_versions::catch_up_rank::{CatchUpCandidate, ExtensionLag, RecordedExtension, catch_up_order},
    };
    use andromeda_ngsi_ld_lexicon::tenant::TenantName;
    use pretty_assertions::assert_eq;

    const TIMESCALEDB_SHIPPED: ExtensionVersion = ExtensionVersion::new(2, 31, 0);
    const POSTGIS_SHIPPED: ExtensionVersion = ExtensionVersion::new(3, 6, 0);
    const PG_REPACK_SHIPPED: ExtensionVersion = ExtensionVersion::new(1, 5, 3);

    /// An extension recorded at `installed` by an open that ran against
    /// `reconciled_against`, while the server ships `shipped`.
    const fn recorded(installed: ExtensionVersion, reconciled_against: ExtensionVersion, shipped: ExtensionVersion) -> ExtensionLag {
        ExtensionLag {
            recorded: Some(RecordedExtension {
                installed: ExtensionPresence::Installed(installed),
                reconciled_against: Some(reconciled_against),
            }),
            shipped: Some(shipped),
        }
    }

    /// An extension recorded at the version an older server shipped, by an
    /// open that ran on that server.
    const fn behind(installed: ExtensionVersion, shipped: ExtensionVersion) -> ExtensionLag {
        recorded(installed, installed, shipped)
    }

    /// An extension recorded at the version the server ships.
    const fn current(shipped: ExtensionVersion) -> ExtensionLag {
        recorded(shipped, shipped, shipped)
    }

    fn candidate(name: &str, timescaledb: ExtensionLag, postgis: ExtensionLag, committed_transactions: i64) -> CatchUpCandidate {
        CatchUpCandidate {
            name: TenantName::try_from(name.to_owned()).expect("a valid tenant name"),
            timescaledb,
            postgis,
            pg_repack: current(PG_REPACK_SHIPPED),
            committed_transactions,
        }
    }

    /// `pg_repack` recorded absent by an open that ran against
    /// `reconciled_against`, while the server ships it.
    const fn repack_absent(reconciled_against: ExtensionVersion) -> ExtensionLag {
        ExtensionLag {
            recorded: Some(RecordedExtension {
                installed: ExtensionPresence::Absent,
                reconciled_against: Some(reconciled_against),
            }),
            shipped: Some(PG_REPACK_SHIPPED),
        }
    }

    fn names(order: &[TenantName]) -> Vec<&str> {
        order.iter().map(TenantName::as_str).collect()
    }

    #[test]
    fn a_tenant_two_minor_versions_behind_comes_before_one_a_minor_version_behind() {
        let order = catch_up_order(vec![
            candidate(
                "one",
                behind(ExtensionVersion::new(2, 30, 0), TIMESCALEDB_SHIPPED),
                current(POSTGIS_SHIPPED),
                900,
            ),
            candidate(
                "two",
                behind(ExtensionVersion::new(2, 29, 1), TIMESCALEDB_SHIPPED),
                current(POSTGIS_SHIPPED),
                10,
            ),
        ]);
        assert_eq!(names(&order), ["two", "one"]);
    }

    #[test]
    fn of_two_tenants_at_one_version_the_one_with_more_committed_transactions_comes_first() {
        let installed = ExtensionVersion::new(2, 30, 0);
        let order = catch_up_order(vec![
            candidate("quiet", behind(installed, TIMESCALEDB_SHIPPED), current(POSTGIS_SHIPPED), 12),
            candidate("busy", behind(installed, TIMESCALEDB_SHIPPED), current(POSTGIS_SHIPPED), 4_000),
        ]);
        assert_eq!(names(&order), ["busy", "quiet"]);
    }

    #[test]
    fn a_tenant_behind_on_timescaledb_comes_before_one_behind_only_on_postgis() {
        let order = catch_up_order(vec![
            candidate(
                "postgis",
                current(TIMESCALEDB_SHIPPED),
                behind(ExtensionVersion::new(3, 0, 0), POSTGIS_SHIPPED),
                0,
            ),
            candidate(
                "timescaledb",
                behind(ExtensionVersion::new(2, 30, 9), TIMESCALEDB_SHIPPED),
                current(POSTGIS_SHIPPED),
                0,
            ),
        ]);
        assert_eq!(names(&order), ["timescaledb", "postgis"]);
    }

    #[test]
    fn a_tenant_no_open_has_recorded_comes_before_every_recorded_one() {
        let unrecorded = ExtensionLag {
            recorded: None,
            shipped: Some(TIMESCALEDB_SHIPPED),
        };
        let order = catch_up_order(vec![
            candidate(
                "recorded",
                behind(ExtensionVersion::new(2, 20, 0), TIMESCALEDB_SHIPPED),
                current(POSTGIS_SHIPPED),
                9_000,
            ),
            candidate("unrecorded", unrecorded, current(POSTGIS_SHIPPED), 0),
        ]);
        assert_eq!(names(&order), ["unrecorded", "recorded"]);
    }

    #[test]
    fn a_tenant_whose_last_open_ran_against_the_shipped_version_is_not_listed_though_still_behind() {
        let refused = recorded(ExtensionVersion::new(2, 30, 0), TIMESCALEDB_SHIPPED, TIMESCALEDB_SHIPPED);
        assert_eq!(
            catch_up_order(vec![candidate("refused", refused, current(POSTGIS_SHIPPED), 50)]),
            Vec::<TenantName>::new()
        );
    }

    #[test]
    fn a_tenant_last_tried_against_an_older_server_version_is_listed_again() {
        let tried_before = recorded(ExtensionVersion::new(2, 29, 0), ExtensionVersion::new(2, 30, 0), TIMESCALEDB_SHIPPED);
        let order = catch_up_order(vec![candidate("refused", tried_before, current(POSTGIS_SHIPPED), 50)]);
        assert_eq!(names(&order), ["refused"]);
    }

    #[test]
    fn a_tenant_with_nothing_to_update_is_not_listed() {
        let ahead = recorded(ExtensionVersion::new(2, 32, 0), ExtensionVersion::new(2, 32, 0), TIMESCALEDB_SHIPPED);
        let unshipped = ExtensionLag {
            recorded: Some(RecordedExtension {
                installed: ExtensionPresence::Installed(ExtensionVersion::new(3, 0, 0)),
                reconciled_against: None,
            }),
            shipped: None,
        };
        let absent = ExtensionLag {
            recorded: Some(RecordedExtension {
                installed: ExtensionPresence::Absent,
                reconciled_against: Some(ExtensionVersion::new(3, 5, 0)),
            }),
            shipped: Some(POSTGIS_SHIPPED),
        };
        let order = catch_up_order(vec![
            candidate("current", current(TIMESCALEDB_SHIPPED), current(POSTGIS_SHIPPED), 1),
            candidate("ahead", ahead, current(POSTGIS_SHIPPED), 1),
            candidate("unshipped", current(TIMESCALEDB_SHIPPED), unshipped, 1),
            candidate("absent", current(TIMESCALEDB_SHIPPED), absent, 1),
        ]);
        assert_eq!(order, Vec::<TenantName>::new());
    }

    #[test]
    fn a_tenant_whose_pg_repack_is_a_patch_behind_the_image_is_listed() {
        let order = catch_up_order(vec![CatchUpCandidate {
            pg_repack: behind(ExtensionVersion::new(1, 5, 2), PG_REPACK_SHIPPED),
            ..candidate("repack", current(TIMESCALEDB_SHIPPED), current(POSTGIS_SHIPPED), 0)
        }]);
        assert_eq!(names(&order), ["repack"]);
    }

    #[test]
    fn a_tenant_whose_pg_repack_is_absent_and_never_tried_against_the_shipped_version_is_listed() {
        let order = catch_up_order(vec![
            CatchUpCandidate {
                pg_repack: repack_absent(ExtensionVersion::new(1, 5, 2)),
                ..candidate("tried-before", current(TIMESCALEDB_SHIPPED), current(POSTGIS_SHIPPED), 0)
            },
            CatchUpCandidate {
                pg_repack: ExtensionLag {
                    recorded: None,
                    shipped: Some(PG_REPACK_SHIPPED),
                },
                ..candidate("unrecorded", current(TIMESCALEDB_SHIPPED), current(POSTGIS_SHIPPED), 0)
            },
        ]);
        assert_eq!(names(&order), ["unrecorded", "tried-before"]);
    }

    #[test]
    fn a_tenant_whose_pg_repack_stayed_absent_against_the_shipped_version_or_is_not_shipped_is_not_listed() {
        let order = catch_up_order(vec![
            CatchUpCandidate {
                pg_repack: repack_absent(PG_REPACK_SHIPPED),
                ..candidate("refused", current(TIMESCALEDB_SHIPPED), current(POSTGIS_SHIPPED), 1)
            },
            CatchUpCandidate {
                pg_repack: ExtensionLag {
                    recorded: Some(RecordedExtension {
                        installed: ExtensionPresence::Absent,
                        reconciled_against: None,
                    }),
                    shipped: None,
                },
                ..candidate("unshipped", current(TIMESCALEDB_SHIPPED), current(POSTGIS_SHIPPED), 1)
            },
            CatchUpCandidate {
                pg_repack: recorded(ExtensionVersion::new(1, 5, 2), PG_REPACK_SHIPPED, PG_REPACK_SHIPPED),
                ..candidate("deferred", current(TIMESCALEDB_SHIPPED), current(POSTGIS_SHIPPED), 1)
            },
        ]);
        assert_eq!(order, Vec::<TenantName>::new());
    }

    #[test]
    fn a_tenant_behind_on_postgis_comes_before_one_behind_only_on_pg_repack() {
        let order = catch_up_order(vec![
            CatchUpCandidate {
                pg_repack: behind(ExtensionVersion::new(1, 4, 0), PG_REPACK_SHIPPED),
                ..candidate("repack", current(TIMESCALEDB_SHIPPED), current(POSTGIS_SHIPPED), 9_000)
            },
            candidate(
                "postgis",
                current(TIMESCALEDB_SHIPPED),
                behind(ExtensionVersion::new(3, 5, 0), POSTGIS_SHIPPED),
                0,
            ),
        ]);
        assert_eq!(names(&order), ["postgis", "repack"]);
    }
}
