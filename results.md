# Rumba corpus results

This document reports two different configurations. They must not be confused:

1. the **experimental P7e/P8 pipeline**, which reaches 41 NG and eliminates all
   QSynth NG cases;
2. the **normal simplifier baseline**, which does not enable those diagnostics
   and still reports 101 NG, including 19 in QSynth.

## Experimental P7e/P8 result

The best measured diagnostic order is:

```text
normal simplification -> P7e -> P8a -> P8b -> P8c
```

This order is now packaged by the opt-in `p8::experiment_pipeline` API and is
covered by stage-specific integration tests. It is still not invoked by the
normal `simplify_mba` entry point.

It reduces the complete 41,000-expression corpus from 101 to **41 NG**:

| Dataset | Cases | Baseline NG | Resolved experimentally | Remaining NG | Success rate |
|---|---:|---:|---:|---:|---:|
| `syntia.csv` | 500 | 0 | 0 | 0 | 100.00% |
| `mba_obf_nonlinear.csv` | 1,000 | 0 | 0 | 0 | 100.00% |
| `mba_obf_linear.csv` | 1,000 | 0 | 0 | 0 | 100.00% |
| `qsynth_ea.csv` | 500 | 19 | **19** | **0** | **100.00%** |
| `mba_flatten.csv` | 3,000 | 0 | 0 | 0 | 100.00% |
| `neureduce.csv` | 10,000 | 0 | 0 | 0 | 100.00% |
| `loki_tiny.csv` | 25,000 | 82 | 41 | 41 | 99.84% |
| **Total** | **41,000** | **101** | **60** | **41** | **99.90%** |

The stage-by-stage progression is:

| Diagnostic stage | Newly resolved | NG remaining |
|---|---:|---:|
| Normal simplifier baseline | — | 101 |
| P7e bitwise dependency closure | 32 | 69 |
| P8a then P8b | 3 | 66 |
| P8c after P8b | 25 | **41** |

P7e accounts for all QSynth improvements:

```text
qsynth_ea.csv: 19 NG -> 0 NG
loki_tiny.csv: 82 NG -> 69 NG
```

The P8 stages then operate on the remaining Loki cases. None of the other five
datasets contains an NG case in either configuration.

### P8c ablation

On the same 66 inputs remaining after P7e, P8a, and P8b:

| P8c variant | Resolved | Analysis time |
|---|---:|---:|
| Full relative quotient domain | 24 | 1.935 ms |
| `ZeroOnly` | 24 | 1.518 ms |
| `AnchorsOnly` / `AboveLow` | 24 | 1.462 ms |
| Full, after terminal P8b | **25** | 1.576 ms |

`KnownOne` does not add corpus coverage yet: the 24 shared resolutions can all
be obtained from LowBit anchor recognition and `AboveLow`. Running P8b before
P8c additionally resolves `loki_tiny.csv:19728`.

### No-oracle deployment benchmark

This benchmark simplified only the 41,000 MBA inputs and never inspected their
ground truths:

| Variant | Triggered expressions | Changed expressions | Incremental overhead |
|---|---:|---:|---:|
| Full | 65 | 24 | 0.846% |
| `AnchorsOnly` | 65 | 24 | 0.648% |

Both variants reported zero sampled semantic regressions, zero determinism
violations, and zero cases where an `Unknown` result changed the AST.

### Exact validation

The complete 41,000-case comparison and the exact 64-bit certificate run report:

```text
baseline_NG=101
experimental_NG=41
qsynth_ea.csv experimental_NG=0

no_oracle_changed=24
exact_UNSAT=24
exact_SAT=0
exact_UNKNOWN=0
p8c_resolved_residuals=25
```

`changed=24` and `resolved=25` measure different populations: the first is the
number of ordinary simplified MBA outputs changed by the no-oracle P8c pass;
the second is the number of known residuals closed by P8c after P7e/P8a/P8b.
All 24 changed no-oracle outputs were proven exactly equivalent at width 64.

### Remaining 41 NG: causal blockers

The deterministic first-blocker classification is:

| First blocker | Cases |
|---|---:|
| `NoLowBitCandidate` | 13 |
| `AnchorCandidateUnproved` | 11 |
| `AnchorProvedNoUse` | 7 |
| `AboveLowUnknown` | 10 |
| `MultipleAnchors` | 0 |
| `RewriteAppliedNonZero` | 0 |
| `TerminalNormalizationFailure` | 0 |

The non-exclusive raw counters additionally include 13 `AboveLowUnknown` and
11 multiple-anchor residuals. Counterfactual facts are applied only to copies:

```text
counterfactual_anchor_zero=0
counterfactual_above_zero=3
counterfactual_reduced_over_50_percent=5
```

The three residuals made exactly zero by an assumed `AboveLow` fact are
`loki_tiny.csv:3312`, `17633`, and `17980`. Therefore a bounded P8d membership
proof has an immediate causal ceiling of three known resolutions. Improving
anchor recognition alone has no directly resolving counterfactual in this set.

The P7e/P8 pipeline remains opt-in: these projected corpus results are not yet
produced by the normal `simplify_mba` path.

## Normal simplifier baseline

The baseline was reproduced with:

```console
just test
```

It ran in release mode with all features enabled and completed the seven dataset
tests in 3.11 seconds, excluding compilation.

### Baseline summary

| Status | Count | Rate |
|---|---:|---:|
| OK | 40,767 | 99.43% |
| OKZ | 132 | 0.32% |
| NG | 101 | 0.25% |
| **Successful (OK + OKZ)** | **40,899** | **99.75%** |

### Baseline by dataset

| Dataset | Cases | OK | OKZ | NG | Success rate | Median time |
|---|---:|---:|---:|---:|---:|---:|
| `syntia.csv` | 500 | 480 | 20 | 0 | 100.00% | 20.93 us |
| `mba_obf_nonlinear.csv` | 1,000 | 1,000 | 0 | 0 | 100.00% | 96.52 us |
| `mba_obf_linear.csv` | 1,000 | 1,000 | 0 | 0 | 100.00% | 58.43 us |
| `qsynth_ea.csv` | 500 | 369 | 112 | 19 | 96.20% | 301.41 us |
| `mba_flatten.csv` | 3,000 | 3,000 | 0 | 0 | 100.00% | 20.84 us |
| `neureduce.csv` | 10,000 | 10,000 | 0 | 0 | 100.00% | 56.24 us |
| `loki_tiny.csv` | 25,000 | 24,918 | 0 | 82 | 99.67% | 39.33 us |

## Terminology and validation

- **OK**: the simplified expression matches the simplified ground truth.
- **OKZ**: the expressions differ structurally, but their difference simplifies
  to zero. These cases count as successful.
- **NG**: Rumba did not reduce the case to a form recognized as equivalent to
  the expected result. It does not necessarily mean the result is mathematically
  incorrect.

For non-rejected baseline results, the test suite also checks semantic
equivalence on 200 generated assignments. The P7e/P8 diagnostics additionally
use exact bounded transformations and report sampled regression counters. The
sampling checks are validation evidence, not general formal proofs.

Timing values are machine-dependent and should be used only to compare runs in
the same environment.
