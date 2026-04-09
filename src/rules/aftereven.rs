use crate::board::{Cell, Position};
use crate::facts::PositionFacts;
use crate::rules::{
    RuleGenerator, RuleKind, Solution, controller_live, problem_groups_solved_by_cells,
    unique_cells, unique_columns, unique_group_ids,
};

pub struct AfterevenRule;

impl RuleGenerator for AfterevenRule {
    fn rule_kind(&self) -> RuleKind {
        RuleKind::Aftereven
    }

    fn generate(&self, position: &Position, facts: &PositionFacts) -> Vec<Solution> {
        let mut solutions = Vec::new();

        for (group, _) in controller_live(facts) {
            let empty_cells: Vec<_> = group
                .cells
                .iter()
                .copied()
                .filter(|cell| position.board.get(*cell).is_none())
                .collect();

            if empty_cells.is_empty() {
                continue;
            }

            if !empty_cells.iter().all(|cell| {
                cell.is_even_row()
                    && cell.row > 1
                    && position.board.get(Cell::new(cell.col, cell.row - 1)).is_none()
                    && facts.mask.enabled_columns[cell.col]
            }) {
                continue;
            }

            let mut solved = problem_groups_solved_by_cells(facts, |candidate| {
                empty_cells.iter().all(|empty| {
                    candidate
                        .cells
                        .iter()
                        .any(|cell| cell.col == empty.col && cell.row > empty.row)
                })
            });

            solved.extend(problem_groups_solved_by_cells(facts, |candidate| {
                empty_cells.iter().any(|empty| candidate.cells.contains(empty))
            }));

            let solves_groups = unique_group_ids(solved);
            if solves_groups.is_empty() {
                continue;
            }

            let involved_squares = unique_cells(
                empty_cells
                    .iter()
                    .flat_map(|cell| [Cell::new(cell.col, cell.row - 1), *cell]),
            );
            let involved_columns = unique_columns(&involved_squares);

            solutions.push(Solution {
                id: 0,
                rule_kind: self.rule_kind(),
                involved_squares,
                involved_columns,
                zugzwang_dependent: true,
                solves_groups,
                explanation: format!("Aftereven on group {}", group.id),
            });
        }

        solutions
    }
}
