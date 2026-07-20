use std::collections::BTreeSet;

use rumba_core::{
    expr::Expr,
    parser::parse_expr,
    simplify::{
        ComplementRelationProof, DirectDependencyRejectReason, ExpectedBitwiseDependency,
        HiddenAtomDependencyKind, HiddenScopeTrace, SemanticAliasProof, diagnose_hidden_atoms,
        experiment_direct_bitwise_dependencies, experiment_direct_complement_relation,
        experiment_expected_bitwise_dependency, experiment_guided_semantic_aliases,
        experiment_guided_ternary_relations,
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
            "  scope={} width={} input_nodes={} atoms={} input_is_result={}",
            scope.scope,
            scope.bit_width,
            scope.input.size(),
            scope.atoms.len(),
            scope.input == *result,
        );
        if let Some(pre_restore_result) = &scope.pre_restore_result {
            println!("    pre_restore_result={pre_restore_result}");
        }
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

        if line == 369 {
            for scope in residual_trace.iter().filter(|scope| scope.input == residual) {
                let Some(experiment) = experiment_direct_bitwise_dependencies(scope)
                    .unwrap_or_else(|err| panic!("P7b-lite failed in scope {}: {err}", scope.scope))
                else {
                    continue;
                };
                println!(
                    "p7b_lite line=369 scope={} direct_atoms={:?} attempted_atoms={:?} proofs={} result_nodes={} residual_zero={}",
                    experiment.scope,
                    experiment.direct_atoms,
                    experiment.attempted_atoms,
                    experiment.proofs.len(),
                    experiment.simplified_after_substitution.size(),
                    experiment.simplified_after_substitution == Expr::zero(),
                );
                for proof in experiment.proofs {
                    println!(
                        "  proof atom=v{} definition={} candidate={}",
                        proof.atom, proof.definition, proof.candidate
                    );
                }
                for rejection in experiment.rejections {
                    match rejection.reason {
                        DirectDependencyRejectReason::NonBooleanCube => println!(
                            "  rejection atom=v{} reason=non_boolean_cube",
                            rejection.atom
                        ),
                        DirectDependencyRejectReason::ExactProofNotFound {
                            candidate,
                            residual,
                        } => println!(
                            "  rejection atom=v{} reason=exact_proof_not_found candidate={} proof_residual={:?}",
                            rejection.atom, candidate, residual
                        ),
                    }
                }

                let Some(experiment) = experiment_direct_complement_relation(scope)
                    .unwrap_or_else(|err| panic!("P7c-lite failed in scope {}: {err}", scope.scope))
                else {
                    continue;
                };
                let result_nodes = experiment
                    .simplified_after_substitution
                    .as_ref()
                    .map(Expr::size);
                let residual_zero = experiment
                    .simplified_after_substitution
                    .as_ref()
                    .is_some_and(|result| *result == Expr::zero());
                let proof = match &experiment.proof {
                    ComplementRelationProof::Proved => "proved",
                    ComplementRelationProof::NotProved { .. } => "not_proved",
                    ComplementRelationProof::ProofError(_) => "proof_error",
                };
                println!(
                    "p7c_lite line=369 scope={} left=v{} right=v{} proof={} result_nodes={:?} residual_zero={}",
                    experiment.scope,
                    experiment.left,
                    experiment.right,
                    proof,
                    result_nodes,
                    residual_zero,
                );
                println!("  relation={}", experiment.relation);
                println!("  left_definition={}", experiment.left_definition);
                println!("  right_definition={}", experiment.right_definition);
                match experiment.proof {
                    ComplementRelationProof::Proved => {}
                    ComplementRelationProof::NotProved { residual } => {
                        println!("  proof_residual={residual}");
                    }
                    ComplementRelationProof::ProofError(error) => {
                        println!("  proof_error={error}");
                    }
                }
            }
        } else {
            for scope in residual_trace.iter().filter(|scope| scope.input == residual) {
                let Some(experiment) = experiment_guided_semantic_aliases(scope)
                    .unwrap_or_else(|err| panic!("P7d-lite failed in scope {}: {err}", scope.scope))
                else {
                    continue;
                };
                let proved = experiment
                    .attempts
                    .iter()
                    .filter(|attempt| attempt.proof == SemanticAliasProof::Proved)
                    .count();
                let proof_errors = experiment
                    .attempts
                    .iter()
                    .filter(|attempt| matches!(attempt.proof, SemanticAliasProof::ProofError(_)))
                    .count();
                println!(
                    "p7d_lite line={} scope={} direct_atoms={:?} candidate_pairs={:?} attempts={} proved={} proof_errors={} substitutions={} result_nodes={} residual_zero={}",
                    line,
                    experiment.scope,
                    experiment.direct_atoms,
                    experiment.candidate_pairs,
                    experiment.attempts.len(),
                    proved,
                    proof_errors,
                    experiment.certified_aliases.len(),
                    experiment.simplified_after_substitution.size(),
                    experiment.simplified_after_substitution == Expr::zero(),
                );
                for alias in &experiment.certified_aliases {
                    println!(
                        "  alias left=v{} right=v{} kind={:?}",
                        alias.left, alias.right, alias.alias
                    );
                }
                if verbose {
                    println!(
                        "  substituted_pre_restore={}",
                        experiment.substituted_pre_restore
                    );
                    println!(
                        "  simplified_after_substitution={}",
                        experiment.simplified_after_substitution
                    );
                    for attempt in &experiment.attempts {
                        println!(
                            "  attempt left=v{} right=v{} kind={:?} proof={:?} relation={}",
                            attempt.left,
                            attempt.right,
                            attempt.alias,
                            attempt.proof,
                            attempt.relation,
                        );
                    }
                }

                let expected_dependency = match line {
                    53 => Some(ExpectedBitwiseDependency {
                        line,
                        target: 7.into(),
                        candidate: !(Expr::Var(2.into()) | Expr::Var(6.into())),
                    }),
                    249 => Some(ExpectedBitwiseDependency {
                        line,
                        target: 4.into(),
                        candidate: Expr::Var(1.into()) ^ Expr::Var(3.into()),
                    }),
                    260 => Some(ExpectedBitwiseDependency {
                        line,
                        target: 10.into(),
                        candidate: !(Expr::Var(0.into()) | Expr::Var(8.into())),
                    }),
                    _ => None,
                };
                if let Some(dependency) = expected_dependency {
                    let Some(expected) = experiment_expected_bitwise_dependency(
                        scope,
                        &dependency,
                        &experiment.certified_aliases,
                    )
                    .unwrap_or_else(|err| {
                        panic!("P7e-micro failed in scope {}: {err}", scope.scope)
                    })
                    else {
                        continue;
                    };
                    let proof = match &expected.proof {
                        SemanticAliasProof::Proved => "proved",
                        SemanticAliasProof::NotProved { .. } => "not_proved",
                        SemanticAliasProof::ProofError(_) => "proof_error",
                    };
                    let result_nodes = expected
                        .simplified_after_substitution
                        .as_ref()
                        .map(Expr::size);
                    let residual_zero = expected
                        .simplified_after_substitution
                        .as_ref()
                        .is_some_and(|result| *result == Expr::zero());
                    println!(
                        "p7e_micro line={} scope={} target=v{} candidate={} proof={} initial_aliases={} result_nodes={:?} residual_zero={}",
                        line,
                        expected.scope,
                        expected.target,
                        expected.candidate,
                        proof,
                        expected.initial_aliases.len(),
                        result_nodes,
                        residual_zero,
                    );
                    if verbose {
                        println!("  target_definition={}", expected.target_definition);
                        println!("  expanded_candidate={}", expected.expanded_candidate);
                        println!("  relation={}", expected.relation);
                        println!(
                            "  substituted_pre_restore={:?}",
                            expected.substituted_pre_restore
                        );
                        println!(
                            "  restored_after_substitution={:?}",
                            expected.restored_after_substitution
                        );
                        println!(
                            "  simplified_after_substitution={:?}",
                            expected.simplified_after_substitution
                        );
                    }
                }

                if line == 260 {
                    let Some(ternary) = experiment_guided_ternary_relations(scope)
                        .unwrap_or_else(|err| {
                            panic!("P7f-micro failed in scope {}: {err}", scope.scope)
                        })
                    else {
                        continue;
                    };
                    let proved = ternary
                        .attempts
                        .iter()
                        .filter(|attempt| attempt.proof == SemanticAliasProof::Proved)
                        .count();
                    let proof_errors = ternary
                        .attempts
                        .iter()
                        .filter(|attempt| {
                            matches!(attempt.proof, SemanticAliasProof::ProofError(_))
                        })
                        .count();
                    println!(
                        "p7f_micro line=260 scope={} remaining_atoms={:?} occurrence_counts={:?} attempts={} proved={} proof_errors={} substitutions={} result_nodes={} residual_zero={}",
                        ternary.scope,
                        ternary.remaining_atoms,
                        ternary.occurrence_counts,
                        ternary.attempts.len(),
                        proved,
                        proof_errors,
                        ternary.certified_relations.len(),
                        ternary.simplified_after_substitution.size(),
                        ternary.simplified_after_substitution == Expr::zero(),
                    );
                    println!(
                        "  residual_after_binary_aliases={}",
                        ternary.residual_after_binary_aliases
                    );
                    for relation in &ternary.certified_relations {
                        println!(
                            "  ternary atoms={:?} kind={:?} target=v{} replacement={}",
                            relation.atoms,
                            relation.kind,
                            relation.target,
                            relation.replacement,
                        );
                    }
                    println!(
                        "  simplified_after_ternary={}",
                        ternary.simplified_after_substitution
                    );
                    if verbose {
                        for attempt in &ternary.attempts {
                            println!(
                                "  ternary_attempt atoms={:?} kind={:?} proof={:?} relation={}",
                                attempt.atoms,
                                attempt.kind,
                                attempt.proof,
                                attempt.relation,
                            );
                        }
                    }
                }
            }
        }
    }
}
