use crate::board::{Cell, Position, ROWS};
use crate::facts::PositionFacts;
use crate::rules::{
    RuleGenerator, RuleKind, Solution, controller_live, problem_groups_solved_by_cells,
    unique_cells, unique_columns, unique_group_ids,
};

pub struct BeforeRule;

#[derive(Clone, Copy)]
enum BeforePart {
    Claimeven { lower: Cell, upper: Cell },
    Vertical { lower: Cell, upper: Cell },
}

impl RuleGenerator for BeforeRule {
    fn rule_kind(&self) -> RuleKind {
        RuleKind::Before
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

            if empty_cells.is_empty()
                || empty_cells.iter().any(|cell| cell.row == ROWS || !facts.mask.enabled_columns[cell.col])
            {
                continue;
            }

            let mut choices = Vec::new();
            let mut valid = true;
            for empty in &empty_cells {
                let options = before_options(position, *empty);
                if options.is_empty() {
                    valid = false;
                    break;
                }
                choices.push(options);
            }
            if !valid {
                continue;
            }

            let mut combos = Vec::new();
            enumerate_before_parts(&choices, 0, &mut Vec::new(), &mut combos);

            for combo in combos {
                if combo
                    .iter()
                    .all(|part| matches!(part, BeforePart::Claimeven { .. }))
                {
                    continue;
                }

                let successor_cells: Vec<_> =
                    empty_cells.iter().map(|cell| Cell::new(cell.col, cell.row + 1)).collect();
                let mut solved = problem_groups_solved_by_cells(facts, |candidate| {
                    successor_cells.iter().all(|cell| candidate.cells.contains(cell))
                });

                for part in &combo {
                    match part {
                        BeforePart::Claimeven { upper, .. } => {
                            solved.extend(problem_groups_solved_by_cells(facts, |candidate| {
                                candidate.cells.contains(upper)
                            }));
                        }
                        BeforePart::Vertical { lower, upper } => {
                            solved.extend(problem_groups_solved_by_cells(facts, |candidate| {
                                candidate.cells.contains(lower) && candidate.cells.contains(upper)
                            }));
                        }
                    }
                }

                let solves_groups = unique_group_ids(solved);
                if solves_groups.is_empty() {
                    continue;
                }

                let involved_squares = unique_cells(combo.iter().flat_map(|part| match part {
                    BeforePart::Claimeven { lower, upper } | BeforePart::Vertical { lower, upper } => {
                        [*lower, *upper]
                    }
                }));
                let involved_columns = unique_columns(&involved_squares);

                solutions.push(Solution {
                    id: 0,
                    rule_kind: self.rule_kind(),
                    involved_squares,
                    involved_columns,
                    zugzwang_dependent: true,
                    solves_groups,
                    explanation: format!("Before on group {}", group.id),
                });
            }
        }

        solutions
    }
}

fn before_options(position: &Position, empty: Cell) -> Vec<BeforePart> {
    let mut options = Vec::new();

    if empty.is_even_row() && empty.row > 1 {
        let lower = Cell::new(empty.col, empty.row - 1);
        if position.board.get(lower).is_none() {
            options.push(BeforePart::Claimeven { lower, upper: empty });
        }
    }

    if empty.row < ROWS {
        let upper = Cell::new(empty.col, empty.row + 1);
        if position.board.get(upper).is_none() {
            options.push(BeforePart::Vertical { lower: empty, upper });
        }
    }

    options
}

fn enumerate_before_parts(
    choices: &[Vec<BeforePart>],
    idx: usize,
    current: &mut Vec<BeforePart>,
    out: &mut Vec<Vec<BeforePart>>,
) {
    if idx == choices.len() {
        out.push(current.clone());
        return;
    }

    for option in &choices[idx] {
        current.push(*option);
        enumerate_before_parts(choices, idx + 1, current, out);
        current.pop();
    }
}
