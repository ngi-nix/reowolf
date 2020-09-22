# Reowolf 1.0 Implementation

This library provides connectors as a generalization of sockets for use in communication over the internet. This repository is one of the deliverables of the [Reowolf project](https://nlnet.nl/project/Reowolf/) funded by the NLNet foundation.

## Compilation instructions
1. Install the latest stable Rust toolchain (`rustup install stable`) using the [rustup](https://rustup.rs/) CLI tool, available for most platforms.
1. Have `cargo` (the Rust language package manager) download source dependencies, and compile the library with release-level optimizations with `cargo build --release`: 
	- The resulting dylib can be found in `./target/release/`, to be used with the header file: `./reowolf.h`.
	- *Note*: A list of immediate ancestor dependencies is visible in `./Cargo.toml`.
	- *Note*: Run `cargo test --release` to run unit tests with release-level optimizations.

## Using the library
- The library may be used as a Rust dependency by adding it as a git dependency, i.e., by adding line `reowolf_rs = { git = "https://scm.cwi.nl/FM/reowolf" }` to downstream crate's manifest file, `Cargo.toml`.
- The library may be used as a dynamically-linked library using its C ABI as the cdylib written to `./target/release` when compiled with release optimizations, in combination to the header file `./reowolf.h`.
- When compiled on Linux, the compiled library will include definitions of pseudo-socket procedures declared in `./pseudo-socket.h` when compiled with `cargo build --release --features ffi_pseudo_socket_api`. The added functionality is only needed when requiring that connectors expose a socket-like API.

## Examples
The `examples` directory contains example usages of connectors for message passing over the internet. The programs within require that the library is compiled as a dylib (see above).

## Notes
3. Running `cbindgen > reowolf.h` from the root will overwrite the header file. This is only necessary to update it.  

## Short User Overview
The bulk of the library's functionality is exposed to the user in two types: 
1. `protocol::ProtocolDescription` 
1. `runtime::Connector` 

The former is created using `parse`. For the most part, the user is not expected to interact much with the structure, only passing it to the connector as a communication session is being set up.

The latter is created with `new`, configured with methods such as `new_net_port` and `add_component`, and connected via `connect`, whereafter it can be used for multi-party communication through methods `put`, `get`, `next_batch`, and `sync`.

## Contributor Overview
The details of the implementation are best understood by reading the doc comments, starting from the procedures listed in the section above. It is suggested to first/also refer to the Reowolf project's companion documentation (link TODO) for a higher level overview of the goals and design of the implementation.