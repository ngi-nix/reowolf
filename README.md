# Reowolf 1.1 Implementation

This library builds upon the previous Reowolf 1.0 implementation, with the intention of incrementally moving towards Reowolf 2.0. The goals of Reowolf 2.0 are to provide a generalization of sockets for communication over the internet, to simplify the implementation of algorithms that require distributed consensus, and to have a clear delineation between protocol and data whenever information is transmitted over the internet.

The Reowolf 1.1 implementation has the main goal of extending the Protocol Description Language (PDL) to be more pragmatic. The language now supports `struct`, `enum` and `union` types, has support for basic ad-hoc polymorphism, supports basic pattern matching, and makes channels of communication specify the types of values they're transmitting.

Reowolf 1.2 will focus on replacing the centralized algorithm for consensus with a distributed one. The distributed implementation should reduce the overhead of synchronization and consensus, especially in the case where the communicating parties are governed by simple protocols.

## Compilation instructions

1. Install the latest stable Rust toolchain (`rustup install stable`) using the [rustup](https://rustup.rs/) CLI tool, available for most platforms.
2. Have `cargo` (the Rust language package manager) download source dependencies, and compile the library with release-level optimizations with `cargo build --release`. Run `cargo test --release` to run the available tests with release-level optimization.

## Using the library

The library may be used as a Rust dependency by adding it as a git dependency, i.e., by adding line `reowolf_rs = { git = "https://scm.cwi.nl/FM/reowolf" }` to downstream crate's manifest file, `Cargo.toml`.

## Library Overview

The library is subdivided into the following modules:

- `collections`: Utility datastructures used throughout the compiler.
- `protocol`: PDL parser, compiler and runner. It is itself subdivided into:
	- `eval`: The evaluator of the compiled language. Currently an AST walker with cell-like memory management.
	- `parser`: The compiler itself, responsible for lexing, parsing and validating the incoming source.
	- `tests`: Language tests.
- `runtime`: The PDL runtime.

The code's entry point for the compiler can be found in `src/protocol/parser/mod.rs`. It accepts several input source files which will then all be compiled at once. Compilation is achieved through several helpers found in `src/protocol/parser/pass_*.rs`.

## Contributor Overview

The details of the implementation are best understood by reading the doc comments, starting from the procedures listed in the section above. One may also refer to the Reowolf 1.0 project's companion [documentation](https://doi.org/10.5281/zenodo.3559822) for a higher level overview of the goals and design of the implementation.