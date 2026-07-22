#![cfg(feature = "parse")]

use std::sync::OnceLock;
use std::time::{Duration, Instant};

use rumba_core::{expr::Expr, parser::parse_expr, simplify};

/// Whether the pattern engine is enabled for this run.
///
/// `RUMBA_PATTERNS=0` disables it, so the corpus can be scored with and without
/// the patterns -- see the `test-nopatterns` just target. Read once, so it costs
/// nothing inside the timed loop.
fn patterns_enabled() -> bool {
    static ENABLED: OnceLock<bool> = OnceLock::new();
    *ENABLED.get_or_init(|| std::env::var("RUMBA_PATTERNS").as_deref() != Ok("0"))
}

/// Simplifies under the options selected by the environment.
fn simplify_mba(e: Expr, n: u8) -> Result<Expr, simplify::SolveError> {
    simplify::simplify_mba_with(
        e,
        n,
        simplify::SimplifyOptions {
            patterns: patterns_enabled(),
        },
    )
}

/// The number of semantic tests to run
const SEMANTIC_TEST_COUNT: usize = 200;

/// The bit count for the experiments
const BIT_COUNT: u8 = 64;

/// Simiplification status
enum Status {
    /// gt == simplified_mba
    Ok,

    /// gt - simplified_mba == 0
    OkZ,

    /// Not Good
    NG,
}

/// The results of an experiment run
struct ExperimentResult {
    /// Simplification execution time
    elapsed: Duration,

    /// Simplification status
    status: Status,

    /// For NG results, a human-readable dump of the failing case
    ng: Option<String>,
}

/// An MBA to simplify with the expected ground truth
/// The filename and line number are used to report errors
struct Experiment {
    filename: &'static str,
    line_nb: usize,
    mba: Expr,
    gt: Expr,
}

impl Experiment {
    fn new(filename: &'static str, line: &str, line_nb: usize) -> Self {
        let line = line.trim();

        let line: &str = line.trim();
        let parts: Vec<&str> = line.split(',').map(|s| s.trim()).collect();
        assert!(parts.len() == 2, "line {}: expected 2 columns", line_nb + 1);

        Self {
            filename,
            line_nb: line_nb + 1,
            mba: parse_expr(parts[0]).unwrap(),
            gt: parse_expr(parts[1]).unwrap(),
        }
    }

    /// Runs our experiment, timing simplification time, comparing semantics then comparing the result with the ground truth
    fn run(&self) -> ExperimentResult {
        let start = Instant::now();
        let simplified_mba = simplify_mba(self.mba.clone(), BIT_COUNT);
        let elapsed = start.elapsed();

        // The solver rejected this expression outright. Score it as a miss
        // rather than aborting the whole corpus run.
        let Ok(simplified_mba) = simplified_mba else {
            return ExperimentResult {
                elapsed,
                status: Status::NG,
                ng: None,
            };
        };

        if let Err((_, v1, v2)) = simplified_mba.sem_equal(&self.gt, BIT_COUNT, SEMANTIC_TEST_COUNT)
        {
            assert_eq!(
                v1, v2,
                "{}:{} semantic error mba: {}, gt: {}",
                self.filename, self.line_nb, simplified_mba, self.gt
            );
        }

        let Ok(gt_produced) = simplify_mba(self.gt.clone(), BIT_COUNT) else {
            return ExperimentResult {
                elapsed,
                status: Status::NG,
                ng: None,
            };
        };

        let mut status = Status::NG;
        let mut ng = None;

        if simplified_mba == gt_produced {
            status = Status::Ok;
        } else {
            let diff = (self.mba.clone() - self.gt.clone()).reduce(BIT_COUNT);
            let diff_produced = simplify_mba(diff, BIT_COUNT);
            if diff_produced == Ok(Expr::zero()) {
                status = Status::OkZ;
            } else {
                let diff_produced = match &diff_produced {
                    Ok(e) => e.to_string(),
                    Err(err) => format!("<error: {err}>"),
                };
                ng = Some(format!(
                    "{}:{}\n  mba:           {}\n  gt:            {}\n  produced:      {}\n  gt-produced:   {}\n  diff-produced: {}\n",
                    self.filename,
                    self.line_nb,
                    self.mba,
                    self.gt,
                    simplified_mba,
                    gt_produced,
                    diff_produced
                ));
            }
        }

        ExperimentResult {
            elapsed,
            status,
            ng,
        }
    }
}

