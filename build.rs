//! Generates the build metadata behind the broker's `--version` output.

use shadow_rs::ShadowBuilder;
use std::error::Error;

/// Write the shadow-rs build metadata module into `OUT_DIR`.
fn main() -> Result<(), Box<dyn Error>> {
    ShadowBuilder::builder().build()?;
    Ok(())
}
