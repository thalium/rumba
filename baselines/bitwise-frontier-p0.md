# Bitwise-frontier P0 baseline

Recorded on 2026-07-20 before implementing the bitwise-frontier mechanism.

## Reproduction

Run from the repository root:

```console
just bitwise-frontier-baseline
```

The command runs, in order:

1. focused `rumba-core` unit tests;
2. classification of QSynth EA lines 53, 249, 260, 369 and 481;
3. the complete dataset corpus in release mode with all features enabled.

## Source and build configuration

| Field | Value |
|---|---|
| Commit | `51f16164fa35de91c1a308abe6fc908618869b24` |
| Branch | `bax` |
| Worktree | Dirty |
| Compiler | `rustc 1.95.0 (59807616e 2026-04-14)` |
| Cargo | `cargo 1.95.0 (f2d3ce0bd 2026-03-21)` |
| Target profile | Release |
| Features | All features enabled |
| Bit width | 64 |
| Semantic checks | 200 generated assignments per accepted solve |

The worktree already contained the uncommitted P2 solver corrections and other
build-related changes when this baseline was recorded. Consequently, this is a
pre-bitwise-frontier baseline, not a pristine pre-P2 baseline. The five target
cases and the aggregate corpus counts were unchanged after the P2 corrections.

## Five target regressions

| QSynth EA line | Status | Solve time | Input nodes | Output nodes |
|---:|---:|---:|---:|---:|
| 53 | NG | 1,501.57 us | 505 | 110 |
| 249 | NG | 947.13 us | 666 | 83 |
| 260 | NG | 707.17 us | 225 | 31 |
| 369 | NG | 1,062.18 us | 560 | 26 |
| 481 | NG | 1,371.07 us | 348 | 94 |

Target summary: **0 OK, 0 OKZ, 5 NG**. Total execution time for the targeted
runner, including parsing, ground-truth classification and semantic checks, was
10.43 ms.

All five expressions parsed as two-column `mba,ground_truth` rows. No semantic
mismatch was detected during their 200-assignment differential checks. Their
`NG` classification means Rumba did not reduce the result to the same canonical
form as the ground truth, nor reduce their difference to zero.

## Complete corpus

| Status | Count | Rate |
|---|---:|---:|
| OK | 40,769 | 99.44% |
| OKZ | 130 | 0.32% |
| NG | 101 | 0.25% |
| **Successful (OK + OKZ)** | **40,899** | **99.75%** |

The corpus contains 41,000 expressions across seven datasets. All seven dataset
tests passed, and their release-mode execution took 2.37 seconds excluding
compilation.

## Focused test state

The focused unit-test step ran four tests: the three existing P2 regression
tests and the parser unit test. Result: **4 passed, 0 failed**.

## Measurement caveats

- Timings are single-run observations and are not cross-machine guarantees.
- Semantic checks use generated assignments and are not formal equivalence
  proofs.
- The authoritative corpus count is the number of expressions processed by the
  Rust test runner; newline-based tools such as `wc -l` can undercount files
  whose final row has no trailing newline.
