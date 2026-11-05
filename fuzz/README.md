# Fuzzing

Two layers back the crate's "no malformed input panics" guarantee.

## Always-on robustness test (stable, in CI)

`tests/fuzz_robustness.rs` is a dependency-free, deterministic fuzzer that runs
as part of `cargo test`. It feeds random and corrupted inputs — including
buffers at the exact sizes that pass image detection, and valid disks with bytes
flipped — to every parser and reader and asserts none panic. No toolchain beyond
stable Rust is needed.

## Coverage-guided fuzzing (this directory, nightly)

These [`cargo-fuzz`](https://github.com/rust-fuzz/cargo-fuzz) targets drive
libFuzzer for deeper, coverage-guided exploration. They are a **standalone
crate**, not built by `cargo test`, and require a nightly toolchain:

```sh
rustup toolchain install nightly
cargo install cargo-fuzz
cargo +nightly fuzz run image        # disk read paths
cargo +nightly fuzz run containers   # T64 / LNX / Zip-Code parsers
```

Targets:

| target | exercises |
|--------|-----------|
| `image` | `Disk::load` + header, blocks-free, validate, directory walk, file/REL reads |
| `containers` | `parse_t64`, `parse_lnx`, `parse_zipcode` |
