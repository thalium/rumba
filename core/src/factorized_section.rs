//! Factorized Sections direct-base certification.
use std::collections::{BTreeMap, VecDeque};

use crate::{
    expr::{Expr, VarId},
    varint::make_mask,
};

const WIDTH: u8 = 64;
const MAX_SOURCE_NODES: usize = 4096;
const MAX_SOURCE_DEPTH: usize = 256;
const MAX_INPUT_SLOTS: usize = MAX_SOURCE_NODES;
const MAX_LINEAR_ADD_DIMENSION: usize = 512;
const MAX_LINEAR_ADD_BASIS_VECTORS: usize = 512;
const MAX_LINEAR_ADD_OPERATIONS: usize = 50_000_000;

#[derive(Clone, Debug, PartialEq, Eq)]
struct Rational {
    numerator: i128,
    denominator: i128,
}

impl Rational {
    fn new(mut numerator: i128, mut denominator: i128) -> Result<Self, FactorizedSectionRefusal> {
        if denominator == 0 {
            return Err(FactorizedSectionRefusal::ArithmeticOverflow);
        }
        if denominator < 0 {
            numerator = numerator
                .checked_neg()
                .ok_or(FactorizedSectionRefusal::ArithmeticOverflow)?;
            denominator = denominator
                .checked_neg()
                .ok_or(FactorizedSectionRefusal::ArithmeticOverflow)?;
        }
        let mut left = numerator
            .checked_abs()
            .ok_or(FactorizedSectionRefusal::ArithmeticOverflow)?;
        let mut right = denominator;
        while right != 0 {
            let remainder = left % right;
            left = right;
            right = remainder;
        }
        let divisor = left.max(1);
        Ok(Self {
            numerator: numerator / divisor,
            denominator: denominator / divisor,
        })
    }

    fn integer(value: i128) -> Self {
        Self {
            numerator: value,
            denominator: 1,
        }
    }

    fn add(&self, other: &Self) -> Result<Self, FactorizedSectionRefusal> {
        Self::new(
            self.numerator
                .checked_mul(other.denominator)
                .and_then(|left| {
                    other
                        .numerator
                        .checked_mul(self.denominator)
                        .and_then(|right| left.checked_add(right))
                })
                .ok_or(FactorizedSectionRefusal::ArithmeticOverflow)?,
            self.denominator
                .checked_mul(other.denominator)
                .ok_or(FactorizedSectionRefusal::ArithmeticOverflow)?,
        )
    }

    fn subtract(&self, other: &Self) -> Result<Self, FactorizedSectionRefusal> {
        Self::new(
            self.numerator
                .checked_mul(other.denominator)
                .and_then(|left| {
                    other
                        .numerator
                        .checked_mul(self.denominator)
                        .and_then(|right| left.checked_sub(right))
                })
                .ok_or(FactorizedSectionRefusal::ArithmeticOverflow)?,
            self.denominator
                .checked_mul(other.denominator)
                .ok_or(FactorizedSectionRefusal::ArithmeticOverflow)?,
        )
    }

    fn multiply(&self, other: &Self) -> Result<Self, FactorizedSectionRefusal> {
        Self::new(
            self.numerator
                .checked_mul(other.numerator)
                .ok_or(FactorizedSectionRefusal::ArithmeticOverflow)?,
            self.denominator
                .checked_mul(other.denominator)
                .ok_or(FactorizedSectionRefusal::ArithmeticOverflow)?,
        )
    }

    fn divide(&self, other: &Self) -> Result<Self, FactorizedSectionRefusal> {
        if other.numerator == 0 {
            return Err(FactorizedSectionRefusal::ArithmeticOverflow);
        }
        Self::new(
            self.numerator
                .checked_mul(other.denominator)
                .ok_or(FactorizedSectionRefusal::ArithmeticOverflow)?,
            self.denominator
                .checked_mul(other.numerator)
                .ok_or(FactorizedSectionRefusal::ArithmeticOverflow)?,
        )
    }

