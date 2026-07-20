# P6 — Hidden-atom dependency diagnosis

## Scope

This diagnostic covers QSynth EA lines 53, 249, 260, 369 and 481 after P3
commit `dfbb8af`. It does not change simplification. Hidden atoms are retained
only during an explicit `diagnose_hidden_atoms` call.

Reproduce the compact report with:

```text
cargo run -p rumba-core --release --all-features \
  --example hidden_atom_diagnostics
```

Pass `--verbose` to include every scope, original definition, simplified
definition and structurally recovered dependency definition.

## Classification method

For every recursive `MBASolver` invocation, the diagnostic records each atom
created by `hide_in_var`. Within one scope it then replaces exact structural
occurrences of other atom definitions and classifies the result as:

- `Root`: no structurally recoverable dependency;
- `BitwiseDependent`: a candidate using only `~`, `&`, `|`, `^`,
  variables, `0` and `mask`;
- `ArithmeticDependent`: a recovered dependency still containing arithmetic;
- `LowBitCandidate`: a structural `x & -x` occurrence.

This is deliberately syntactic. In particular, zero LowBit candidates means
that no direct `x & -x` survived in the traced definitions; it does not prove
that no algebraically equivalent LowBit relation exists.

## Results

All five canonical residuals remain non-zero under ordinary simplification.

| Line | Residual nodes | Scopes | Atoms | Roots | Bitwise deps | Arithmetic deps | LowBit candidates |
| ---: | -------------: | -----: | ----: | ----: | -----------: | --------------: | ----------------: |
| 53   | 230 | 4  | 17 | 12 | 0 | 5  | 0 |
| 249  | 150 | 4  | 10 | 5  | 0 | 5  | 0 |
| 260  | 99  | 23 | 41 | 26 | 0 | 15 | 0 |
| 369  | 45  | 5  | 10 | 8  | 0 | 2  | 0 |
| 481  | 96  | 6  | 18 | 14 | 0 | 4  | 0 |

The same classification is visible before the final residual:

| Line | Stage | Atoms | Roots | Bitwise deps | Arithmetic deps | LowBit candidates |
| ---: | :---- | ----: | ----: | -----------: | --------------: | ----------------: |
| 53  | MBA | 37 | 27 | 0 | 10 | 0 |
| 53  | GT  | 7  | 4  | 0 | 3  | 0 |
| 249 | MBA | 27 | 21 | 0 | 6  | 0 |
| 249 | GT  | 5  | 4  | 0 | 1  | 0 |
| 260 | MBA | 35 | 26 | 0 | 9  | 0 |
| 260 | GT  | 12 | 8  | 0 | 4  | 0 |
| 369 | MBA | 12 | 11 | 0 | 1  | 0 |
| 369 | GT  | 8  | 7  | 0 | 1  | 0 |
| 481 | MBA | 43 | 34 | 0 | 9  | 0 |
| 481 | GT  | 12 | 10 | 0 | 2  | 0 |

## Decision gates

### P7a: strict dependency-constrained signatures

Not justified yet. Structural substitution recovers dependencies, but every
recovered dependency in the five residuals is arithmetic. A Boolean signature
over these definitions would not be a sound word-level proof without first
establishing linearity or applying the existing PCT correctly.

### P8: direct LowBit domain

Not justified for these five cases by the current evidence. No direct LowBit
form occurs in any traced MBA, ground truth or residual definition. P8 remains
useful for the separate corpus family that exposes `x & -x`, `2x` and related
2-adic structure.

### Next focused experiment

Inspect whether the recovered arithmetic dependencies are semantically bitwise
through the existing simplifier/PCT, with exact proofs and the same free
variables. This is the narrow P7b diagnostic described in the plan. Do not
return zero from Boolean samples of an arbitrary polynomial expression: for
example, `x*x - x` vanishes on Boolean inputs but not on arbitrary words.

## P6 conclusion

P6 rejects both immediate branches for the five regressions:

- strict P7a has no eligible bitwise dependency;
- direct P8 has no exposed LowBit candidate.

The blocker is a set of arithmetic hidden-atom dependencies. The next change,
if pursued, must prove which of those definitions are valid bitwise functions
under PCT rather than assuming independent Boolean atoms or adding LowBit rules
without a matching residual.
