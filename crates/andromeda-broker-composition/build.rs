use shadow_rs::ShadowBuilder;
use std::process;

fn main() {
    if let Err(error) = ShadowBuilder::builder().build() {
        eprintln!("failed to generate broker component build metadata: {error}");
        process::exit(1);
    }
}
