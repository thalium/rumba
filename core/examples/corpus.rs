#![cfg(feature = "parse")]

#[path = "support/corpus.rs"]
mod corpus;
#[path = "support/table.rs"]
mod table;

use std::{
    fs,
    hint::black_box,
    io::{self, Write},
    path::PathBuf,
    time::{Duration, Instant},
};

use rumba_core::{expr::Expr, parser::parse_expr, simplify::simplify_mba};

use corpus::DATASETS;
use table::Alignment::{Left, Right};

const BIT_COUNT: u8 = 64;
const MEASURED_RUNS: usize = 5;
const SEMANTIC_TEST_COUNT: usize = 200;
const SNAPSHOT_VERSION: &str = "rumba-corpus-v1";

enum Status {
    Ok,
    OkZ,
    Ng,
}

#[derive(Clone, Copy, Default)]
struct Counts {
    total: usize,
    ok: usize,
    okz: usize,
    ng: usize,
    wins: usize,
    ties: usize,
    losses: usize,
    total_ast: usize,
    raw_ast: usize,
}

impl Counts {
    fn record(&mut self, status: &Status, actual_cost: usize, raw_cost: usize) {
        self.total += 1;
        self.total_ast += actual_cost;
        self.raw_ast += raw_cost;
        match actual_cost.cmp(&raw_cost) {
            std::cmp::Ordering::Less => self.wins += 1,
            std::cmp::Ordering::Equal => self.ties += 1,
            std::cmp::Ordering::Greater => self.losses += 1,
        }
        match status {
            Status::Ok => self.ok += 1,
            Status::OkZ => self.okz += 1,
            Status::Ng => self.ng += 1,
        }
    }

