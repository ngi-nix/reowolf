# Reowolf 1.0 Implementation

## Compilation instructions
1. Install the latest stable Rust toolchain using Rustup. See https://rustup.rs/ for further instructions.
2. Run `cargo build --release` to download source dependencies, and compile the library with release-level optimizations. 
	- The resulting dylib can be found in target/release/, to be used with the header file reowolf.h.
	- Note: A list of immediate ancestor dependencies is visible in Cargo.toml.
	- Note: Run `cargo test --release` to run unit tests with release-level optimizations.
