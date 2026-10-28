//! One way the migrations or the ledger break the seal, naming the file and
//! the rule broken.

use crate::{
    file_digest::FileDigest,
    git_revision::GitRevision,
    migration_file::MigrationFile,
    migration_version::MigrationVersion,
    release_version::ReleaseVersion,
    seal_ledger::{LEDGER_PATH, sealed_release::SealedMigration},
};

/// A broken seal.
#[derive(Debug, Clone, PartialEq, Eq, derive_more::Display)]
pub enum SealViolation {
    /// A sealed file's bytes differ from the ones sealed.
    #[display("{file}: sealed by {release} and edited since; a sealed migration never changes (sealed as {sealed}, now {found})")]
    SealedFileChanged {
        /// The file.
        file: MigrationFile,
        /// The release that sealed it.
        release: ReleaseVersion,
        /// The digest it was sealed with.
        sealed: FileDigest,
        /// The digest of its bytes now.
        found: FileDigest,
    },
    /// A sealed file is not where it was sealed.
    #[display("{migration}: sealed by {release} and deleted or renamed since; a sealed migration stays where it was sealed")]
    SealedFileMissing {
        /// The sealed file.
        migration: SealedMigration,
        /// The release that sealed it.
        release: ReleaseVersion,
    },
    /// An unsealed file holds a version at or below a sealed one.
    #[display("{file}: not sealed, and its version is not above {highest}, the highest sealed one; a new migration is numbered after every shipped one")]
    InsertedBehindSeal {
        /// The file.
        file: MigrationFile,
        /// The highest sealed version.
        highest: MigrationVersion,
    },
    /// The ledger is not in the form the tool writes.
    #[display("{LEDGER_PATH}: not in the canonical form `andromeda-sql seal` writes; the ledger is never edited by hand")]
    LedgerNotCanonical,
    /// One file is sealed by two releases.
    #[display("{LEDGER_PATH}: {migration} is sealed by both {first} and {second}; a release seals only the files it added")]
    MigrationSealedTwice {
        /// The file.
        migration: SealedMigration,
        /// The earlier release.
        first: ReleaseVersion,
        /// The later release.
        second: ReleaseVersion,
    },
    /// One release has two sections.
    #[display("{LEDGER_PATH}: {_0} has two sections; a release is sealed once")]
    ReleaseSealedTwice(ReleaseVersion),
    /// A section seals no file.
    #[display("{LEDGER_PATH}: {_0} seals no migration; a release that adds none is not sealed")]
    EmptyRelease(ReleaseVersion),
    /// A stable release is tagged without a section.
    #[display("{LEDGER_PATH}: {_0} has no section; a stable release is sealed before it is tagged")]
    ReleaseNotSealed(ReleaseVersion),
    /// A stable release is tagged where a later release is sealed already.
    #[display("{LEDGER_PATH}: {latest} is sealed after {release}; a stable release is tagged on the commit where it is the last release sealed")]
    ReleaseNotLatest {
        /// The release tagged.
        release: ReleaseVersion,
        /// The last release the ledger seals.
        latest: ReleaseVersion,
    },
    /// A stable release is tagged holding a migration no release sealed.
    #[display("{file}: not sealed, and {release} is tagged; every migration a stable release ships is sealed")]
    UnsealedAtRelease {
        /// The file.
        file: MigrationFile,
        /// The release tagged.
        release: ReleaseVersion,
    },
    /// A section the ledger held at a revision differs now.
    #[display("{LEDGER_PATH}: the section of {release} differs from the one at {revision}; a sealed section never changes")]
    ReleaseChangedSince {
        /// The release.
        release: ReleaseVersion,
        /// The revision compared with.
        revision: GitRevision,
    },
    /// A section the ledger held at a revision is gone.
    #[display("{LEDGER_PATH}: the section of {release} at {revision} is gone; a sealed section is never removed")]
    ReleaseRemovedSince {
        /// The release.
        release: ReleaseVersion,
        /// The revision compared with.
        revision: GitRevision,
    },
}
