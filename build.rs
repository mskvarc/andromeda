use shadow_rs::ShadowBuilder;

fn main() -> Result<(), shadow_rs::ShadowError> {
    ShadowBuilder::builder().build()?;
    Ok(())
}
