use std::time::{Duration, Instant};

use rumba_core::{
    expr::Expr,
    parser::parse_expr,
    simplify::simplify_mba,
    varint::make_mask,
};

const BIT_COUNT: u8 = 64;
const SEMANTIC_TEST_COUNT: usize = 200;
const TARGET_LINES: [usize; 5] = [53, 249, 260, 369, 481];
const QSYNTH_EA: &str = include_str!("../../third_party/dataset/qsynth_ea.csv");

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Status {
    Ok,
    OkZ,
    Ng,
}

impl Status {
    fn as_str(self) -> &'static str {
        match self {
            Self::Ok => "OK",
            Self::OkZ => "OKZ",
            Self::Ng => "NG",
        }
    }
}

struct ResultRow {
    line: usize,
    status: Status,
    elapsed: Duration,
    input_nodes: usize,
    output_nodes: Option<usize>,
}

fn parse_case(line_number: usize) -> (Expr, Expr) {
    let row = QSYNTH_EA
        .lines()
        .nth(line_number - 1)
        .unwrap_or_else(|| panic!("QSynth EA has no line {line_number}"));
    let (mba, ground_truth) = row
        .split_once(',')
        .unwrap_or_else(|| panic!("QSynth EA line {line_number} does not have two columns"));

    (
        parse_expr(mba.trim()).unwrap_or_else(|err| {
            panic!("failed to parse MBA at QSynth EA line {line_number}: {err}")
        }),
        parse_expr(ground_truth.trim()).unwrap_or_else(|err| {
            panic!("failed to parse ground truth at QSynth EA line {line_number}: {err}")
        }),
    )
}

fn classify(line: usize) -> ResultRow {
    let (mba, ground_truth) = parse_case(line);
    let input_nodes = mba.size();
    let start = Instant::now();
    let simplified = simplify_mba(mba, BIT_COUNT);
    let elapsed = start.elapsed();

    let Ok(simplified) = simplified else {
        return ResultRow {
            line,
            status: Status::Ng,
            elapsed,
            input_nodes,
            output_nodes: None,
        };
    };

    if let Err((vars, actual, expected)) =
        simplified.sem_equal(&ground_truth, make_mask(BIT_COUNT), SEMANTIC_TEST_COUNT)
    {
        panic!(
            "semantic mismatch at QSynth EA line {line}: vars={vars:?}, actual={actual}, expected={expected}"
        );
    }

    let output_nodes = Some(simplified.size());
    let status = match simplify_mba(ground_truth, BIT_COUNT) {
        Ok(simplified_ground_truth) => {
            if simplified == simplified_ground_truth {
                Status::Ok
            } else if simplify_mba(simplified_ground_truth - simplified, BIT_COUNT)
                == Ok(Expr::zero())
            {
                Status::OkZ
            } else {
                Status::Ng
            }
        }
        Err(_) => Status::Ng,
    };

    ResultRow {
        line,
        status,
        elapsed,
        input_nodes,
        output_nodes,
    }
}

fn main() {
    let started = Instant::now();
    let results: Vec<_> = TARGET_LINES.into_iter().map(classify).collect();

    for result in &results {
        let output_nodes = result
            .output_nodes
            .map_or_else(|| "rejected".to_owned(), |size| size.to_string());
        println!(
            "line={} status={} solve_us={:.2} input_nodes={} output_nodes={}",
            result.line,
            result.status.as_str(),
            result.elapsed.as_secs_f64() * 1_000_000.0,
            result.input_nodes,
            output_nodes,
        );
    }

    let ok = results.iter().filter(|row| row.status == Status::Ok).count();
    let okz = results
        .iter()
        .filter(|row| row.status == Status::OkZ)
        .count();
    let ng = results.iter().filter(|row| row.status == Status::Ng).count();

    println!(
        "target_summary count={} ok={} okz={} ng={} elapsed_ms={:.2}",
        results.len(),
        ok,
        okz,
        ng,
        started.elapsed().as_secs_f64() * 1_000.0,
    );
}
