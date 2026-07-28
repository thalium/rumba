# Rumba

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
| `--no-patterns` | Disable the structural pattern-rewrite engine |

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

| Dataset | Count | OK | OKZ | NG | Median |
| --- | ---: | ---: | ---: | ---: | ---: |
| `loki_tiny` | 25 000 | 24 994 | 1 | 5 | 67.99 µs |
| `neureduce` | 10 000 | 10 000 | 0 | 0 | 133.67 µs |
| `mba_flatten` | 3 000 | 3 000 | 0 | 0 | 43.57 µs |
| `mba_obf_linear` | 1 000 | 1 000 | 0 | 0 | 414.76 µs |
| `mba_obf_nonlinear` | 1 000 | 1 000 | 0 | 0 | 136.59 µs |
| `syntia` | 500 | 480 | 20 | 0 | 43.86 µs |
| `qsynth_ea` | 500 | 376 | 124 | 0 | 497.52 µs |
| **Total** | **41 000** | **40 850** | **145** | **5** | |

- **OK** — the simplified expression is syntactically identical to the
  simplified ground truth.
- **OKZ** — the two differ syntactically, but `simplify(mba - ground_truth)`
  reduces to `0`, so they are proven equivalent.
- **NG** — neither of the above. Solver errors are counted here too, so there is
  no separate **ERR** bucket: the 5 above is the total of both.

Every result is additionally checked for semantic equivalence against the input
on 200 random assignments; a mismatch fails the test suite outright, so no
unsound simplification can reach this table.

Timings are the wall-clock median of the simplification call alone, from a
single run on one machine — treat them as an order of magnitude, not a
benchmark. Reproduce with:

```sh
cargo test datasets --release --all-features -- --nocapture
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