/// Extracts execution time quartiles from a set of experiment results.
fn quartiles(samples: &[ExperimentResult]) -> (Duration, Duration, Duration, Duration, Duration) {
    let mut execution_times: Vec<_> = samples.iter().map(|res| res.elapsed).collect();
    execution_times.sort_unstable();

    let n = execution_times.len();

    let q0 = execution_times[0];
    let q1 = execution_times[n / 4];
    let q2 = execution_times[n / 2];
    let q3 = execution_times[(3 * n) / 4];
    let q4 = execution_times[n - 1];

    (q0, q1, q2, q3, q4)
}

fn format_duration(dur: Duration) -> String {
    let secs = dur.as_secs();
    let nanos = dur.subsec_nanos();

    if secs >= 1 {
        let total = secs as f64 + (nanos as f64) / 1_000_000_000.0;
        format!("{:.2}s", total)
    } else if nanos >= 1_000_000 {
        let total = nanos as f64 / 1_000_000.0;
        format!("{:.2}ms", total)
    } else if nanos >= 1_000 {
        let total = nanos as f64 / 1_000.0;
        format!("{:.2}µs", total)
    } else {
        format!("{}ns", nanos)
    }
}

/// Counts the number of statuses in the results
fn count_types(results: &Vec<ExperimentResult>) -> (usize, usize, usize) {
    let mut oks = 0;
    let mut okzs = 0;
    let mut ngs = 0;

    for res in results {
        match res.status {
            Status::Ok => oks += 1,
            Status::OkZ => okzs += 1,
            Status::NG => ngs += 1,
        };
    }

    (oks, okzs, ngs)
}

/// Writes the failing (NG) cases of a dataset to `ng/<dataset>.txt` at the repo
/// root, so they can be inspected after a run. The file is always (re)written,
/// and removed when a dataset has no failures.
fn write_ng_report(filename: &str, results: &[ExperimentResult]) {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("ng");
    let path = dir.join(format!("{filename}.txt"));

    let report: String = results.iter().filter_map(|r| r.ng.as_deref()).collect();

    if report.is_empty() {
        let _ = std::fs::remove_file(&path);
        return;
    }

    std::fs::create_dir_all(&dir).expect("failed to create ng report directory");
    std::fs::write(&path, report).expect("failed to write ng report");
}

/// Runs experiments on a file and prints performance statistics
fn run_csv_tests(filename: &'static str, csv: &str) -> Vec<ExperimentResult> {
    let mut results: Vec<ExperimentResult> = vec![];

    for (line_nb, line) in csv.lines().enumerate() {
        if line.is_empty() {
            continue;
        }

        let expe = Experiment::new(filename, line, line_nb);
        results.push(expe.run());
    }

    let (q0, q1, q2, q3, q4) = quartiles(&results);
    let (oks, okzs, ngs) = count_types(&results);

    write_ng_report(filename, &results);

    println!(
        "RESULTS \"{}\" (count: {}{}):\n\t[{}, {}, {}, {}, {}]\n\tOK: {}\tOKZ: {}\tNG: {}\n",
        filename,
        results.len(),
        if patterns_enabled() {
            ""
        } else {
            ", patterns: off"
        },
        format_duration(q0),
        format_duration(q1),
        format_duration(q2),
        format_duration(q3),
        format_duration(q4),
        oks,
        okzs,
        ngs
    );

    results
}

macro_rules! run_on_dataset {
    ($filename: literal) => {
        run_csv_tests(
            $filename,
            include_str!(concat!("../../third_party/dataset/", $filename)),
        )
    };
}

