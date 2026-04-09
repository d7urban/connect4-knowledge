use crate::board::{Cell, Position};
use crate::facts::PositionFacts;
use crate::rules::{
    RuleGenerator, RuleKind, Solution, problem_groups_solved_by_cells, unique_group_ids,
};

pub struct BaseclaimRule;

impl RuleGenerator for BaseclaimRule {
    fn rule_kind(&self) -> RuleKind {
        RuleKind::Baseclaim
    }

    fn generate(&self, position: &Position, facts: &PositionFacts) -> Vec<Solution> {
        let playable_cells: Vec<_> = position
            .legal_moves()
            .into_iter()
            .filter_map(|column| position.board.playable_cell(column).ok())
            .collect();

        let mut solutions = Vec::new();

        for first_idx in 0..playable_cells.len() {
            for second_idx in 0..playable_cells.len() {
                if second_idx == first_idx {
                    continue;
                }

                let first = playable_cells[first_idx];
                let second = playable_cells[second_idx];
                if second.row >= crate::board::ROWS {
                    continue;
                }
                let upper_second = Cell::new(second.col, second.row + 1);
                if !upper_second.is_even_row() || position.board.get(upper_second).is_some() {
                    continue;
                }

                for (third_idx, third) in playable_cells.iter().copied().enumerate() {
                    if third_idx == first_idx || third_idx == second_idx {
                        continue;
                    }

                    let solves_groups = unique_group_ids(
                        problem_groups_solved_by_cells(facts, |group| {
                            group.cells.contains(&first) && group.cells.contains(&upper_second)
                        })
                        .into_iter()
                        .chain(problem_groups_solved_by_cells(facts, |group| {
                            group.cells.contains(&second) && group.cells.contains(&third)
                        })),
                    );
                    if solves_groups.is_empty() {
                        continue;
                    }

                    solutions.push(Solution {
                        id: 0,
                        rule_kind: self.rule_kind(),
                        involved_squares: vec![first, second, third, upper_second],
                        involved_columns: vec![first.col, second.col, third.col],
                        zugzwang_dependent: true,
                        solves_groups,
                        explanation: format!(
                            "Baseclaim {}{}, {}{}, {}{}",
                            column_name(first.col),
                            first.row,
                            column_name(second.col),
                            second.row,
                            column_name(third.col),
                            third.row
                        ),
                    });
                }
            }
        }

        solutions
    }
}

fn column_name(column: usize) -> char {
    (b'a' + column as u8) as char
}
