use shadow_rs::ShadowBuilder;

fn main() {
    if let Err(error) = ShadowBuilder::builder().build() {
        eprintln!("failed to generate broker component build metadata: {error}");
        std::process::exit(1);
    }
}
