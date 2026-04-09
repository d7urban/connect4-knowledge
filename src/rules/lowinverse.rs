use crate::board::{Cell, Position};
use crate::facts::PositionFacts;
use crate::rules::{
    RuleGenerator, RuleKind, Solution, problem_groups_solved_by_cells, unique_group_ids,
};

pub struct LowinverseRule;

impl RuleGenerator for LowinverseRule {
    fn rule_kind(&self) -> RuleKind {
        RuleKind::Lowinverse
    }

    fn generate(&self, position: &Position, facts: &PositionFacts) -> Vec<Solution> {
        let mut solutions = Vec::new();

        for left_col in 0..crate::board::COLUMNS {
            if !facts.mask.enabled_columns[left_col] {
                continue;
            }
            for right_col in (left_col + 1)..crate::board::COLUMNS {
                if !facts.mask.enabled_columns[right_col] {
                    continue;
                }
                for left_lower_row in 1..crate::board::ROWS {
                    let left_lower = Cell::new(left_col, left_lower_row);
                    let left_upper = Cell::new(left_col, left_lower_row + 1);
                    if !left_upper.is_odd_row()
                        || position.board.get(left_lower).is_some()
                        || position.board.get(left_upper).is_some()
                    {
                        continue;
                    }

                    for right_lower_row in 1..crate::board::ROWS {
                        let right_lower = Cell::new(right_col, right_lower_row);
                        let right_upper = Cell::new(right_col, right_lower_row + 1);
                        if !right_upper.is_odd_row()
                            || position.board.get(right_lower).is_some()
                            || position.board.get(right_upper).is_some()
                        {
                            continue;
                        }

                        let solves_groups = unique_group_ids(
                            problem_groups_solved_by_cells(facts, |group| {
                                group.cells.contains(&left_upper) && group.cells.contains(&right_upper)
                            })
                            .into_iter()
                            .chain(problem_groups_solved_by_cells(facts, |group| {
                                group.cells.contains(&left_lower) && group.cells.contains(&left_upper)
                            }))
                            .chain(problem_groups_solved_by_cells(facts, |group| {
                                group.cells.contains(&right_lower) && group.cells.contains(&right_upper)
                            })),
                        );

                        if solves_groups.is_empty() {
                            continue;
                        }

                        solutions.push(Solution {
                            id: 0,
                            rule_kind: self.rule_kind(),
                            involved_squares: vec![left_lower, left_upper, right_lower, right_upper],
                            involved_columns: vec![left_col, right_col],
                            zugzwang_dependent: true,
                            solves_groups,
                            explanation: format!(
                                "Lowinverse {}{}-{}{} and {}{}-{}{}",
                                column_name(left_col),
                                left_lower.row,
                                column_name(left_col),
                                left_upper.row,
                                column_name(right_col),
                                right_lower.row,
                                column_name(right_col),
                                right_upper.row
                            ),
                        });
                    }
                }
            }
        }

        solutions
    }
}

fn column_name(column: usize) -> char {
    (b'a' + column as u8) as char
}
