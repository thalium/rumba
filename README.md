# Rumba

[![CI](https://img.shields.io/github/actions/workflow/status/thalium/rumba/ci.yml?branch=master&label=CI&logo=github)](https://github.com/thalium/rumba/actions/workflows/ci.yml)
[![crates.io](https://img.shields.io/crates/v/rumba?logo=rust&label=rumba)](https://crates.io/crates/rumba)
[![rumba-core](https://img.shields.io/crates/v/rumba-core?logo=rust&label=rumba-core)](https://crates.io/crates/rumba-core)
[![docs.rs](https://img.shields.io/docsrs/rumba-core?logo=docsdotrs&label=docs.rs)](https://docs.rs/rumba-core)
[![PyPI](https://img.shields.io/pypi/v/pyrumba?logo=pypi&logoColor=white&label=pyrumba)](https://pypi.org/project/pyrumba/)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE.md)
[![Playground](https://img.shields.io/badge/playground-wasm-663399?logo=webassembly&logoColor=white)](https://thalium.github.io/rumba/)
[![Paper](https://img.shields.io/badge/paper-hal--05578742-b31b1b)](https://hal.science/hal-05578742/document)

A fast simplifier for Mixed Boolean-Arithmetic (MBA) expressions.

MBA expressions interleave bitwise operators (`&`, `|`, `^`, `~`) with arithmetic
ones (`+`, `-`, `*`) to obscure an otherwise simple value. They are a common
code-obfuscation primitive. Rumba recovers the underlying expression:

```
(v0 ^ v1) + 2 * (v0 & v1)   ~>   v0 + v1
```

Rumba is the reference implementation of the algorithm described in
[hal-05578742](https://hal.science/hal-05578742/document).

Try it in your browser: **[playground](https://thalium.github.io/rumba/)** — the
simplifier compiled to WebAssembly, running locally in the page.

## Installation

```sh
cargo install rumba          # CLI
cargo add rumba-core         # library
pip install pyrumba          # python bindings
```

Or build from source:

```sh
git clone https://github.com/thalium/rumba
cd rumba
cargo build --release        # binary at target/release/rumba
```

## CLI

```
rumba [OPTIONS] <expression>
```

```sh
$ rumba "(v0^v1)+2*(v0&v1)"
Simplify (v0 ^ v1) + 2 * (v0 & v1)
(v0) + (v1)
```

| Option | Description |
| --- | --- |
| `--n <uint>` | Bit width of the expression (default: `32`) |
| `--hex` | Print constants in hexadecimal |
| `--test` | Check the result against the input on 1000 random inputs |

Variables are written `v0`, `v1`, `v2`, … Use `--test` to gain confidence on a
result you intend to act on:

```sh
$ rumba "-34*~v1*(v0&v1)-36*~v1*(v0&~v1)+12*~v1*~(v0&v1)+10*~v1*~(v0^v1)-24*~v1*~(v0|v1)-36*~v1*~(v0|~v1)+17*v1*(v0&v1)+18*v1*(v0&~v1)-6*v1*~(v0&v1)-5*v1*~(v0^v1)+12*v1*~(v0|v1)+18*v1*~(v0|~v1)+22*~v1*(v0|v1)-11*v1*(v0|v1)" --test
```

## Library

```rust
use rumba_core::expr::{Expr, VarId};
use rumba_core::simplify::simplify_mba;

let x = Expr::Var(VarId(0));
let y = Expr::Var(VarId(1));

// (x ^ y) + 2*(x & y) is an MBA encoding of x + y.
let mba = (x.clone() ^ y.clone()) + 2u64 * (x.clone() & y.clone());
let simplified = simplify_mba(mba, 64).expect("simplification failed");

assert!(simplified.sem_equal(&(x + y), 64, 1000).is_ok());
```

Simplification is best-effort: it returns a `SolveError` or leaves the input
untouched rather than aborting the process.

Cargo features: `parse` (parse expressions from strings via
`rumba_core::parser::parse_expr`), `jit` (JIT-compiled evaluation for faster
semantic checks).

Bindings for [Python](bindings/python), [C](bindings/c) and
[WebAssembly](bindings/wasm) live under `bindings/`.

## Results

Measured on the GAMBA dataset (41 000 expressions), at 64 bits:

| Dataset | Count | OK | OKZ | NG | p50 |
| --- | ---: | ---: | ---: | ---: | ---: |
| `loki_tiny` | 25 000 | 25 000 | 0 | 0 | 12 µs |
| `neureduce` | 10 000 | 9 867 | 133 | 0 | 26 µs |
| `mba_flatten` | 3 000 | 2 952 | 48 | 0 | 31 µs |
| `mba_obf_linear` | 1 000 | 992 | 8 | 0 | 32 µs |
| `mba_obf_nonlinear` | 1 000 | 988 | 12 | 0 | 55 µs |
| `syntia` | 500 | 480 | 20 | 0 | 9 µs |
| `qsynth_ea` | 500 | 376 | 124 | 0 | 237 µs |
| **Total** | **41 000** | **40 655** | **345** | **0** | **19 µs** |

- **OK** — the simplified expression is syntactically identical to the
  simplified ground truth.
- **OKZ** — the two differ syntactically, but `simplify(mba - ground_truth)`
  reduces to `0`, so they are proven equivalent.
- **NG** — neither of the above. Solver errors are counted here too, so there is
  no separate **ERR** bucket.

Every result is additionally checked for semantic equivalence against the input
on 200 random assignments; a mismatch fails the test suite outright, so no
unsound simplification can reach this table.

The quality pass is also the warm-up. The latency report then performs five
measured runs per dataset and reports the distribution from the median
total-time run. Timings are machine-dependent; reproduce them with:

```sh
just corpus
```

## Artifacts

Build the comparison artifacts with

```sh
docker build -o build .
```

This produces:

- `build/gamba_res.csv` and `build/rumba_res.csv`, which together with
  `make_graph.py` generate the time comparison graph
- `build/rumba`, a standalone binary
- `build/test_results.txt`, the test results with timing statistics and success
  rates

## Datasets

This project includes the dataset from
[GAMBA](https://github.com/DenuvoSoftwareSolutions/GAMBA) (Copyright (c) 2023
Denuvo GmbH, released under GPLv3), located in `third_party/dataset` and
redistributed under its original license. See the `LICENSE.md` file in that
directory for full terms.

The datasets were preprocessed to unify variable names and file formats.

## License

Licensed under the MIT License. See [`LICENSE.md`](LICENSE.md) for the full text.

Unless otherwise noted, all files in this repository are licensed under the MIT
License; files located in `third_party/` are excluded and remain licensed under
their original license (see `third_party/dataset/LICENSE.md`).

Copyright (c) 2026 THALES
