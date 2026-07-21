use std::collections::BTreeMap;

use rumba_core::{
    expr::Expr,
    p8::experiment_pipeline,
    parser::parse_expr,
    simplify::{diagnose_hidden_atoms, simplify_mba},
    varint::make_mask,
};

const WIDTH: u8 = 64;
const DATASETS: [(&str, &str); 7] = [
    ("loki_tiny.csv", include_str!("../../third_party/dataset/loki_tiny.csv")),
    ("mba_flatten.csv", include_str!("../../third_party/dataset/mba_flatten.csv")),
    (
        "mba_obf_linear.csv",
        include_str!("../../third_party/dataset/mba_obf_linear.csv"),
    ),
    (
        "mba_obf_nonlinear.csv",
        include_str!("../../third_party/dataset/mba_obf_nonlinear.csv"),
    ),
    ("neureduce.csv", include_str!("../../third_party/dataset/neureduce.csv")),
    ("qsynth_ea.csv", include_str!("../../third_party/dataset/qsynth_ea.csv")),
    ("syntia.csv", include_str!("../../third_party/dataset/syntia.csv")),
];

fn sampled_equal(left: &Expr, right: &Expr) -> bool {
    let mask = make_mask(WIDTH);
    let max_var = left
        .get_vars()
        .into_iter()
        .chain(right.get_vars())
        .map(|variable| variable.0)
        .max()
        .unwrap_or(0);
    let mut state = 0x9e37_79b9_7f4a_7c15u64;
    for sample in 0..200 {
        let values = (0..=max_var)
            .map(|variable| {
                state ^= state << 7;
                state ^= state >> 9;
                state ^= state << 8;
                state
                    .wrapping_add((sample as u64) << 32)
                    .wrapping_add(variable as u64)
                    & mask
            })
            .collect::<Vec<_>>();
        if left.eval(&values).get(mask) != right.eval(&values).get(mask) {
            return false;
        }
    }
    true
}

fn main() {
    let mut cases = 0;
    let mut baseline_ng = 0;
    let mut p7e_resolved = 0;
    let mut p8a_resolved = 0;
    let mut p8b_resolved = 0;
    let mut p8c_resolved = 0;
    let mut regressions = 0;
    let mut changed_by_dataset = BTreeMap::<&str, usize>::new();
    let mut resolved_lines = Vec::new();

    for (dataset, csv) in DATASETS {
        for (index, row) in csv.lines().filter(|row| !row.trim().is_empty()).enumerate() {
            cases += 1;
            let (mba, ground_truth) = row.split_once(',').unwrap();
            let (Ok(mba), Ok(ground_truth)) = (
                simplify_mba(parse_expr(mba.trim()).unwrap(), WIDTH),
                simplify_mba(parse_expr(ground_truth.trim()).unwrap(), WIDTH),
            ) else {
                continue;
            };
            let residual = ground_truth - mba;
            if simplify_mba(residual.clone(), WIDTH) == Ok(Expr::zero()) {
                continue;
            }
            baseline_ng += 1;
            let Ok((diagnosed, trace)) = diagnose_hidden_atoms(residual, WIDTH) else {
                continue;
            };
            let Some(scope) = trace.iter().find(|scope| scope.input == diagnosed) else {
                continue;
            };
            let Some(pipeline) = experiment_pipeline(scope).unwrap() else {
                continue;
            };
            if !sampled_equal(&pipeline.input_after_p7e, &pipeline.result) {
                regressions += 1;
                continue;
            }
            let stage = if pipeline.p7e.residual_zero {
                p7e_resolved += 1;
                "p7e"
            } else if pipeline.after_p8a.result == Expr::zero() {
                p8a_resolved += 1;
                "p8a"
            } else if pipeline.after_p8b.result == Expr::zero() {
                p8b_resolved += 1;
                "p8b"
            } else if pipeline.after_p8c.result == Expr::zero() {
                p8c_resolved += 1;
                "p8c"
            } else {
                continue;
            };
            *changed_by_dataset.entry(dataset).or_default() += 1;
            resolved_lines.push(format!("{dataset}:{}:{stage}", index + 1));
        }
    }

    let resolved = p7e_resolved + p8a_resolved + p8b_resolved + p8c_resolved;
    println!("cases={cases}");
    println!("baseline_NG={baseline_ng}");
    println!("p7e_resolved={p7e_resolved}");
    println!("p8a_resolved={p8a_resolved}");
    println!("p8b_resolved={p8b_resolved}");
    println!("p8c_resolved={p8c_resolved}");
    println!("total_resolved={resolved}");
    println!("NG_remaining={}", baseline_ng - resolved);
    for (dataset, count) in changed_by_dataset {
        println!("dataset={dataset} resolved={count}");
    }
    println!("resolved_lines=[{}]", resolved_lines.join(","));
    println!("sampled_regressions={regressions}");
}
