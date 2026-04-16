#![cfg(feature = "parse")]

use std::time::{Duration, Instant};

use rumba_core::{expr::Expr, parser::parse_expr, simplify, varint::make_mask};

/// The number of semantic tests to run
const SEMANTIC_TEST_COUNT: usize = 200;

/// The bit count for the experiments
const BIT_COUNT: u8 = 64;

const MASK: u64 = make_mask(BIT_COUNT);

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
        let simplified_mba = simplify::simplify_mba(self.mba.clone(), BIT_COUNT);
        let elapsed = start.elapsed();

        if let Err((_, v1, v2)) = simplified_mba.sem_equal(&self.gt, MASK, SEMANTIC_TEST_COUNT) {
            assert_eq!(
                v1, v2,
                "{}:{} semantic error mba: {}, gt: {}",
                self.filename, self.line_nb, simplified_mba, self.gt
            );
        }

        let simplified_gt = simplify::simplify_mba(self.gt.clone(), BIT_COUNT);

        let mut status = Status::NG;

        if simplified_mba == simplified_gt {
            status = Status::Ok;
        } else if simplify::simplify_mba(simplified_gt - simplified_mba.clone(), BIT_COUNT)
            == Expr::zero()
        {
            status = Status::OkZ;
        } else {
            // println!("Solution: {}\n GT: {}\n\n", simplified_mba, self.gt);
        }

        ExperimentResult { elapsed, status }
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

    println!(
        "RESULTS \"{}\" (count: {}):\n\t[{}, {}, {}, {}, {}]\n\tOK: {}\tOKZ: {}\tNG: {}\n",
        filename,
        results.len(),
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

macro_rules! test_dataset {
    ($name:ident, $filename:literal $(, $attr:meta)?) => {
        #[test]
        $(#[$attr])?
        fn $name() {
            run_on_dataset!($filename);
        }
    };
}

#[cfg(test)]
mod datasets {
    use super::*;

    test_dataset!(loki_tiny, "loki_tiny.csv");

    test_dataset!(mba_flatten, "mba_flatten.csv");

    test_dataset!(mba_obf_linear, "mba_obf_linear.csv");

    test_dataset!(mba_obf_nonlinear, "mba_obf_nonlinear.csv");

    test_dataset!(neureduce, "neureduce.csv");

    test_dataset!(qsynth_ea, "qsynth_ea.csv");

    test_dataset!(syntia, "syntia.csv");

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
