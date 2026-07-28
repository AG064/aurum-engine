// Build script for the Life Evolution GDExtension.
//
// This script ensures the build directory exists and sets up
// the environment for the gdext crate to find the Godot headers.

fn main() {
    // The gdext crate handles its own build configuration
    // through the godot-rust/gdext repository.
    println!("cargo:rerun-if-changed=src");
}
