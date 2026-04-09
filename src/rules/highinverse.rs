use crate::board::{Cell, Column, Position};
use crate::facts::PositionFacts;
use crate::rules::{
    RuleGenerator, RuleKind, Solution, problem_groups_solved_by_cells, unique_group_ids,
};

pub struct HighinverseRule;

impl RuleGenerator for HighinverseRule {
    fn rule_kind(&self) -> RuleKind {
        RuleKind::Highinverse
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
                for left_bottom_row in 1..=(crate::board::ROWS - 2) {
                    let left_low = Cell::new(left_col, left_bottom_row);
                    let left_mid = Cell::new(left_col, left_bottom_row + 1);
                    let left_high = Cell::new(left_col, left_bottom_row + 2);
                    if !left_high.is_even_row()
                        || [left_low, left_mid, left_high]
                            .into_iter()
                            .any(|cell| position.board.get(cell).is_some())
                    {
                        continue;
                    }

                    for right_bottom_row in 1..=(crate::board::ROWS - 2) {
                        let right_low = Cell::new(right_col, right_bottom_row);
                        let right_mid = Cell::new(right_col, right_bottom_row + 1);
                        let right_high = Cell::new(right_col, right_bottom_row + 2);
                        if !right_high.is_even_row()
                            || [right_low, right_mid, right_high]
                                .into_iter()
                                .any(|cell| position.board.get(cell).is_some())
                        {
                            continue;
                        }

                        let mut solved = problem_groups_solved_by_cells(facts, |group| {
                            group.cells.contains(&left_high) && group.cells.contains(&right_high)
                        });
                        solved.extend(problem_groups_solved_by_cells(facts, |group| {
                            group.cells.contains(&left_mid) && group.cells.contains(&right_mid)
                        }));
                        solved.extend(problem_groups_solved_by_cells(facts, |group| {
                            group.cells.contains(&left_mid) && group.cells.contains(&left_high)
                        }));
                        solved.extend(problem_groups_solved_by_cells(facts, |group| {
                            group.cells.contains(&right_mid) && group.cells.contains(&right_high)
                        }));

                        if is_directly_playable(position, left_low) {
                            solved.extend(problem_groups_solved_by_cells(facts, |group| {
                                group.cells.contains(&left_low) && group.cells.contains(&right_high)
                            }));
                        }
                        if is_directly_playable(position, right_low) {
                            solved.extend(problem_groups_solved_by_cells(facts, |group| {
                                group.cells.contains(&right_low) && group.cells.contains(&left_high)
                            }));
                        }

                        let solves_groups = unique_group_ids(solved);
                        if solves_groups.is_empty() {
                            continue;
                        }

                        solutions.push(Solution {
                            id: 0,
                            rule_kind: self.rule_kind(),
                            involved_squares: vec![left_low, left_mid, left_high, right_low, right_mid, right_high],
                            involved_columns: vec![left_col, right_col],
                            zugzwang_dependent: true,
                            solves_groups,
                            explanation: format!(
                                "Highinverse {}{}-{}{} and {}{}-{}{}",
                                column_name(left_col),
                                left_low.row,
                                column_name(left_col),
                                left_high.row,
                                column_name(right_col),
                                right_low.row,
                                column_name(right_col),
                                right_high.row
                            ),
                        });
                    }
                }
            }
        }

        solutions
    }
}

fn is_directly_playable(position: &Position, cell: Cell) -> bool {
    position
        .board
        .playable_cell(Column(cell.col))
        .is_ok_and(|playable| playable.row == cell.row)
}

fn column_name(column: usize) -> char {
    (b'a' + column as u8) as char
}
