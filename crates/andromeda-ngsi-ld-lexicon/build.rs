//! Rebuilds the crate whenever a composed grammar changes: the grammar derive reads its file without registering it with cargo, so an edited grammar would otherwise leave the automaton compiled from the one before it.

fn main() {
    println!("cargo:rerun-if-changed=grammar/.generated");
}
