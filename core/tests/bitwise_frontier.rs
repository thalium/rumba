#![cfg(feature = "parse")]

use rumba_core::{expr::Expr, parser::parse_expr, simplify::simplify_mba};

const BIT_COUNT: u8 = 64;
const QSYNTH_EA: &str = include_str!("../../third_party/dataset/qsynth_ea.csv");

fn var(id: usize) -> Expr {
    Expr::Var(id.into())
}

fn bitwise_product_kernel(a: Expr, b: Expr) -> Expr {
    (a.clone() & b.clone()) * (a.clone() | b.clone())
        + (a.clone() & !b.clone()) * (!a & b)
}

fn assert_simplifies_to_zero(expression: Expr) {
    let simplified = simplify_mba(expression, BIT_COUNT)
        .unwrap_or_else(|err| panic!("solver rejected regression expression: {err}"));

    assert_eq!(simplified, Expr::zero(), "residual: {simplified}");
}

fn assert_kernel_simplifies(a: Expr, b: Expr) {
    let expected = a.clone() * b.clone();
    assert_simplifies_to_zero(bitwise_product_kernel(a, b) - expected);
}

fn qsynth_case(line_number: usize) -> (Expr, Expr) {
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

fn assert_qsynth_case_is_resolved(line_number: usize) {
    let (mba, ground_truth) = qsynth_case(line_number);
    let simplified_mba = simplify_mba(mba, BIT_COUNT)
        .unwrap_or_else(|err| panic!("solver rejected MBA at QSynth EA line {line_number}: {err}"));
    let simplified_ground_truth = simplify_mba(ground_truth, BIT_COUNT).unwrap_or_else(|err| {
        panic!("solver rejected ground truth at QSynth EA line {line_number}: {err}")
    });

    assert_simplifies_to_zero(simplified_ground_truth - simplified_mba);
}

#[test]
fn bitwise_product_kernel_identity_is_exact_for_widths_one_through_eight() {
    for bits in 1..=8 {
        let mask = (1u64 << bits) - 1;

        for a in 0..=mask {
            for b in 0..=mask {
                let not_a = !a & mask;
                let not_b = !b & mask;
                let kernel = (a & b)
                    .wrapping_mul(a | b)
                    .wrapping_add((a & not_b).wrapping_mul(not_a & b))
                    & mask;
                let expected = a.wrapping_mul(b) & mask;

                assert_eq!(
                    kernel, expected,
                    "identity failed for bits={bits}, a={a:#x}, b={b:#x}"
                );
            }
        }
    }
}

#[test]
fn simplifies_generic_bitwise_product_kernel() {
    assert_kernel_simplifies(var(0), var(1));
}

#[test]
fn simplifies_kernel_with_add_or_operand() {
    let x = var(0);
    let y = var(1);
    let z = var(2);
    let a = x.clone() + (y | x);

    assert_kernel_simplifies(a, z);
}

#[test]
fn simplifies_kernel_with_or_add_operand() {
    let x = var(0);
    let y = var(1);
    let a = x.clone() | (x + y.clone());

    assert_kernel_simplifies(a, y);
}

#[test]
fn simplifies_kernel_with_polynomial_and_arithmetic_operands() {
    let x = var(0);
    let y = var(1);
    let z = var(2);
    let polynomial = x.clone() * y.clone() + z;
    let arithmetic = 3 * x + u64::MAX * y;

    assert_kernel_simplifies(polynomial, arithmetic);
}

macro_rules! qsynth_regression {
    ($name:ident, $line:literal) => {
        #[test]
        #[ignore = "known NG: P6 found unresolved arithmetic hidden-atom dependencies"]
        fn $name() {
            assert_qsynth_case_is_resolved($line);
        }
    };
}

qsynth_regression!(resolves_qsynth_ea_line_53, 53);
qsynth_regression!(resolves_qsynth_ea_line_249, 249);
qsynth_regression!(resolves_qsynth_ea_line_260, 260);
qsynth_regression!(resolves_qsynth_ea_line_369, 369);
qsynth_regression!(resolves_qsynth_ea_line_481, 481);
