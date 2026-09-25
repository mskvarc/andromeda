//! Re-run the build whenever a migration changes.
//!
//! `sqlx::migrate!` embeds the migration bodies at compile time but does not
//! register the directory with cargo, so an in-place edit of a migration was
//! not seen until the crate was otherwise rebuilt, and the stale embedded set
//! then stamped every fresh tenant with the old digest.

fn main() {
    println!("cargo:rerun-if-changed=../../migrations/platform");
    println!("cargo:rerun-if-changed=../../migrations/tenant");
    println!("cargo:rerun-if-changed=../../migrations/branch");
}
