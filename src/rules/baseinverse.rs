use crate::board::Position;
use crate::facts::PositionFacts;
use crate::rules::{RuleGenerator, RuleKind, Solution, problem_groups_solved_by_cells};

pub struct BaseinverseRule;

impl RuleGenerator for BaseinverseRule {
    fn rule_kind(&self) -> RuleKind {
        RuleKind::Baseinverse
    }

    fn generate(&self, position: &Position, facts: &PositionFacts) -> Vec<Solution> {
        let playable = position.legal_moves();
        let mut solutions = Vec::new();

        for left_idx in 0..playable.len() {
            for right_idx in (left_idx + 1)..playable.len() {
                let left = match position.board.playable_cell(playable[left_idx]) {
                    Ok(cell) => cell,
                    Err(_) => continue,
                };
                let right = match position.board.playable_cell(playable[right_idx]) {
                    Ok(cell) => cell,
                    Err(_) => continue,
                };

                let solves_groups = problem_groups_solved_by_cells(facts, |group| {
                    group.cells.contains(&left) && group.cells.contains(&right)
                });
                if solves_groups.is_empty() {
                    continue;
                }

                solutions.push(Solution {
                    id: 0,
                    rule_kind: self.rule_kind(),
                    involved_squares: vec![left, right],
                    involved_columns: vec![left.col, right.col],
                    zugzwang_dependent: false,
                    solves_groups,
                    explanation: format!(
                        "Baseinverse {}{}-{}{}",
                        column_name(left.col),
                        left.row,
                        column_name(right.col),
                        right.row
                    ),
                });
            }
        }

        solutions
    }
}

fn column_name(column: usize) -> char {
    (b'a' + column as u8) as char
}
