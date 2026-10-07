//! Re-runs the build whenever a migration changes: `sqlx::migrate!` embeds the migration bodies but does not register the directory with cargo, so an edited migration would otherwise leave a stale embedded set and stamp fresh tenants with the old digest.

fn main() {
    println!("cargo:rerun-if-changed=../../migrations/platform");
    println!("cargo:rerun-if-changed=../../migrations/tenant");
    println!("cargo:rerun-if-changed=../../migrations/branch");
}