    fn merge(&mut self, other: Self) {
        self.total += other.total;
        self.ok += other.ok;
        self.okz += other.okz;
        self.ng += other.ng;
        self.wins += other.wins;
        self.ties += other.ties;
        self.losses += other.losses;
        self.total_ast += other.total_ast;
        self.raw_ast += other.raw_ast;
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Summary {
    count: usize,
    total: Duration,
    p50: Duration,
    p95: Duration,
    p99: Duration,
    max: Duration,
    max_case: String,
}

impl Summary {
    fn throughput(&self) -> f64 {
        self.count as f64 / self.total.as_secs_f64()
    }
}

#[derive(Debug, PartialEq, Eq)]
struct BenchmarkResult {
    global: Summary,
    datasets: Vec<(String, Summary)>,
}

struct BenchmarkRun {
    samples: Vec<(Duration, String)>,
    total: Duration,
}

#[derive(Default)]
struct Options {
    baseline: Option<PathBuf>,
    save: Option<PathBuf>,
}

fn parse_options() -> Options {
    let mut options = Options::default();
    let mut arguments = std::env::args_os().skip(1);
    while let Some(argument) = arguments.next() {
        match argument.to_str() {
            Some("--baseline") => {
                options.baseline = Some(PathBuf::from(
                    arguments
                        .next()
                        .expect("--baseline requires a snapshot path"),
                ));
            }
            Some("--save") => {
                options.save = Some(PathBuf::from(
                    arguments.next().expect("--save requires a snapshot path"),
                ));
            }
            _ => panic!("unknown argument: {}", argument.to_string_lossy()),
        }
    }
    options
}

fn percentile(sorted: &[(Duration, String)], percentile: usize) -> Duration {
    let rank = (percentile * sorted.len()).div_ceil(100);
    sorted[rank.saturating_sub(1)].0
}

fn summarize(mut samples: Vec<(Duration, String)>, total: Duration) -> Summary {
    assert!(!samples.is_empty());
    samples.sort_unstable_by_key(|sample| sample.0);
    let (max, max_case) = samples.last().unwrap().clone();
    Summary {
        count: samples.len(),
        total,
        p50: percentile(&samples, 50),
        p95: percentile(&samples, 95),
        p99: percentile(&samples, 99),
        max,
        max_case,
    }
}

fn run_benchmark_once(cases: &[(String, Expr)]) -> BenchmarkRun {
    let cases = cases
        .iter()
        .map(|(source, expression)| (source.clone(), expression.clone()))
        .collect::<Vec<_>>();
    let mut samples = Vec::with_capacity(cases.len());
    let dataset_started = Instant::now();
    for (source, expression) in cases {
        let started = Instant::now();
        let simplified = simplify_mba(expression, BIT_COUNT);
        let elapsed = started.elapsed();
        let _ = black_box(simplified);
        samples.push((elapsed, source.clone()));
    }
    BenchmarkRun {
        samples,
        total: dataset_started.elapsed(),
    }
}

fn median_duration(values: impl Iterator<Item = Duration>) -> Duration {
    let mut values = values.collect::<Vec<_>>();
    assert_eq!(values.len(), MEASURED_RUNS);
    values.sort_unstable();
    values[values.len() / 2]
}

fn median_summary(summaries: impl Iterator<Item = Summary>) -> Summary {
    let summaries = summaries.collect::<Vec<_>>();
    assert_eq!(summaries.len(), MEASURED_RUNS);
    assert!(
        summaries
            .iter()
            .all(|summary| summary.count == summaries[0].count)
    );

    let mut maxima = summaries
        .iter()
        .map(|summary| (summary.max, summary.max_case.clone()))
        .collect::<Vec<_>>();
    maxima.sort_unstable();
    let (max, max_case) = maxima[maxima.len() / 2].clone();

    Summary {
        count: summaries[0].count,
        total: median_duration(summaries.iter().map(|summary| summary.total)),
        p50: median_duration(summaries.iter().map(|summary| summary.p50)),
        p95: median_duration(summaries.iter().map(|summary| summary.p95)),
        p99: median_duration(summaries.iter().map(|summary| summary.p99)),
        max,
        max_case,
    }
}

fn run_benchmark(cases: &[(String, Expr)]) -> (Summary, Vec<BenchmarkRun>) {
    let runs = (0..MEASURED_RUNS)
        .map(|_| run_benchmark_once(cases))
        .collect::<Vec<_>>();
    let summary = median_summary(
        runs.iter()
            .map(|run| summarize(run.samples.clone(), run.total)),
    );
    (summary, runs)
}

fn format_duration(duration: Duration) -> String {
    let nanos = duration.as_nanos();
    if nanos >= 1_000_000_000 {
        format!("{:.2} s", duration.as_secs_f64())
    } else if nanos >= 100_000_000 {
        format!("{:.0} ms", duration.as_secs_f64() * 1_000.0)
    } else if nanos >= 10_000_000 {
        format!("{:.1} ms", duration.as_secs_f64() * 1_000.0)
    } else if nanos >= 1_000_000 {
        format!("{:.2} ms", duration.as_secs_f64() * 1_000.0)
    } else if nanos >= 1_000 {
        format!("{:.0} µs", duration.as_secs_f64() * 1_000_000.0)
    } else {
        format!("{nanos} ns")
    }
}

fn classify(source: &str, mba: Expr, ground_truth: Expr, simplified: &Expr) -> Status {
    if let Err((variables, actual, expected)) =
        simplified.sem_equal(&ground_truth, BIT_COUNT, SEMANTIC_TEST_COUNT)
    {
        panic!(
            "semantic mismatch at {source}: vars={variables:?}, actual={actual}, expected={expected}"
        );
    }

    let simplified_ground_truth = match simplify_mba(ground_truth.clone(), BIT_COUNT) {
        Ok(simplified) => simplified,
        Err(_) => return Status::Ng,
    };

    if *simplified == simplified_ground_truth {
        Status::Ok
    } else {
        match simplify_mba((mba - ground_truth).reduce(BIT_COUNT), BIT_COUNT) {
            Ok(residual) if residual == Expr::zero() => Status::OkZ,
            Ok(_) | Err(_) => Status::Ng,
        }
    }
}

fn run_quality(rows: &[corpus::CorpusRow<'_>]) -> Counts {
    let mut counts = Counts::default();
    for row in rows {
        let mba = parse_expr(row.mba)
            .unwrap_or_else(|error| panic!("failed to parse {}: {error}", row.source));
        let ground_truth = parse_expr(row.ground_truth)
            .unwrap_or_else(|error| panic!("failed to parse {}: {error}", row.source));
        let raw_cost = ground_truth.size();
        let simplified = simplify_mba(mba.clone(), BIT_COUNT);
        let (status, actual_cost) = match simplified {
            Ok(simplified) => {
                let actual_cost = simplified.size();
                (
                    classify(&row.source, mba, ground_truth, &simplified),
                    actual_cost,
                )
            }
            Err(_) => (Status::Ng, mba.size()),
        };
        counts.record(&status, actual_cost, raw_cost);
    }
    counts
}

const QUALITY_HEADERS: [&str; 10] = [
    "Dataset",
    "Total",
    "OK",
    "OKZ",
    "NG",
    "Win",
    "Tied",
    "Loss",
    "Actual AST",
    "Raw AST",
];
const QUALITY_ALIGNMENTS: [table::Alignment; 10] = [
    Left, Right, Right, Right, Right, Right, Right, Right, Right, Right,
];
const PERFORMANCE_HEADERS: [&str; 7] = ["Dataset", "Time", "Expr/s", "p50", "p95", "p99", "Max"];
const PERFORMANCE_ALIGNMENTS: [table::Alignment; 7] =
    [Left, Right, Right, Right, Right, Right, Right];

fn dataset_width() -> usize {
    DATASETS
        .iter()
        .map(|dataset| dataset.name.chars().count())
        .chain(["Dataset".len(), "**Total**".len()])
        .max()
        .expect("corpus list is not empty")
}

fn quality_widths() -> [usize; 10] {
    let expression_count = DATASETS
        .iter()
        .map(|dataset| {
            dataset
                .contents
                .lines()
                .filter(|line| !line.trim().is_empty())
                .count()
        })
        .sum::<usize>();
    let count_width = expression_count.to_string().len().max("Total".len());
    [
        dataset_width(),
        count_width,
        count_width,
        count_width,
        count_width,
        count_width,
        count_width,
        count_width,
        "Actual AST".len(),
        "Raw AST".len(),
    ]
}

fn performance_widths() -> [usize; 7] {
    [dataset_width(), 8, 6, 8, 8, 8, 8]
}

fn print_quality_header(widths: &[usize; 10]) {
    println!("## Quality\n");
    print!(
        "{}",
        table::render_header(&QUALITY_HEADERS, widths, &QUALITY_ALIGNMENTS)
    );
}

fn print_quality_row(name: &str, counts: Counts, widths: &[usize; 10]) {
    let cells = vec![
        name.to_owned(),
        counts.total.to_string(),
        counts.ok.to_string(),
        counts.okz.to_string(),
        counts.ng.to_string(),
        counts.wins.to_string(),
        counts.ties.to_string(),
        counts.losses.to_string(),
        counts.total_ast.to_string(),
        counts.raw_ast.to_string(),
    ];
    print!("{}", table::render_row(&cells, widths, &QUALITY_ALIGNMENTS));
    io::stdout().flush().expect("failed to flush corpus report");
}

fn print_performance_header(widths: &[usize; 7]) {
    println!("\n## Performance\n");
    println!("Runs: {MEASURED_RUNS} measured per dataset (quality pass used as warm-up)\n");
    print!(
        "{}",
        table::render_header(&PERFORMANCE_HEADERS, widths, &PERFORMANCE_ALIGNMENTS)
    );
}

fn print_performance_row(name: &str, summary: &Summary, widths: &[usize; 7]) {
    let cells = vec![
        name.to_owned(),
        format_duration(summary.total),
        format!("{:.0}", summary.throughput().min(999_999.0)),
        format_duration(summary.p50),
        format_duration(summary.p95),
        format_duration(summary.p99),
        format_duration(summary.max),
    ];
    print!(
        "{}",
        table::render_row(&cells, widths, &PERFORMANCE_ALIGNMENTS)
    );
    io::stdout().flush().expect("failed to flush corpus report");
}

fn duration_from_nanos(value: &str) -> Duration {
    Duration::from_nanos(value.parse().expect("invalid nanosecond value"))
}

fn serialize(result: &BenchmarkResult) -> String {
    let global = &result.global;
    let mut output = format!(
        "{SNAPSHOT_VERSION}\nglobal\t{}\t{}\t{}\t{}\t{}\t{}\t{}\n",
        global.count,
        global.total.as_nanos(),
        global.p50.as_nanos(),
        global.p95.as_nanos(),
        global.p99.as_nanos(),
        global.max.as_nanos(),
        global.max_case,
    );
    for (dataset, summary) in &result.datasets {
        output.push_str(&format!(
            "dataset\t{dataset}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\n",
            summary.count,
            summary.total.as_nanos(),
            summary.p50.as_nanos(),
            summary.p95.as_nanos(),
            summary.p99.as_nanos(),
            summary.max.as_nanos(),
            summary.max_case,
        ));
    }
    output
}

fn deserialize(contents: &str) -> BenchmarkResult {
    let mut lines = contents.lines();
    assert_eq!(lines.next(), Some(SNAPSHOT_VERSION), "unsupported snapshot");
    let fields = lines
        .next()
        .expect("missing global snapshot row")
        .split('\t')
        .collect::<Vec<_>>();
    assert_eq!(fields.len(), 8, "invalid global snapshot row");
    assert_eq!(fields[0], "global");
    let global = Summary {
        count: fields[1].parse().expect("invalid expression count"),
        total: duration_from_nanos(fields[2]),
        p50: duration_from_nanos(fields[3]),
        p95: duration_from_nanos(fields[4]),
        p99: duration_from_nanos(fields[5]),
        max: duration_from_nanos(fields[6]),
        max_case: fields[7].to_owned(),
    };
    let datasets = lines
        .map(|line| {
            let fields = line.split('\t').collect::<Vec<_>>();
            assert_eq!(fields.len(), 9, "invalid dataset snapshot row");
            assert_eq!(fields[0], "dataset");
            (
                fields[1].to_owned(),
                Summary {
                    count: fields[2].parse().expect("invalid dataset count"),
                    total: duration_from_nanos(fields[3]),
                    p50: duration_from_nanos(fields[4]),
                    p95: duration_from_nanos(fields[5]),
                    p99: duration_from_nanos(fields[6]),
                    max: duration_from_nanos(fields[7]),
                    max_case: fields[8].to_owned(),
                },
            )
        })
        .collect();
    BenchmarkResult { global, datasets }
}

fn delta(baseline: f64, candidate: f64) -> f64 {
    100.0 * (candidate - baseline) / baseline
}

fn print_comparison(baseline: &BenchmarkResult, candidate: &BenchmarkResult) {
    assert_eq!(baseline.global.count, candidate.global.count);
    let baseline_global = &baseline.global;
    let candidate_global = &candidate.global;
    let rows = [
        ("Total time", baseline_global.total, candidate_global.total),
        ("p50", baseline_global.p50, candidate_global.p50),
        ("p95", baseline_global.p95, candidate_global.p95),
        ("p99", baseline_global.p99, candidate_global.p99),
        ("Max", baseline_global.max, candidate_global.max),
    ]
    .map(|(metric, baseline, candidate)| {
        vec![
            metric.to_owned(),
            format_duration(baseline),
            format_duration(candidate),
            format!(
                "{:+.2}%",
                delta(baseline.as_secs_f64(), candidate.as_secs_f64())
            ),
        ]
    })
    .to_vec();
    let mut rows = rows;
    rows.push(vec![
        "Expr/s".to_owned(),
        format!("{:.0}", baseline_global.throughput()),
        format!("{:.0}", candidate_global.throughput()),
        format!(
            "{:+.2}%",
            delta(baseline_global.throughput(), candidate_global.throughput())
        ),
    ]);
    println!("\n## Comparison\n");
    print!(
        "{}",
        table::render(
            &["Metric", "Baseline", "Candidate", "Delta %"],
            &rows,
            &[Left, Right, Right, Right],
        )
    );
    println!(
        "\nBaseline max  : {} — {}",
        format_duration(baseline_global.max),
        baseline_global.max_case
    );
    println!(
        "Candidate max : {} — {}",
        format_duration(candidate_global.max),
        candidate_global.max_case
    );

    let mut rows = Vec::new();
    for (dataset, candidate) in &candidate.datasets {
        let baseline = baseline
            .datasets
            .iter()
            .find(|(name, _)| name == dataset)
            .unwrap_or_else(|| panic!("baseline is missing {dataset}"));
        for (metric, baseline, candidate) in [
            ("Total time", baseline.1.total, candidate.total),
            ("p95", baseline.1.p95, candidate.p95),
            ("p99", baseline.1.p99, candidate.p99),
        ] {
            rows.push(vec![
                dataset.clone(),
                metric.to_owned(),
                format_duration(baseline),
                format_duration(candidate),
                format!(
                    "{:+.2}%",
                    delta(baseline.as_secs_f64(), candidate.as_secs_f64())
                ),
            ]);
        }
    }
    println!();
    print!(
        "{}",
        table::render(
            &["Dataset", "Metric", "Baseline", "Candidate", "Delta %"],
            &rows,
            &[Left, Left, Right, Right, Right],
        )
    );
}

fn main() {
    let options = parse_options();
    let baseline = options.baseline.as_ref().map(|path| {
        deserialize(
            &fs::read_to_string(path)
                .unwrap_or_else(|error| panic!("failed to read {}: {error}", path.display())),
        )
    });

    let quality_widths = quality_widths();
    print_quality_header(&quality_widths);
    let mut global_counts = Counts::default();
    for dataset in DATASETS {
        let rows = corpus::rows(dataset.name, dataset.contents);
        let counts = run_quality(&rows);
        global_counts.merge(counts);
        print_quality_row(dataset.name, counts, &quality_widths);
    }
    print_quality_row("**Total**", global_counts, &quality_widths);

    let performance_widths = performance_widths();
    print_performance_header(&performance_widths);
    let mut global_runs = (0..MEASURED_RUNS)
        .map(|_| BenchmarkRun {
            samples: Vec::new(),
            total: Duration::ZERO,
        })
        .collect::<Vec<_>>();
    let mut datasets = Vec::with_capacity(DATASETS.len());

    for dataset in DATASETS {
        let rows = corpus::rows(dataset.name, dataset.contents);
        let cases = rows
            .iter()
            .map(|row| {
                let expression = parse_expr(row.mba)
                    .unwrap_or_else(|error| panic!("failed to parse {}: {error}", row.source));
                (row.source.clone(), expression)
            })
            .collect::<Vec<_>>();
        let (summary, runs) = run_benchmark(&cases);
        for (global, run) in global_runs.iter_mut().zip(runs) {
            global.total += run.total;
            global.samples.extend(run.samples);
        }
        print_performance_row(dataset.name, &summary, &performance_widths);
        datasets.push((dataset.name.to_owned(), summary));
    }

    let global = median_summary(
        global_runs
            .into_iter()
            .map(|run| summarize(run.samples, run.total)),
    );
    print_performance_row("**Total**", &global, &performance_widths);
    println!(
        "\nSlowest median maximum: {} — {}",
        format_duration(global.max),
        global.max_case
    );
    let candidate = BenchmarkResult { global, datasets };

    if let Some(baseline) = baseline {
        print_comparison(&baseline, &candidate);
    }
    if let Some(path) = options.save {
        fs::write(&path, serialize(&candidate))
            .unwrap_or_else(|error| panic!("failed to write {}: {error}", path.display()));
        println!("\nSnapshot saved: {}", path.display());
    }
}

#[cfg(test)]
mod tests {
    use super::{
        BenchmarkResult, Counts, Status, Summary, deserialize, format_duration, median_summary,
        serialize,
    };
    use std::time::Duration;

    fn summary(millis: u64, max_case: &str) -> Summary {
        let duration = Duration::from_millis(millis);
        Summary {
            count: 41_000,
            total: duration,
            p50: duration,
            p95: duration,
            p99: duration,
            max: duration,
            max_case: max_case.to_owned(),
        }
    }

    #[test]
    fn selects_the_median_of_five_measured_runs() {
        let median = median_summary(
            [
                summary(50, "slowest"),
                summary(10, "fast"),
                summary(30, "middle"),
                summary(20, "faster"),
                summary(40, "slower"),
            ]
            .into_iter(),
        );

        assert_eq!(median.total, Duration::from_millis(30));
        assert_eq!(median.max_case, "middle");
    }

    #[test]
    fn formats_compact_numeric_columns() {
        assert_eq!(format_duration(Duration::from_micros(129_700)), "130 ms");
        assert_eq!(format_duration(Duration::from_micros(4_330)), "4.33 ms");
    }

    #[test]
    fn aggregates_ast_comparisons_and_totals() {
        let mut counts = Counts::default();
        counts.record(&Status::Ok, 2, 3);
        counts.record(&Status::OkZ, 4, 4);
        counts.record(&Status::Ng, 6, 5);

        assert_eq!(counts.total, 3);
        assert_eq!(counts.wins, 1);
        assert_eq!(counts.ties, 1);
        assert_eq!(counts.losses, 1);
        assert_eq!(counts.total_ast, 12);
        assert_eq!(counts.raw_ast, 12);
    }

    #[test]
    fn snapshot_round_trip_preserves_every_summary_field() {
        let result = BenchmarkResult {
            global: summary(10, "global:1"),
            datasets: vec![("dataset".to_owned(), summary(20, "dataset:2"))],
        };

        assert_eq!(deserialize(&serialize(&result)), result);
    }
}
