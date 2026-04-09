use serde::{Deserialize, Serialize};

use crate::compat::build_incompatibility_matrix;
use crate::facts::Problem;
use crate::rules::{RuleKind, Solution};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Verdict {
    SolvedWin,
    SolvedDraw,
    Unresolved,
}

#[derive(Debug, Clone)]
pub struct Proof {
    pub verdict: Verdict,
    pub chosen_solution_ids: Vec<usize>,
    pub unresolved_problem_ids: Vec<usize>,
}

pub fn solve_cover(problems: &[Problem], solutions: &[Solution]) -> Proof {
    if problems.is_empty() {
        return Proof {
            verdict: Verdict::SolvedDraw,
            chosen_solution_ids: Vec::new(),
            unresolved_problem_ids: Vec::new(),
        };
    }

    let incompat = build_incompatibility_matrix(solutions);
    let mut available = vec![true; solutions.len()];
    let mut chosen = Vec::new();

    if backtrack(problems, solutions, &incompat, &mut available, &mut chosen) {
        let verdict = if chosen
            .iter()
            .any(|&idx| solutions[idx].rule_kind == RuleKind::Aftereven)
        {
            Verdict::SolvedWin
        } else {
            Verdict::SolvedDraw
        };
        Proof {
            verdict,
            chosen_solution_ids: chosen.into_iter().map(|idx| solutions[idx].id).collect(),
            unresolved_problem_ids: Vec::new(),
        }
    } else {
        Proof {
            verdict: Verdict::Unresolved,
            chosen_solution_ids: Vec::new(),
            unresolved_problem_ids: problems.iter().map(|problem| problem.group_id).collect(),
        }
    }
}

fn backtrack(
    problems: &[Problem],
    solutions: &[Solution],
    incompat: &[Vec<bool>],
    available: &mut [bool],
    chosen: &mut Vec<usize>,
) -> bool {
    let Some(problem_idx) = pick_problem(problems, solutions, available) else {
        return true;
    };

    let problem = &problems[problem_idx];
    let candidates: Vec<_> = solutions
        .iter()
        .enumerate()
        .filter(|(idx, solution)| available[*idx] && solution.solves_groups.contains(&problem.group_id))
        .map(|(idx, _)| idx)
        .collect();

    if candidates.is_empty() {
        return false;
    }

    for candidate in candidates {
        let snapshot = available.to_vec();
        chosen.push(candidate);

        for (idx, is_available) in available.iter_mut().enumerate() {
            if incompat[candidate][idx] {
                *is_available = false;
            }
        }
        available[candidate] = false;

        let remaining: Vec<_> = problems
            .iter()
            .filter(|problem| !solutions[candidate].solves_groups.contains(&problem.group_id))
            .cloned()
            .collect();

        if backtrack(&remaining, solutions, incompat, available, chosen) {
            return true;
        }

        available.copy_from_slice(&snapshot);
        chosen.pop();
    }

    false
}

fn pick_problem(problems: &[Problem], solutions: &[Solution], available: &[bool]) -> Option<usize> {
    problems
        .iter()
        .enumerate()
        .min_by_key(|(_, problem)| {
            solutions
                .iter()
                .enumerate()
                .filter(|(idx, solution)| available[*idx] && solution.solves_groups.contains(&problem.group_id))
                .count()
        })
        .map(|(idx, _)| idx)
}
