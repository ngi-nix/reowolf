# Examples
This directory contains a set of programs for demonstrating the use of connectors for communications over the internet.

## Setting up and running
First, ensure that the Reowolf has been compiled, such that a dylib is present in `reowolf/target/release/`; see the parent directory for instructions on compiling the library.

The examples are designed to be run with the examples folder as your working directory (`reowolf/examples`), containing a local copy of the dylib. Two convenience scripts are made available for copying the library: `cpy_dll.sh` and `cpy_so.sh` for platforms Linux and Windows respectively.

Compiling the example programs is done using Python 3, with `python3 ./make.py`, which will crawl the example directory's children and compiling any C source files. 

## Groups of examples and takeaways
### Incremental Connector API examples
The examples contained within directories with names prefixed with `incremental_` demonstrate usage of the connector API. This set of examples is characterized by each example being self-contained, designed to be run in isolation. The examples are best understood when read in the order of their directories' names (from 1 onward), as they demonstrate the functionality of the connector API starting from the most (trivially) simple connectors, to more complex connectors incorporating multiple network channels and components.

Each example source file is prefixed by a multi-line comment, explaining what a reader is intended to take away from the example.

### Presentation examples
The examples contained within directories with names matching `pres_` are designed to accompany the Reowolf demonstration slides (inclusion here TODO).
Examples include interactions whose distributed sessions span multiple source files. What follows is a list of the sessions' consituent programs, along with what the session intends to demonstrate

1. {pres_1/amy, pres_1/bob}: Connectors can be used to transmit messages over the internet in a fashion comparable to that of UDP sockets.

2. {pres_1/amy, pres_2/bob}: The protocol descriptions used to configure components are loaded and parsed at runtime. Consequently, changing these descriptions can change the behavior of the system without recompiling any of the constituent programs.
2. {pres_3/amy, pres_3/bob}: *Atomicity*. Message exchange actions are grouped into synchronous interactions. Actions occur with observable effects IFF their interactions are well-formed, and complete successfully. For example, a put succeeds IFF its accompanying get succeeds.
2. {pres_3/amy, pres_4/bob}: *Nondeterminism*. Programs/components can express flexibility by providing mutually-exclusive firing patterns on their ports, as a nondeterministic choice. Which (if any) choice occurs can be determined after synchronization by inspecting the return value of `connector_sync`. Atomicity + Nondeterminism = Specialization of behavior.
2. {pres_5/amy, pres_5/bob}: When no synchronous interaction is found before some consituent program times out, the system RECOVERS to the synchronous state at the start of the round, allowing components to try again.

### Interoperability examples
The examples contained within directories with names matching `interop_` demonstrate the use of different APIs for communication over UDP channels. The three given programs are intended to be run together, each as its own process.

Each example source file is prefixed by a multi-line comment, explaining what a reader is intended to take away from the example.

NOTE: These examples are designed to compile on Linux!