/// Runs a dataset and fails if the number of unsolved cases (NG) exceeds
/// `max_ng`. The ceiling is the count observed on `master`; it is an upper
/// bound, so improvements keep passing and can be ratcheted down, while any
/// regression that turns a solved case into a failure trips the gate.
macro_rules! test_dataset {
    ($name:ident, $filename:literal, max_ng = $max_ng:expr $(, $attr:meta)?) => {
        #[test]
        $(#[$attr])?
        fn $name() {
            let results = run_on_dataset!($filename);
            let (_, _, ngs) = count_types(&results);
            assert!(
                ngs <= $max_ng,
                "{}: {} failing cases (NG), baseline ceiling is {}",
                $filename,
                ngs,
                $max_ng,
            );
        }
    };
}

#[test]
fn merges_semantically_equal_hidden_components() {
    let x = parse_expr("v0 - v1 + 2 * (v1 & -v0)").unwrap();
    let equivalent_x = parse_expr("v0 + v1 - 2 * (v1 & (v0 - 1))").unwrap();
    let common = parse_expr("-v1 + (v0 & v1)").unwrap();
    let difference = -(x & common.clone()) + (equivalent_x & common);

    assert_eq!(
        simplify::simplify_mba(difference, BIT_COUNT),
        Ok(Expr::zero())
    );
}

#[test]
fn uses_two_hidden_definitions_to_recognize_bitwise_expression() {
    let difference = parse_expr(
        "-v3 - (v3 & 2*v4 & (v3+v4)) + (v3 & 2*v4) + (v3 & (v3+v4)) \
         + (v3 & (-1 - 3*v4 - v3 + ((2*v4) & (v3+v4))))",
    )
    .unwrap();

    assert_eq!(
        simplify::simplify_mba(difference, BIT_COUNT),
        Ok(Expr::zero())
    );
}

#[test]
fn merges_semantically_complementary_hidden_components() {
    let difference = parse_expr(
        "-v0 + (v0 & (-1 - 3*v0 - v2 - (v2 & (-1 - 2*v0)))) \
         + (v0 & (2*v2 + 3*v0 - (v2 & 2*v0)))",
    )
    .unwrap();

    assert_eq!(
        simplify::simplify_mba(difference, BIT_COUNT),
        Ok(Expr::zero())
    );
}

#[test]
fn ignores_nonlinear_hidden_variables_when_selecting_lambda_relations() {
    let difference = parse_expr(
        "-(2*v2 & (v0+v2)) - (2*v2 & (v2*v2)) \
         + (2*v2 & (v0+v2) & (v2*v2)) \
         + (2*v2 & ((v0+v2) + ((-1-v0-v2) & (v2*v2))))",
    )
    .unwrap();

    assert_eq!(
        simplify::simplify_mba(difference, BIT_COUNT),
        Ok(Expr::zero())
    );
}

#[test]
fn recursively_proves_hidden_components_are_complements() {
    let difference = parse_expr(
        "-v3 \
         + (v3 & (-1 - v2*v3 - v3*v3 - v3*(v2 & (-1-v2-v3)))) \
         + (v3 & (2*v2*v3 - v3*(v2 & (v2+v3)) + v3*v3))",
    )
    .unwrap();

    assert_eq!(
        simplify::simplify_mba(difference, BIT_COUNT),
        Ok(Expr::zero())
    );
}

#[test]
fn discovers_proven_binary_relation_between_hidden_components() {
    // B = v4-v0-v3-(v4&-v0), C = ~(B|v0), and v5 is an arbitrary value.
    // The signatures nominate that relation, but its expanded definitions must
    // be proved equal before it is used.
    let difference = parse_expr(
        "-v5 \
         - (v0 & v5 & (v4-v0-v3-(v4 & -v0))) \
         + (v0 & v5) \
         + (v5 & (v3-1-(v4 & (v0-1))+(v0 & (-v0-v3+(v4 & (v0-1)))))) \
         + (v5 & (v4-v0-v3-(v4 & -v0)))",
    )
    .unwrap();

    assert_eq!(
        simplify::simplify_mba(difference, BIT_COUNT),
        Ok(Expr::zero())
    );
}

#[cfg(test)]
mod datasets {
    use super::*;

    test_dataset!(loki_tiny, "loki_tiny.csv", max_ng = 5);

    test_dataset!(mba_flatten, "mba_flatten.csv", max_ng = 0);

    test_dataset!(mba_obf_linear, "mba_obf_linear.csv", max_ng = 0);

    test_dataset!(mba_obf_nonlinear, "mba_obf_nonlinear.csv", max_ng = 0);

    test_dataset!(neureduce, "neureduce.csv", max_ng = 0);

    test_dataset!(qsynth_ea, "qsynth_ea.csv", max_ng = 0);

    test_dataset!(syntia, "syntia.csv", max_ng = 0);

    // #[test]
    // fn test_all() {
    //     let mut exps = run_on_dataset!("loki_tiny.csv");
    //     exps.extend(run_on_dataset!("mba_flatten.csv"));
    //     exps.extend(run_on_dataset!("mba_obf_linear.csv"));
    //     exps.extend(run_on_dataset!("mba_obf_nonlinear.csv"));
    //     exps.extend(run_on_dataset!("neureduce.csv"));
    //     exps.extend(run_on_dataset!("qsynth_ea.csv"));
    //     exps.extend(run_on_dataset!("syntia.csv"));

    //     let (q0, q1, q2, q3, q4) = quartiles(&exps);
    //     let s = exps.iter().map(|e| e.elapsed).sum();
    //     print!(
    //         " HERE [{} {} {} {} {}] avg: {} total: {}\n\n\n",
    //         format_duration(q0),
    //         format_duration(q1),
    //         format_duration(q2),
    //         format_duration(q3),
    //         format_duration(q4),
    //         format_duration(s),
    //         format_duration(s / (exps.len() as u32))
    //     )
    // }
}
