# Reowolf 1.0 Implementation

## Compilation instructions
1. Install the latest stable Rust toolchain using Rustup. See https://rustup.rs/ for further instructions.
1. Run `cargo build --release` to download source dependencies, and compile the library with release-level optimizations. 
	- The resulting dylib can be found in target/release/, to be used with the header file reowolf.h.
	- Note: A list of immediate ancestor dependencies is visible in Cargo.toml.
	- Note: Run `cargo test --release` to run unit tests with release-level optimizations.

## Build options
- `cargo build --release` produces the dylib object, exposing connector API for Rust. The C FFI is also included, and corresponds to the header file `reowolf.h`.
- `cargo build --release --features ffi_pseudo_socket_api` is only available on Linux, and also generates functions which comprise the pseudo-socket C FFI.

## Notes
3. Running `cbindgen > reowolf.h` from the root will overwrite the header file. (WIP) This is only necessary to update it.  