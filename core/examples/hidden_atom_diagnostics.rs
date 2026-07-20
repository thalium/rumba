use std::collections::BTreeSet;

use rumba_core::{
    expr::Expr,
    parser::parse_expr,
    simplify::{
        HiddenAtomDependencyKind, HiddenScopeTrace, diagnose_hidden_atoms,
    },
};

const BIT_COUNT: u8 = 64;
const TARGET_LINES: [usize; 5] = [53, 249, 260, 369, 481];
const QSYNTH_EA: &str = include_str!("../../third_party/dataset/qsynth_ea.csv");

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

fn print_trace(label: &str, result: &Expr, scopes: &[HiddenScopeTrace], verbose: bool) {
    let atoms = scopes.iter().flat_map(|scope| &scope.atoms);
    let total = atoms.clone().count();
    let roots = atoms
        .clone()
        .filter(|atom| atom.dependency_kind == HiddenAtomDependencyKind::Root)
        .count();
    let bitwise = atoms
        .clone()
        .filter(|atom| atom.dependency_kind == HiddenAtomDependencyKind::BitwiseDependent)
        .count();
    let arithmetic = atoms
        .clone()
        .filter(|atom| atom.dependency_kind == HiddenAtomDependencyKind::ArithmeticDependent)
        .count();
    let lowbit = atoms
        .clone()
        .filter(|atom| atom.dependency_kind == HiddenAtomDependencyKind::LowBitCandidate)
        .count();
    let free_roots: BTreeSet<_> = atoms
        .clone()
        .flat_map(|atom| atom.free_atoms.iter().copied())
        .collect();

    println!(
        "stage={label} result_nodes={} scopes={} atoms={} roots={} bitwise_dependencies={} arithmetic_dependencies={} lowbit_candidates={} free_roots={:?}",
        result.size(),
        scopes.len(),
        total,
        roots,
        bitwise,
        arithmetic,
        lowbit,
        free_roots,
    );

    for scope in scopes {
        if !verbose {
            continue;
        }

        println!(
            "  scope={} width={} input_nodes={} atoms={}",
            scope.scope,
            scope.bit_width,
            scope.input.size(),
            scope.atoms.len()
        );
        for atom in &scope.atoms {
            println!(
                "    atom=v{} kind={:?} free={:?} dependent={:?} dependency_definition={:?} original={} simplified={}",
                atom.atom,
                atom.dependency_kind,
                atom.free_atoms,
                atom.dependent_atoms,
                atom.dependency_definition,
                atom.original,
                atom.simplified,
            );
        }
    }
}

fn main() {
    let verbose = std::env::args().any(|arg| arg == "--verbose");

    for line in TARGET_LINES {
        let (mba, ground_truth) = parse_case(line);
        let (simplified_mba, mba_trace) = diagnose_hidden_atoms(mba, BIT_COUNT)
            .unwrap_or_else(|err| panic!("MBA diagnosis failed at line {line}: {err}"));
        let (simplified_gt, gt_trace) = diagnose_hidden_atoms(ground_truth, BIT_COUNT)
            .unwrap_or_else(|err| panic!("ground-truth diagnosis failed at line {line}: {err}"));
        let (residual, residual_trace) = diagnose_hidden_atoms(
            simplified_gt.clone() - simplified_mba.clone(),
            BIT_COUNT,
        )
        .unwrap_or_else(|err| panic!("residual diagnosis failed at line {line}: {err}"));

        println!("line={line} residual_zero={}", residual == Expr::zero());
        print_trace("mba", &simplified_mba, &mba_trace, verbose);
        print_trace("gt", &simplified_gt, &gt_trace, verbose);
        print_trace("residual", &residual, &residual_trace, verbose);
    }
}