    fn is_zero(&self) -> bool {
        self.numerator == 0
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct SectionScalars {
    r0: i128,
    epsilon: [i128; 4],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum FactorizedSectionRefusal {
    InvalidWidth,
    InvalidVariables,
    SourceBudgetExceeded,
    UnsupportedNode,
    UnsupportedMultiplication,
    ArithmeticOverflow,
    InternalGaugeViolation,
    MultipleSemanticBases,
    CandidateMismatch,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Base {
    epsilon_nonzero: [i128; 3],
    tau: [usize; 4],
}

impl Base {
    #[inline]
    fn epsilon(&self, symbol: usize) -> i128 {
        match symbol {
            0 => 0,
            1..=3 => self.epsilon_nonzero[symbol - 1],
            _ => unreachable!("FSC symbols are restricted to 0..4"),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct BaseGraph {
    bases: Vec<Base>,
    root: usize,
    r0: i128,
}

/// Exact FSC-2 coordinates for a stationary one-base source.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct OneBaseDescriptor {
    pub(crate) variables: [VarId; 2],
    pub(crate) r0: i128,
    pub(crate) epsilon_nonzero: [i128; 3],
    pub(crate) direct: bool,
}

#[derive(Clone, Copy)]
enum OneBasePartial {
    Stationary([u8; 4]),
    Exact { r0: i128, epsilon: [i128; 3] },
    Unknown,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct DigitCursor {
    base: usize,
    carry: i128,
}

#[derive(Clone, Copy, Debug, Default)]
struct SourceAnalysis {
    variables: [Option<VarId>; 2],
    variable_count: usize,
    node_count: usize,
    contains_multiplication: bool,
}

fn variables_valid(variables: [VarId; 2]) -> bool {
    variables[0] < variables[1]
}

fn analyze_source(expression: &Expr) -> Result<SourceAnalysis, FactorizedSectionRefusal> {
    fn visit(
        expression: &Expr,
        depth: usize,
        analysis: &mut SourceAnalysis,
    ) -> Result<(), FactorizedSectionRefusal> {
        if depth >= MAX_SOURCE_DEPTH {
            return Err(FactorizedSectionRefusal::SourceBudgetExceeded);
        }
        analysis.node_count = analysis
            .node_count
            .checked_add(1)
            .ok_or(FactorizedSectionRefusal::SourceBudgetExceeded)?;
        if analysis.node_count > MAX_SOURCE_NODES {
            return Err(FactorizedSectionRefusal::SourceBudgetExceeded);
        }
        match expression {
            Expr::Var(variable) => {
                if !analysis.variables[..analysis.variable_count.min(2)].contains(&Some(*variable))
                {
                    if analysis.variable_count < 2 {
                        analysis.variables[analysis.variable_count] = Some(*variable);
                    }
                    analysis.variable_count += 1;
                    if analysis.variable_count > 2 {
                        return Err(FactorizedSectionRefusal::InvalidVariables);
                    }
                }
            }
            Expr::Mul(children) => {
                analysis.contains_multiplication = true;
                for child in children {
                    visit(child, depth + 1, analysis)?;
                }
            }
            Expr::Not(child) | Expr::Scale(_, child) => visit(child, depth + 1, analysis)?,
            Expr::And(children)
            | Expr::Or(children)
            | Expr::Xor(children)
            | Expr::Add(children) => {
                for child in children {
                    visit(child, depth + 1, analysis)?;
                }
            }
            Expr::Const(_) => {}
        }
        Ok(())
    }

    let mut analysis = SourceAnalysis::default();
    visit(expression, 0, &mut analysis)?;
    Ok(analysis)
}

fn analyzed_variables(analysis: &SourceAnalysis) -> Option<[VarId; 2]> {
    if analysis.variable_count != 2 {
        return None;
    }
    let [Some(first), Some(second)] = analysis.variables else {
        return None;
    };
    let mut variables = [first, second];
    variables.sort_unstable();
    Some(variables)
}

fn boolean_coordinates(
    expression: &Expr,
    variables: [VarId; 2],
    width: u8,
) -> Result<[u64; 4], FactorizedSectionRefusal> {
    let max_variable = variables[0].0.max(variables[1].0);
    let input_len = max_variable
        .checked_add(1)
        .ok_or(FactorizedSectionRefusal::SourceBudgetExceeded)?;
    if input_len > MAX_INPUT_SLOTS {
        return Err(FactorizedSectionRefusal::SourceBudgetExceeded);
    }
    let mut inputs = vec![0; input_len];
    let mut coordinates = [0; 4];
    for (assignment, coordinate) in coordinates.iter_mut().enumerate() {
        inputs[variables[0].0] = u64::from(assignment & 1 != 0);
        inputs[variables[1].0] = u64::from(assignment & 2 != 0);
        *coordinate = expression.eval(&inputs, width) & make_mask(width);
    }
    Ok(coordinates)
}

fn signed_word(value: u64) -> i128 {
    i128::from(value as i64)
}

fn normalize_scalars(graph: BaseGraph) -> Result<SectionScalars, FactorizedSectionRefusal> {
    if graph.bases.len() != 1 || graph.root != 0 {
        return Err(FactorizedSectionRefusal::MultipleSemanticBases);
    }
    let base = &graph.bases[0];
    Ok(SectionScalars {
        r0: graph.r0,
        epsilon: [
            0,
            base.epsilon_nonzero[0],
            base.epsilon_nonzero[1],
            base.epsilon_nonzero[2],
        ],
    })
}

fn compile_single_base_scalars(
    source: &Expr,
    variables: [VarId; 2],
) -> Result<SectionScalars, FactorizedSectionRefusal> {
    if !variables_valid(variables) {
        return Err(FactorizedSectionRefusal::InvalidVariables);
    }
    normalize_scalars(compile_graph(source, variables)?)
}

fn local_stationary(expression: &Expr) -> bool {
    match expression {
        Expr::Var(_) => true,
        Expr::Const(value) => matches!(*value, 0 | u64::MAX),
        Expr::Not(child) => local_stationary(child),
        Expr::And(children) | Expr::Or(children) | Expr::Xor(children) => {
            children.iter().all(local_stationary)
        }
        Expr::Add(_) | Expr::Mul(_) | Expr::Scale(_, _) => false,
    }
}

fn canonical_xor(children: &[Expr]) -> Expr {
    fn collect(expression: &Expr, constant: &mut u64, parity: &mut BTreeMap<Expr, bool>) {
        match expression {
            Expr::Xor(children) => {
                for child in children {
                    collect(child, constant, parity);
                }
            }
            Expr::Const(value) => *constant ^= *value,
            expression => {
                let entry = parity.entry(expression.clone()).or_insert(false);
                *entry = !*entry;
            }
        }
    }

    let mut constant = 0u64;
    let mut parity = BTreeMap::new();
    for child in children {
        collect(child, &mut constant, &mut parity);
    }
    let mut survivors = parity
        .into_iter()
        .filter_map(|(expression, odd)| odd.then_some(expression))
        .collect::<Vec<_>>();
    if constant != 0 {
        survivors.push(Expr::Const(constant));
    }
    match survivors.len() {
        0 => Expr::zero(),
        1 => survivors.pop().unwrap_or_else(Expr::zero),
        _ => Expr::Xor(survivors),
    }
}

fn split_scale(expression: &Expr) -> (u64, Expr) {
    match expression {
        Expr::Scale(coefficient, child) => {
            let (nested, base) = split_scale(child);
            (coefficient.wrapping_mul(nested), base)
        }
        _ => (1, expression.clone()),
    }
}

fn canonical_add(children: &[Expr]) -> Expr {
    fn collect(expression: &Expr, constant: &mut u64, coefficients: &mut BTreeMap<Expr, u64>) {
        match expression {
            Expr::Add(children) => {
                for child in children {
                    collect(child, constant, coefficients);
                }
            }
            _ => {
                let (coefficient, base) = split_scale(expression);
                if let Expr::Const(value) = base {
                    *constant = constant.wrapping_add(coefficient.wrapping_mul(value));
                } else {
                    let entry = coefficients.entry(base).or_insert(0);
                    *entry = entry.wrapping_add(coefficient);
                }
            }
        }
    }

    let mut constant = 0u64;
    let mut coefficients = BTreeMap::new();
    for child in children {
        collect(child, &mut constant, &mut coefficients);
    }
    let mut terms = coefficients
        .into_iter()
        .filter_map(|(expression, coefficient)| {
            (coefficient != 0).then_some(if coefficient == 1 {
                expression
            } else {
                Expr::Scale(coefficient, Box::new(expression))
            })
        })
        .collect::<Vec<_>>();
    if constant != 0 {
        terms.push(Expr::Const(constant));
    }
    match terms.len() {
        0 => Expr::zero(),
        1 => terms.pop().unwrap_or_else(Expr::zero),
        _ => Expr::Add(terms),
    }
}

fn canonical_composition(expression: &Expr) -> Option<Expr> {
    let canonical = match expression {
        Expr::Add(children) => canonical_add(children),
        Expr::Xor(children) => canonical_xor(children),
        _ => return None,
    };
    (canonical != *expression).then_some(canonical)
}

fn stationary_graph(
    expression: &Expr,
    variables: [VarId; 2],
) -> Result<BaseGraph, FactorizedSectionRefusal> {
    let table = boolean_coordinates(expression, variables, 1)?.map(|value| value as u8);
    let epsilon_nonzero =
        std::array::from_fn(|assignment| i128::from(table[assignment + 1]) - i128::from(table[0]));
    Ok(BaseGraph {
        bases: vec![Base {
            epsilon_nonzero,
            tau: [0; 4],
        }],
        root: 0,
        r0: -i128::from(table[0]),
    })
}

fn compile_graph(
    expression: &Expr,
    variables: [VarId; 2],
) -> Result<BaseGraph, FactorizedSectionRefusal> {
    if let Some(canonical) = canonical_composition(expression) {
        return compile_graph(&canonical, variables);
    }
    if local_stationary(expression) {
        return stationary_graph(expression, variables);
    }
    match expression {
        Expr::Const(value) => Ok(BaseGraph {
            bases: vec![Base {
                epsilon_nonzero: [0; 3],
                tau: [0; 4],
            }],
            root: 0,
            r0: signed_word(*value),
        }),
        Expr::Add(children) => {
            let mut graphs = Vec::with_capacity(children.len());
            for child in children {
                graphs.push(compile_graph(child, variables)?);
            }
            product_graph(&graphs)
        }
        Expr::Scale(coefficient, child) => {
            let graph = compile_graph(child, variables)?;
            scale_graph(graph, signed_word(*coefficient))
        }
        Expr::Not(child) => negate_graph(compile_graph(child, variables)?),
        Expr::And(children) => compile_bitwise_children(BitwiseOperation::And, children, variables),
        Expr::Or(children) => compile_bitwise_children(BitwiseOperation::Or, children, variables),
        Expr::Xor(children) => compile_bitwise_children(BitwiseOperation::Xor, children, variables),
        Expr::Mul(_) => Err(FactorizedSectionRefusal::UnsupportedMultiplication),
        Expr::Var(_) => Err(FactorizedSectionRefusal::UnsupportedNode),
    }
}

fn one_base_from_table(table: [u8; 4]) -> (i128, [i128; 3]) {
    let table = table.map(i128::from);
    (
        -table[0],
        std::array::from_fn(|index| table[index + 1] - table[0]),
    )
}

fn one_base_add(
    left: (i128, [i128; 3]),
    right: (i128, [i128; 3]),
) -> Result<(i128, [i128; 3]), FactorizedSectionRefusal> {
    let mut epsilon = [0; 3];
    for (index, value) in epsilon.iter_mut().enumerate() {
        *value = left.1[index]
            .checked_add(right.1[index])
            .ok_or(FactorizedSectionRefusal::ArithmeticOverflow)?;
    }
    Ok((
        left.0
            .checked_add(right.0)
            .ok_or(FactorizedSectionRefusal::ArithmeticOverflow)?,
        epsilon,
    ))
}

fn one_base_scale(
    key: (i128, [i128; 3]),
    coefficient: i128,
) -> Result<(i128, [i128; 3]), FactorizedSectionRefusal> {
    Ok((
        key.0
            .checked_mul(coefficient)
            .ok_or(FactorizedSectionRefusal::ArithmeticOverflow)?,
        [
            key.1[0]
                .checked_mul(coefficient)
                .ok_or(FactorizedSectionRefusal::ArithmeticOverflow)?,
            key.1[1]
                .checked_mul(coefficient)
                .ok_or(FactorizedSectionRefusal::ArithmeticOverflow)?,
            key.1[2]
                .checked_mul(coefficient)
                .ok_or(FactorizedSectionRefusal::ArithmeticOverflow)?,
        ],
    ))
}

fn one_base_not(key: (i128, [i128; 3])) -> Result<(i128, [i128; 3]), FactorizedSectionRefusal> {
    Ok((
        (-1i128)
            .checked_sub(key.0)
            .ok_or(FactorizedSectionRefusal::ArithmeticOverflow)?,
        [
            key.1[0]
                .checked_neg()
                .ok_or(FactorizedSectionRefusal::ArithmeticOverflow)?,
            key.1[1]
                .checked_neg()
                .ok_or(FactorizedSectionRefusal::ArithmeticOverflow)?,
            key.1[2]
                .checked_neg()
                .ok_or(FactorizedSectionRefusal::ArithmeticOverflow)?,
        ],
    ))
}

fn one_base_key(value: OneBasePartial) -> Option<(i128, [i128; 3])> {
    match value {
        OneBasePartial::Stationary(table) => Some(one_base_from_table(table)),
        OneBasePartial::Exact { r0, epsilon } => Some((r0, epsilon)),
        OneBasePartial::Unknown => None,
    }
}

/// Deliberately incomplete bottom-up compiler: exact one-base coordinates or
/// `Unknown`. An unknown result never classifies the source as non-FSC.
fn compile_one_base_direct(
    expression: &Expr,
    variables: [VarId; 2],
) -> Result<OneBasePartial, FactorizedSectionRefusal> {
    match expression {
        Expr::Const(0) => Ok(OneBasePartial::Stationary([0; 4])),
        Expr::Const(value) if *value == u64::MAX => Ok(OneBasePartial::Stationary([1; 4])),
        Expr::Const(value) => Ok(OneBasePartial::Exact {
            r0: signed_word(*value),
            epsilon: [0; 3],
        }),
        Expr::Var(variable) => {
            let Some(index) = variables.iter().position(|candidate| candidate == variable) else {
                return Err(FactorizedSectionRefusal::InvalidVariables);
            };
            Ok(OneBasePartial::Stationary(std::array::from_fn(
                |assignment| ((assignment >> index) & 1) as u8,
            )))
        }
        Expr::Not(child) => match compile_one_base_direct(child, variables)? {
            OneBasePartial::Stationary(table) => {
                Ok(OneBasePartial::Stationary(table.map(|bit| bit ^ 1)))
            }
            OneBasePartial::Exact { r0, epsilon } => {
                let (r0, epsilon) = one_base_not((r0, epsilon))?;
                Ok(OneBasePartial::Exact { r0, epsilon })
            }
            OneBasePartial::Unknown => Ok(OneBasePartial::Unknown),
        },
        Expr::And(children) | Expr::Or(children) | Expr::Xor(children) => {
            let operation = match expression {
                Expr::And(_) => BitwiseOperation::And,
                Expr::Or(_) => BitwiseOperation::Or,
                Expr::Xor(_) => BitwiseOperation::Xor,
                _ => unreachable!(),
            };
            let mut table = match operation {
                BitwiseOperation::And => [1; 4],
                BitwiseOperation::Or | BitwiseOperation::Xor => [0; 4],
            };
            for child in children {
                let OneBasePartial::Stationary(child) = compile_one_base_direct(child, variables)?
                else {
                    return Ok(OneBasePartial::Unknown);
                };
                for assignment in 0..4 {
                    table[assignment] = operation.apply(&[table[assignment], child[assignment]])?;
                }
            }
            Ok(OneBasePartial::Stationary(table))
        }
        Expr::Add(children) => {
            let mut sum = (0, [0; 3]);
            for child in children {
                let Some(key) = one_base_key(compile_one_base_direct(child, variables)?) else {
                    return Ok(OneBasePartial::Unknown);
                };
                sum = one_base_add(sum, key)?;
            }
            Ok(OneBasePartial::Exact {
                r0: sum.0,
                epsilon: sum.1,
            })
        }
        Expr::Scale(coefficient, child) => {
            let Some(key) = one_base_key(compile_one_base_direct(child, variables)?) else {
                return Ok(OneBasePartial::Unknown);
            };
            let (r0, epsilon) = one_base_scale(key, signed_word(*coefficient))?;
            Ok(OneBasePartial::Exact { r0, epsilon })
        }
        Expr::Mul(_) => Err(FactorizedSectionRefusal::UnsupportedMultiplication),
    }
}

fn graph_one_base_key(graph: &BaseGraph) -> Option<(i128, [i128; 3])> {
    (graph.root == 0 && graph.bases.len() == 1 && graph.bases[0].tau == [0; 4])
        .then_some((graph.r0, graph.bases[0].epsilon_nonzero))
}

fn compile_one_base_analyzed(
    source: &Expr,
    variables: [VarId; 2],
) -> Result<Option<OneBaseDescriptor>, FactorizedSectionRefusal> {
    match compile_one_base_direct(source, variables)? {
        OneBasePartial::Stationary(table) => {
            let (r0, epsilon_nonzero) = one_base_from_table(table);
            Ok(Some(OneBaseDescriptor {
                variables,
                r0,
                epsilon_nonzero,
                direct: true,
            }))
        }
        OneBasePartial::Exact {
            r0,
            epsilon: epsilon_nonzero,
        } => Ok(Some(OneBaseDescriptor {
            variables,
            r0,
            epsilon_nonzero,
            direct: true,
        })),
        OneBasePartial::Unknown => Ok(graph_one_base_key(&compile_graph(source, variables)?).map(
            |(r0, epsilon_nonzero)| OneBaseDescriptor {
                variables,
                r0,
                epsilon_nonzero,
                direct: false,
            },
        )),
    }
}

/// Compiles an exact FSC-2 one-base descriptor. Every refusal or incomplete
/// fast-path result fails closed; unsupported sources continue through v1.
pub(crate) fn compile_one_base(source: &Expr, width: u8) -> Option<OneBaseDescriptor> {
    if width != WIDTH {
        return None;
    }
    let analysis = analyze_source(source).ok()?;
    if analysis.contains_multiplication {
        return None;
    }
    let variables = analyzed_variables(&analysis)?;
    compile_one_base_analyzed(source, variables).ok().flatten()
}

pub(crate) fn compile_declared_one_base_descriptor(
    source: &Expr,
    variables: [VarId; 2],
) -> Option<OneBaseDescriptor> {
    if !variables_valid(variables) {
        return None;
    }
    compile_one_base_analyzed(source, variables).ok().flatten()
}

#[cfg(test)]
pub(crate) struct OneBaseCompileProfile {
    pub(crate) descriptor: Option<OneBaseDescriptor>,
    pub(crate) source_analysis_ns: u128,
    pub(crate) direct_compile_ns: u128,
    pub(crate) general_fallback_ns: u128,
}

#[cfg(test)]
pub(crate) fn compile_one_base_profiled(source: &Expr, width: u8) -> OneBaseCompileProfile {
    use std::time::Instant;

    let analysis_started = Instant::now();
    let analysis = analyze_source(source);
    let source_analysis_ns = analysis_started.elapsed().as_nanos();
    let Ok(analysis) = analysis else {
        return OneBaseCompileProfile {
            descriptor: None,
            source_analysis_ns,
            direct_compile_ns: 0,
            general_fallback_ns: 0,
        };
    };
    let Some(variables) = (width == WIDTH && !analysis.contains_multiplication)
        .then(|| analyzed_variables(&analysis))
        .flatten()
    else {
        return OneBaseCompileProfile {
            descriptor: None,
            source_analysis_ns,
            direct_compile_ns: 0,
            general_fallback_ns: 0,
        };
    };
    let direct_started = Instant::now();
    let direct = compile_one_base_direct(source, variables);
    let direct_compile_ns = direct_started.elapsed().as_nanos();
    let descriptor = match direct {
        Ok(OneBasePartial::Stationary(table)) => {
            let (r0, epsilon_nonzero) = one_base_from_table(table);
            Some(OneBaseDescriptor {
                variables,
                r0,
                epsilon_nonzero,
                direct: true,
            })
        }
        Ok(OneBasePartial::Exact {
            r0,
            epsilon: epsilon_nonzero,
        }) => Some(OneBaseDescriptor {
            variables,
            r0,
            epsilon_nonzero,
            direct: true,
        }),
        Ok(OneBasePartial::Unknown) => None,
        Err(_) => {
            return OneBaseCompileProfile {
                descriptor: None,
                source_analysis_ns,
                direct_compile_ns,
                general_fallback_ns: 0,
            };
        }
    };
    if descriptor.is_some() {
        return OneBaseCompileProfile {
            descriptor,
            source_analysis_ns,
            direct_compile_ns,
            general_fallback_ns: 0,
        };
    }
    let fallback_started = Instant::now();
    let descriptor = compile_graph(source, variables)
        .ok()
        .as_ref()
        .and_then(graph_one_base_key)
        .map(|(r0, epsilon_nonzero)| OneBaseDescriptor {
            variables,
            r0,
            epsilon_nonzero,
            direct: false,
        });
    OneBaseCompileProfile {
        descriptor,
        source_analysis_ns,
        direct_compile_ns,
        general_fallback_ns: fallback_started.elapsed().as_nanos(),
    }
}

#[cfg(test)]
pub(crate) fn compile_one_base_authoritative(
    source: &Expr,
    width: u8,
) -> Option<OneBaseDescriptor> {
    let analysis = (width == WIDTH)
        .then(|| analyze_source(source).ok())
        .flatten()?;
    let variables = analyzed_variables(&analysis)?;
    let (r0, epsilon_nonzero) = graph_one_base_key(&compile_graph(source, variables).ok()?)?;
    Some(OneBaseDescriptor {
        variables,
        r0,
        epsilon_nonzero,
        direct: false,
    })
}

#[cfg(test)]
pub(crate) fn compile_declared_one_base(
    source: &Expr,
    variables: [VarId; 2],
) -> Option<(i128, [i128; 3])> {
    variables_valid(variables)
        .then(|| compile_graph(source, variables).ok())
        .flatten()
        .as_ref()
        .and_then(graph_one_base_key)
}

fn compile_bitwise_children(
    operation: BitwiseOperation,
    children: &[Expr],
    variables: [VarId; 2],
) -> Result<BaseGraph, FactorizedSectionRefusal> {
    let mut graphs = Vec::with_capacity(children.len());
    for child in children {
        graphs.push(compile_graph(child, variables)?);
    }
    bitwise_graph(operation, &graphs)
}

fn negate_graph(mut graph: BaseGraph) -> Result<BaseGraph, FactorizedSectionRefusal> {
    graph.r0 = (-1i128)
        .checked_sub(graph.r0)
        .ok_or(FactorizedSectionRefusal::ArithmeticOverflow)?;
    for base in &mut graph.bases {
        for epsilon in &mut base.epsilon_nonzero {
            *epsilon = epsilon
                .checked_neg()
                .ok_or(FactorizedSectionRefusal::ArithmeticOverflow)?;
        }
    }
    Ok(graph)
}

#[derive(Clone, Copy)]
enum BitwiseOperation {
    And,
    Or,
    Xor,
}

impl BitwiseOperation {
    fn apply(self, bits: &[u8]) -> Result<u8, FactorizedSectionRefusal> {
        match self {
            Self::And => Ok(bits.iter().copied().fold(1, |left, right| left & right)),
            Self::Or => Ok(bits.iter().copied().fold(0, |left, right| left | right)),
            Self::Xor => Ok(bits.iter().copied().fold(0, |left, right| left ^ right)),
        }
    }
}

fn state_response(operation: BitwiseOperation, state: &[DigitCursor]) -> i128 {
    match operation {
        BitwiseOperation::And => state.iter().fold(-1i128, |acc, cursor| acc & cursor.carry),
        BitwiseOperation::Or => state.iter().fold(0i128, |acc, cursor| acc | cursor.carry),
        BitwiseOperation::Xor => state.iter().fold(0i128, |acc, cursor| acc ^ cursor.carry),
    }
}

fn advance_digit(
    graph: &BaseGraph,
    cursor: DigitCursor,
    symbol: usize,
) -> Result<(u8, DigitCursor), FactorizedSectionRefusal> {
    let base = graph
        .bases
        .get(cursor.base)
        .ok_or(FactorizedSectionRefusal::UnsupportedNode)?;
    let total = cursor
        .carry
        .checked_add(base.epsilon(symbol))
        .ok_or(FactorizedSectionRefusal::ArithmeticOverflow)?;
    let output = total.rem_euclid(2) as u8;
    let numerator = total
        .checked_sub(i128::from(output))
        .ok_or(FactorizedSectionRefusal::ArithmeticOverflow)?;
    debug_assert_eq!(numerator % 2, 0);
    let carry = numerator / 2;
    Ok((
        output,
        DigitCursor {
            base: base.tau[symbol],
            carry,
        },
    ))
}

fn bitwise_graph(
    operation: BitwiseOperation,
    children: &[BaseGraph],
) -> Result<BaseGraph, FactorizedSectionRefusal> {
    let initial = children
        .iter()
        .map(|child| DigitCursor {
            base: child.root,
            carry: child.r0,
        })
        .collect::<Vec<_>>();
    let mut ids = BTreeMap::from([(initial.clone(), 0usize)]);
    let mut states = vec![initial];
    let mut tau = Vec::new();
    let mut responses = vec![state_response(operation, &states[0])];
    let mut epsilon = Vec::new();
    let mut head = 0usize;
    while head < states.len() {
        let state = states[head].clone();
        let mut state_epsilon = [0i128; 3];
        let mut state_tau = [0usize; 4];
        for symbol in 0..4 {
            let mut bits = Vec::with_capacity(children.len());
            let mut destination = Vec::with_capacity(children.len());
            for (child, cursor) in children.iter().zip(&state) {
                let (bit, next) = advance_digit(child, *cursor, symbol)?;
                bits.push(bit);
                destination.push(next);
            }
            let output = operation.apply(&bits)?;
            let id = if let Some(id) = ids.get(&destination) {
                *id
            } else {
                if states.len() >= 256 {
                    return Err(FactorizedSectionRefusal::SourceBudgetExceeded);
                }
                let id = states.len();
                ids.insert(destination.clone(), id);
                states.push(destination);
                id
            };
            state_tau[symbol] = id;
            if id == responses.len() {
                responses.push(state_response(operation, &states[id]));
            }
            let doubled_destination = responses[id]
                .checked_mul(2)
                .ok_or(FactorizedSectionRefusal::ArithmeticOverflow)?;
            let epsilon_value = i128::from(output)
                .checked_add(doubled_destination)
                .and_then(|value| value.checked_sub(responses[head]))
                .ok_or(FactorizedSectionRefusal::ArithmeticOverflow)?;
            if symbol == 0 {
                if epsilon_value != 0 {
                    return Err(FactorizedSectionRefusal::InternalGaugeViolation);
                }
            } else {
                state_epsilon[symbol - 1] = epsilon_value;
            }
        }
        epsilon.push(state_epsilon);
        tau.push(state_tau);
        head += 1;
    }
    let (bases, root) = minimize_bases(&epsilon, &tau, 0)?;
    Ok(BaseGraph {
        bases,
        root,
        r0: responses[0],
    })
}

fn product_graph(children: &[BaseGraph]) -> Result<BaseGraph, FactorizedSectionRefusal> {
    if children.is_empty() {
        return Err(FactorizedSectionRefusal::UnsupportedNode);
    }
    let initial = children.iter().map(|child| child.root).collect::<Vec<_>>();
    let mut ids = BTreeMap::from([(initial.clone(), 0usize)]);
    let mut tuples = vec![initial];
    let mut epsilon = Vec::new();
    let mut tau = Vec::new();
    let mut head = 0usize;
    while head < tuples.len() {
        let tuple = tuples[head].clone();
        let mut state_epsilon = [0i128; 3];
        let mut state_tau = [0usize; 4];
        for symbol in 0..4 {
            let mut destination = Vec::with_capacity(children.len());
            for (child, base) in children.iter().zip(&tuple) {
                if symbol != 0 {
                    state_epsilon[symbol - 1] = state_epsilon[symbol - 1]
                        .checked_add(child.bases[*base].epsilon(symbol))
                        .ok_or(FactorizedSectionRefusal::ArithmeticOverflow)?;
                }
                destination.push(child.bases[*base].tau[symbol]);
            }
            let id = if let Some(id) = ids.get(&destination) {
                *id
            } else {
                if tuples.len() >= 256 {
                    return Err(FactorizedSectionRefusal::SourceBudgetExceeded);
                }
                let id = tuples.len();
                ids.insert(destination.clone(), id);
                tuples.push(destination);
                id
            };
            state_tau[symbol] = id;
        }
        epsilon.push(state_epsilon);
        tau.push(state_tau);
        head += 1;
    }
    let r0 = children.iter().try_fold(0i128, |sum, child| {
        sum.checked_add(child.r0)
            .ok_or(FactorizedSectionRefusal::ArithmeticOverflow)
    })?;
    let (bases, root) = minimize_bases(&epsilon, &tau, 0)?;
    Ok(BaseGraph { bases, root, r0 })
}

#[derive(Clone, Copy)]
struct LinearAddBudget {
    max_dimension: usize,
    max_basis_vectors: usize,
    max_exact_operations: usize,
    exact_operations: usize,
}

impl LinearAddBudget {
    fn new() -> Self {
        Self {
            max_dimension: MAX_LINEAR_ADD_DIMENSION,
            max_basis_vectors: MAX_LINEAR_ADD_BASIS_VECTORS,
            max_exact_operations: MAX_LINEAR_ADD_OPERATIONS,
            exact_operations: 0,
        }
    }

    fn charge(&mut self, operations: usize) -> Result<(), FactorizedSectionRefusal> {
        self.exact_operations = self
            .exact_operations
            .checked_add(operations)
            .ok_or(FactorizedSectionRefusal::SourceBudgetExceeded)?;
        if self.exact_operations > self.max_exact_operations {
            return Err(FactorizedSectionRefusal::SourceBudgetExceeded);
        }
        Ok(())
    }
}

fn linear_add_match(
    children: &[BaseGraph],
    target: &SectionScalars,
) -> Result<(), FactorizedSectionRefusal> {
    if children.is_empty() {
        return Err(FactorizedSectionRefusal::UnsupportedNode);
    }
    let mut budget = LinearAddBudget::new();
    let mut dimension = 1usize;
    for child in children {
        dimension = dimension
            .checked_add(child.bases.len())
            .ok_or(FactorizedSectionRefusal::SourceBudgetExceeded)?;
    }
    if dimension > budget.max_dimension {
        return Err(FactorizedSectionRefusal::SourceBudgetExceeded);
    }

    let source_r0 = children.iter().try_fold(0i128, |sum, child| {
        sum.checked_add(child.r0)
            .ok_or(FactorizedSectionRefusal::ArithmeticOverflow)
    })?;
    if source_r0 != target.r0 {
        return Err(FactorizedSectionRefusal::CandidateMismatch);
    }

    let mut transitions = Vec::with_capacity(dimension);
    let mut profiles = Vec::with_capacity(dimension);
    let mut offset = 0usize;
    for child in children {
        for base in &child.bases {
            transitions.push(base.tau.map(|destination| destination + offset));
            profiles.push([
                base.epsilon(0),
                base.epsilon_nonzero[0],
                base.epsilon_nonzero[1],
                base.epsilon_nonzero[2],
            ]);
        }
        offset += child.bases.len();
    }
    let target_index = dimension - 1;
    transitions.push([target_index; 4]);
    profiles.push(target.epsilon);

    let mut initial = vec![Rational::integer(0); dimension];
    let mut offset = 0usize;
    for child in children {
        initial[offset + child.root].numerator = initial[offset + child.root]
            .numerator
            .checked_add(1)
            .ok_or(FactorizedSectionRefusal::ArithmeticOverflow)?;
        offset += child.bases.len();
    }
    initial[target_index].numerator = -1;

    let mut basis = Vec::<(usize, Vec<Rational>)>::new();
    let mut pending = vec![initial];
    let mut head = 0usize;
    while head < pending.len() {
        let mut vector = pending[head].clone();
        head += 1;
        for (pivot, row) in &basis {
            let coefficient = vector[*pivot].clone();
            if coefficient.is_zero() {
                continue;
            }
            budget.charge(dimension.saturating_mul(2))?;
            for (value, basis_value) in vector.iter_mut().zip(row) {
                *value = value.subtract(&coefficient.multiply(basis_value)?)?;
            }
        }
        let Some(pivot) = vector.iter().position(|value| !value.is_zero()) else {
            continue;
        };
        budget.charge(dimension)?;
        let pivot_value = vector[pivot].clone();
        for value in &mut vector {
            *value = value.divide(&pivot_value)?;
        }
        let insertion = basis.partition_point(|(existing, _)| *existing < pivot);
        basis.insert(insertion, (pivot, vector.clone()));
        if basis.len() > budget.max_basis_vectors {
            return Err(FactorizedSectionRefusal::SourceBudgetExceeded);
        }

        for symbol in 0..4 {
            budget.charge(dimension.saturating_mul(3))?;
            let mut observation = Rational::integer(0);
            for (coefficient, profile) in vector.iter().zip(&profiles) {
                observation =
                    observation.add(&coefficient.multiply(&Rational::integer(profile[symbol]))?)?;
            }
            if !observation.is_zero() {
                return Err(FactorizedSectionRefusal::CandidateMismatch);
            }

            let mut destination = vec![Rational::integer(0); dimension];
            for (coefficient, transition) in vector.iter().zip(&transitions) {
                let destination_index = transition[symbol];
                destination[destination_index] = destination[destination_index].add(coefficient)?;
            }
            pending.push(destination);
        }
    }
    Ok(())
}

fn scale_graph(graph: BaseGraph, coefficient: i128) -> Result<BaseGraph, FactorizedSectionRefusal> {
    if coefficient == 0 {
        return Ok(BaseGraph {
            bases: vec![Base {
                epsilon_nonzero: [0; 3],
                tau: [0; 4],
            }],
            root: 0,
            r0: 0,
        });
    }
    let mut graph = graph;
    graph.r0 = graph
        .r0
        .checked_mul(coefficient)
        .ok_or(FactorizedSectionRefusal::ArithmeticOverflow)?;
    for base in &mut graph.bases {
        for epsilon in &mut base.epsilon_nonzero {
            *epsilon = epsilon
                .checked_mul(coefficient)
                .ok_or(FactorizedSectionRefusal::ArithmeticOverflow)?;
        }
    }
    Ok(graph)
}

fn minimize_bases(
    epsilon: &[[i128; 3]],
    tau: &[[usize; 4]],
    initial: usize,
) -> Result<(Vec<Base>, usize), FactorizedSectionRefusal> {
    if epsilon.is_empty() || epsilon.len() != tau.len() || initial >= epsilon.len() {
        return Err(FactorizedSectionRefusal::UnsupportedNode);
    }
    let mut ids = BTreeMap::<[i128; 3], usize>::new();
    let mut classes = epsilon
        .iter()
        .map(|profile| {
            let next = ids.len();
            *ids.entry(*profile).or_insert(next)
        })
        .collect::<Vec<_>>();
    let mut refined = Vec::with_capacity(epsilon.len());
    loop {
        let mut signatures = BTreeMap::<([i128; 3], [usize; 4]), usize>::new();
        refined.clear();
        refined.extend((0..epsilon.len()).map(|state| {
            let signature = (
                epsilon[state],
                std::array::from_fn(|symbol| classes[tau[state][symbol]]),
            );
            let next = signatures.len();
            *signatures.entry(signature).or_insert(next)
        }));
        if refined == classes {
            break;
        }
        std::mem::swap(&mut classes, &mut refined);
    }
    let class_count = classes.iter().copied().max().map_or(0, |value| value + 1);
    if class_count > 64 {
        return Err(FactorizedSectionRefusal::SourceBudgetExceeded);
    }
    let initial_class = classes[initial];
    if class_count == 1 {
        return Ok((
            vec![Base {
                epsilon_nonzero: epsilon[initial],
                tau: [0; 4],
            }],
            0,
        ));
    }
    let mut representatives = vec![usize::MAX; class_count];
    for (state, class) in classes.iter().copied().enumerate() {
        representatives[class] = representatives[class].min(state);
    }
    let mut canonical = vec![usize::MAX; class_count];
    canonical[initial_class] = 0;
    let mut queue = VecDeque::from([initial_class]);
    let mut next_canonical = 1;
    while let Some(class) = queue.pop_front() {
        let state = representatives[class];
        for symbol in 0..4 {
            let destination = classes[tau[state][symbol]];
            if canonical[destination] == usize::MAX {
                canonical[destination] = next_canonical;
                next_canonical += 1;
                queue.push_back(destination);
            }
        }
    }
    if canonical.contains(&usize::MAX) {
        return Err(FactorizedSectionRefusal::UnsupportedNode);
    }
    let mut old_by_canonical = vec![usize::MAX; class_count];
    for (old, id) in canonical.iter().copied().enumerate() {
        old_by_canonical[id] = old;
    }
    let bases = old_by_canonical
        .iter()
        .map(|old| {
            let state = representatives[*old];
            Base {
                epsilon_nonzero: epsilon[state],
                tau: std::array::from_fn(|symbol| canonical[classes[tau[state][symbol]]]),
            }
        })
        .collect::<Vec<_>>();
    Ok((bases, canonical[initial_class]))
}

struct NominatedCandidate {
    expression: Expr,
    scalars: SectionScalars,
}

fn nominate_candidate(
    source: &Expr,
    variables: [VarId; 2],
    width: u8,
    analysis: SourceAnalysis,
) -> Option<NominatedCandidate> {
    if width == 0
        || width > WIDTH
        || analysis.node_count > MAX_SOURCE_NODES
        || analysis.contains_multiplication
    {
        return None;
    }
    let mask = make_mask(width);
    let mut coefficients = boolean_coordinates(source, variables, width).ok()?;
    for bit in 0..2 {
        let flag = 1usize << bit;
        for index in 0..4 {
            if index & flag != 0 {
                coefficients[index] =
                    coefficients[index].wrapping_sub(coefficients[index ^ flag]) & mask;
            }
        }
    }
    let exact = coefficients.map(signed_word);
    let epsilon3 = exact[1].checked_add(exact[2])?.checked_add(exact[3])?;
    let scalars = SectionScalars {
        r0: exact[0],
        epsilon: [0, exact[1], exact[2], epsilon3],
    };
    let mut terms = Vec::with_capacity(4);
    if coefficients[0] != 0 {
        terms.push(Expr::Const(coefficients[0]));
    }
    for (subset, coefficient) in coefficients.into_iter().enumerate().skip(1) {
        if coefficient == 0 {
            continue;
        }
        let conjunction = match subset {
            1 => Expr::Var(variables[0]),
            2 => Expr::Var(variables[1]),
            3 => Expr::And(vec![Expr::Var(variables[0]), Expr::Var(variables[1])]),
            _ => unreachable!(),
        };
        terms.push(if coefficient == 1 {
            conjunction
        } else {
            coefficient * conjunction
        });
    }
    let expression = match terms.len() {
        0 => Expr::zero(),
        1 => terms.pop().unwrap_or_else(Expr::zero),
        _ => Expr::Add(terms),
    }
    .reduce(width);
    (expression.size() < analysis.node_count).then_some(NominatedCandidate {
        expression,
        scalars,
    })
}

fn direct_match(
    source: &Expr,
    candidate_scalars: &SectionScalars,
    variables: [VarId; 2],
    width: u8,
) -> Result<(), FactorizedSectionRefusal> {
    if width != WIDTH {
        return Err(FactorizedSectionRefusal::InvalidWidth);
    }
    if !variables_valid(variables) {
        return Err(FactorizedSectionRefusal::InvalidVariables);
    }
    if let Some(canonical) = canonical_composition(source) {
        return direct_match(&canonical, candidate_scalars, variables, width);
    }
    if let Expr::Add(children) = source {
        let graphs = children
            .iter()
            .map(|child| compile_graph(child, variables))
            .collect::<Result<Vec<_>, _>>()?;
        let source_scalars = match product_graph(&graphs) {
            Ok(graph) => normalize_scalars(graph),
            Err(FactorizedSectionRefusal::SourceBudgetExceeded) => {
                return linear_add_match(&graphs, candidate_scalars);
            }
            Err(error) => Err(error),
        }?;
        return (source_scalars == *candidate_scalars)
            .then_some(())
            .ok_or(FactorizedSectionRefusal::CandidateMismatch);
    }
    let source_scalars = compile_single_base_scalars(source, variables)?;
    if source_scalars != *candidate_scalars {
        return Err(FactorizedSectionRefusal::CandidateMismatch);
    }
    Ok(())
}

pub(crate) fn install(source: Expr, width: u8) -> Expr {
    if width != WIDTH {
        return source;
    }
    let Ok(analysis) = analyze_source(&source) else {
        return source;
    };
    debug_assert_eq!(analysis.node_count, source.size());
    let Some(variables) = analyzed_variables(&analysis) else {
        return source;
    };
    let Some(nominated) = nominate_candidate(&source, variables, width, analysis) else {
        return source;
    };
    let direct_result = direct_match(&source, &nominated.scalars, variables, width);
    if direct_result.is_ok() {
        crate::prettify::prettify(nominated.expression, width)
    } else {
        source
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const VARIABLES: [VarId; 2] = [VarId(0), VarId(1)];

    fn x() -> Expr {
        Expr::Var(VarId(0))
    }

    fn y() -> Expr {
        Expr::Var(VarId(1))
    }

    fn scalar(value: i128) -> i128 {
        value
    }

    fn base(epsilon: [i128; 4], tau: [usize; 4]) -> Base {
        Base {
            epsilon_nonzero: [epsilon[1], epsilon[2], epsilon[3]],
            tau,
        }
    }

    fn scalars(r0: i128, epsilon: [i128; 4]) -> SectionScalars {
        SectionScalars {
            r0: scalar(r0),
            epsilon: epsilon.map(scalar),
        }
    }

    fn xor_delay(variable: Expr, memory: u32) -> Expr {
        Expr::Xor(
            (0..=memory)
                .map(|shift| Expr::Scale(1u64 << shift, Box::new(variable.clone())))
                .collect(),
        )
    }

    fn cancelling_add(x_memory: u32, y_memory: u32) -> Expr {
        let left = xor_delay(x(), x_memory);
        let right = xor_delay(y(), y_memory);
        Expr::Add(vec![
            left.clone(),
            Expr::Scale(u64::MAX, Box::new(left)),
            right.clone(),
            Expr::Scale(u64::MAX, Box::new(right)),
        ])
    }

    fn nonstructural_cancelling_add(x_memory: u32, y_memory: u32) -> Expr {
        let left = xor_delay(x(), x_memory);
        let right = xor_delay(y(), y_memory);
        Expr::Add(vec![
            left.clone(),
            Expr::Scale(u64::MAX, Box::new(Expr::Add(vec![left, Expr::zero()]))),
            right.clone(),
            Expr::Scale(u64::MAX, Box::new(Expr::Add(vec![right, Expr::zero()]))),
        ])
    }

    fn cancelling_xor(x_memory: u32, y_memory: u32) -> Expr {
        let left = xor_delay(x(), x_memory);
        let right = xor_delay(y(), y_memory);
        Expr::Xor(vec![left.clone(), left, right.clone(), right])
    }

    #[test]
    fn adversarial_integer_limits_are_checked_and_two_adic() {
        let graph = BaseGraph {
            bases: vec![Base {
                epsilon_nonzero: [i128::MAX, i128::MIN, -3],
                tau: [0; 4],
            }],
            root: 0,
            r0: 0,
        };
        let (_, positive) = advance_digit(&graph, DigitCursor { base: 0, carry: 0 }, 1)
            .expect("positive max transition");
        assert_eq!(positive.carry, i128::MAX / 2);
        let (_, negative) = advance_digit(&graph, DigitCursor { base: 0, carry: 0 }, 3)
            .expect("negative odd transition");
        assert_eq!(negative.carry, -2);
        assert_eq!(
            advance_digit(
                &graph,
                DigitCursor {
                    base: 0,
                    carry: i128::MAX,
                },
                1,
            ),
            Err(FactorizedSectionRefusal::ArithmeticOverflow)
        );
        assert_eq!(
            state_response(BitwiseOperation::And, &[DigitCursor { base: 0, carry: -1 }],),
            -1
        );
        assert_eq!(
            scale_graph(
                BaseGraph {
                    bases: vec![Base {
                        epsilon_nonzero: [0; 3],
                        tau: [0; 4],
                    }],
                    root: 0,
                    r0: i128::MAX,
                },
                2,
            ),
            Err(FactorizedSectionRefusal::ArithmeticOverflow)
        );
        assert_eq!(
            product_graph(&[
                BaseGraph {
                    bases: vec![Base {
                        epsilon_nonzero: [0; 3],
                        tau: [0; 4],
                    }],
                    root: 0,
                    r0: i128::MAX,
                },
                BaseGraph {
                    bases: vec![Base {
                        epsilon_nonzero: [0; 3],
                        tau: [0; 4],
                    }],
                    root: 0,
                    r0: 1,
                },
            ]),
            Err(FactorizedSectionRefusal::ArithmeticOverflow)
        );
        println!(
            "ADVERSARIAL_INTEGER_TESTS checked_extremes=PASS negative_odd=PASS signed_bitwise=PASS"
        );
    }

    #[test]
    fn invalid_inputs_and_refusals_are_rejected() {
        let source = x() + y();
        assert_eq!(
            compile_single_base_scalars(&source, [VarId(1), VarId(0)]),
            Err(FactorizedSectionRefusal::InvalidVariables)
        );
        assert_eq!(install(source.clone(), WIDTH - 1), source);
        assert_eq!(
            compile_single_base_scalars(&Expr::Mul(vec![x(), y()]), VARIABLES),
            Err(FactorizedSectionRefusal::UnsupportedMultiplication)
        );
        let huge_variable = Expr::Add(vec![Expr::Var(VarId(usize::MAX)), x()]);
        assert_eq!(install(huge_variable.clone(), WIDTH), huge_variable);
    }

    #[test]
    fn constants_and_single_variables_have_exact_sections() {
        assert_eq!(
            compile_single_base_scalars(&Expr::Const(7), VARIABLES),
            Ok(scalars(7, [0, 0, 0, 0]))
        );
        assert_eq!(
            compile_single_base_scalars(&x(), VARIABLES),
            Ok(scalars(0, [0, 1, 0, 1]))
        );
        assert_eq!(
            compile_single_base_scalars(&y(), VARIABLES),
            Ok(scalars(0, [0, 0, 1, 1]))
        );
    }

    #[test]
    fn known_mobius_identity_matches_nominated_scalars() {
        let source = (x() ^ y()) + 2u64 * (x() & y());
        let nominated = nominate_candidate(
            &source,
            VARIABLES,
            WIDTH,
            analyze_source(&source).expect("analysis"),
        )
        .expect("nomination");
        assert_eq!(nominated.expression, x() + y());
        assert_eq!(nominated.scalars, scalars(0, [0, 1, 1, 2]));
        assert_eq!(
            direct_match(&source, &nominated.scalars, VARIABLES, WIDTH),
            Ok(())
        );
    }

    #[test]
    fn installed_candidate_is_prettified() {
        let source = Expr::Add(vec![
            x() ^ y(),
            Expr::zero(),
            Expr::zero(),
            Expr::zero(),
            Expr::zero(),
            Expr::zero(),
        ]);
        assert_eq!(install(source, WIDTH), x() ^ y());
    }

    #[test]
    fn each_scalar_mismatch_is_rejected() {
        let source = (x() ^ y()) + 2u64 * (x() & y());
        let expected = scalars(0, [0, 1, 1, 2]);

        let mut changed = expected.clone();
        changed.r0 = scalar(1);
        assert_eq!(
            direct_match(&source, &changed, VARIABLES, WIDTH),
            Err(FactorizedSectionRefusal::CandidateMismatch)
        );
        for index in 0..4 {
            let mut changed = expected.clone();
            changed.epsilon[index] = changed.epsilon[index]
                .checked_add(scalar(1))
                .expect("mutation");
            assert_eq!(
                direct_match(&source, &changed, VARIABLES, WIDTH),
                Err(FactorizedSectionRefusal::CandidateMismatch)
            );
        }
    }

    #[test]
    fn all_supported_compositions_match_exact_scalars() {
        let cases = [
            (x() + y(), scalars(0, [0, 1, 1, 2])),
            (3u64 * (x() + y()), scalars(0, [0, 3, 3, 6])),
            (!x(), scalars(-1, [0, -1, 0, -1])),
            (x() & y(), scalars(0, [0, 0, 0, 1])),
            (x() | y(), scalars(0, [0, 1, 1, 1])),
            (x() ^ y(), scalars(0, [0, 1, 1, 0])),
        ];
        for (source, expected) in cases {
            let compiled = compile_single_base_scalars(&source, VARIABLES).expect("single base");
            assert_eq!(compiled, expected);
            let coordinates = boolean_coordinates(&source, VARIABLES, WIDTH).expect("coordinates");
            for (symbol, coordinate) in coordinates.into_iter().enumerate() {
                assert_eq!(
                    compiled.r0.checked_add(compiled.epsilon[symbol]),
                    Some(scalar(signed_word(coordinate)))
                );
            }
        }
    }

    #[test]
    fn analysis_cost_matches_expression_size() {
        let expressions = [
            Expr::Const(0),
            x(),
            x() + y(),
            Expr::And(Vec::new()),
            Expr::Add(vec![x(), Expr::Not(Box::new(y()))]),
            Expr::Scale(3, Box::new(x() ^ y())),
            Expr::Mul(vec![x(), y()]),
        ];
        for expression in expressions {
            let analysis = analyze_source(&expression).expect("analysis");
            assert_eq!(analysis.node_count, expression.size());
        }
    }

    #[test]
    fn rejects_unbounded_variable_ids_and_deep_sources() {
        let huge_variable = Expr::Add(vec![Expr::Var(VarId(usize::MAX)), x()]);
        assert_eq!(install(huge_variable.clone(), WIDTH), huge_variable);

        let mut deep = x() + y();
        for _ in 0..MAX_SOURCE_DEPTH {
            deep = Expr::Not(Box::new(deep));
        }
        assert_eq!(install(deep.clone(), WIDTH), deep);
        assert_eq!(install(x() + y(), WIDTH - 1), x() + y());
    }

    #[test]
    fn affine_transforms_preserve_a_multi_base_graph() {
        let source = x() & (x() + y());
        let graph = compile_graph(&source, VARIABLES).expect("graph");
        assert!(graph.bases.len() > 1);

        let negated = compile_graph(&(!source.clone()), VARIABLES).expect("negated graph");
        let expected_negated = negate_graph(graph.clone()).expect("negation");
        assert_eq!(negated, expected_negated);

        let scaled = scale_graph(graph.clone(), 3).expect("scaled graph");
        assert_eq!(scaled.r0, graph.r0.checked_mul(3).expect("scale"));
        for (before, after) in graph.bases.iter().zip(&scaled.bases) {
            assert_eq!(before.tau, after.tau);
            for (old, new) in before.epsilon_nonzero.iter().zip(after.epsilon_nonzero) {
                assert_eq!(new, old.checked_mul(3).expect("scale"));
            }
        }
    }

    #[test]
    fn euclidean_remainder_produces_a_bit() {
        assert_eq!(5i128.rem_euclid(2), 1);
    }

    #[test]
    fn minimize_bases_collapses_equivalent_states() {
        let epsilon = [[scalar(1), scalar(1), scalar(2)]; 2];
        let tau = [[0; 4], [1; 4]];
        assert_eq!(
            minimize_bases(&epsilon, &tau, 0),
            Ok((vec![base([0, 1, 1, 2], [0; 4])], 0,))
        );
    }

    #[test]
    fn minimize_bases_is_independent_of_state_permutation() {
        let first_epsilon = vec![
            [scalar(0), scalar(0), scalar(0)],
            [scalar(1), scalar(0), scalar(1)],
        ];
        let first_tau = vec![[1; 4], [1; 4]];
        let permuted_epsilon = vec![
            [scalar(1), scalar(0), scalar(1)],
            [scalar(0), scalar(0), scalar(0)],
        ];
        let permuted_tau = vec![[0; 4], [0; 4]];
        assert_eq!(
            minimize_bases(&first_epsilon, &first_tau, 0),
            minimize_bases(&permuted_epsilon, &permuted_tau, 1)
        );
    }

    #[test]
    fn multi_base_and_unsupported_sources_are_refused() {
        let multi_base = x() + (y() ^ Expr::Const(1));
        assert!(matches!(
            compile_single_base_scalars(&multi_base, VARIABLES),
            Err(FactorizedSectionRefusal::MultipleSemanticBases)
        ));
        assert_eq!(
            compile_single_base_scalars(&(x() * y()), VARIABLES),
            Err(FactorizedSectionRefusal::UnsupportedMultiplication)
        );
    }

    #[test]
    fn canonical_compositions_remove_structural_cancellations_before_recursion() {
        let source = cancelling_add(5, 4);
        assert_eq!(
            compile_single_base_scalars(&source, VARIABLES),
            Ok(scalars(0, [0, 0, 0, 0]))
        );
        assert_eq!(install(source, WIDTH), Expr::zero());

        let source = cancelling_xor(5, 4);
        assert_eq!(
            compile_graph(&source, VARIABLES),
            Ok(BaseGraph {
                bases: vec![Base {
                    epsilon_nonzero: [scalar(0); 3],
                    tau: [0; 4],
                }],
                root: 0,
                r0: scalar(0),
            })
        );

        let d7 = Expr::Xor(vec![xor_delay(x(), 7), xor_delay(x(), 7), Expr::zero()]);
        assert_eq!(
            compile_graph(&d7, VARIABLES),
            Ok(BaseGraph {
                bases: vec![Base {
                    epsilon_nonzero: [scalar(0); 3],
                    tau: [0; 4],
                }],
                root: 0,
                r0: scalar(0),
            })
        );
    }

    #[test]
    fn linear_add_fallback_certifies_nonstructural_cancellations() {
        let source = nonstructural_cancelling_add(5, 4);
        assert_eq!(
            compile_single_base_scalars(&source, VARIABLES),
            Err(FactorizedSectionRefusal::SourceBudgetExceeded)
        );
        assert_eq!(install(source, WIDTH), Expr::zero());

        let source = nonstructural_cancelling_add(6, 6);
        assert_eq!(
            compile_single_base_scalars(&source, VARIABLES),
            Err(FactorizedSectionRefusal::SourceBudgetExceeded)
        );
        assert_eq!(install(source, WIDTH), Expr::zero());
    }

    #[test]
    fn delayed_xor_keeps_the_64_base_boundary() {
        assert_eq!(
            compile_graph(&xor_delay(x(), 6), VARIABLES)
                .expect("64-state graph")
                .bases
                .len(),
            64
        );
        assert_eq!(
            compile_graph(&xor_delay(x(), 7), VARIABLES),
            Err(FactorizedSectionRefusal::SourceBudgetExceeded)
        );
    }
}
