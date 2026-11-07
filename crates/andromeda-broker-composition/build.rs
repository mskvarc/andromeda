//! Generates the build metadata (version, build time, branch, commit) the
//! broker's component report carries.

use shadow_rs::ShadowBuilder;
use std::process;

/// Write the shadow-rs build metadata module into `OUT_DIR`, failing the
/// build with a message when it cannot.
fn main() {
    if let Err(error) = ShadowBuilder::builder().build() {
        eprintln!("failed to generate broker component build metadata: {error}");
        process::exit(1);
    }
}
