use crate::board::{Cell, Position};
use crate::facts::PositionFacts;
use crate::rules::{
    RuleGenerator, RuleKind, Solution, problem_groups_solved_by_cells, unique_columns,
};

pub struct VerticalRule;

impl RuleGenerator for VerticalRule {
    fn rule_kind(&self) -> RuleKind {
        RuleKind::Vertical
    }

    fn generate(&self, position: &Position, facts: &PositionFacts) -> Vec<Solution> {
        let mut solutions = Vec::new();

        for col in 0..crate::board::COLUMNS {
            if !facts.mask.enabled_columns[col] {
                continue;
            }

            for lower_row in 1..crate::board::ROWS {
                let lower = Cell::new(col, lower_row);
                let upper = Cell::new(col, lower_row + 1);
                if !upper.is_odd_row() {
                    continue;
                }
                if position.board.get(lower).is_some() || position.board.get(upper).is_some() {
                    continue;
                }

                let solves_groups = problem_groups_solved_by_cells(facts, |group| {
                    group.cells.contains(&lower) && group.cells.contains(&upper)
                });
                if solves_groups.is_empty() {
                    continue;
                }

                let involved_squares = vec![lower, upper];
                solutions.push(Solution {
                    id: 0,
                    rule_kind: self.rule_kind(),
                    involved_columns: unique_columns(&involved_squares),
                    involved_squares,
                    zugzwang_dependent: false,
                    solves_groups,
                    explanation: format!("Vertical {}{}-{}{}", column_name(col), lower.row, column_name(col), upper.row),
                });
            }
        }

        solutions
    }
}

fn column_name(column: usize) -> char {
    (b'a' + column as u8) as char
}
