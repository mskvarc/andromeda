use shadow_rs::ShadowBuilder;
use std::error::Error;

fn main() -> Result<(), Box<dyn Error>> {
    ShadowBuilder::builder().build()?;
    Ok(())
}
