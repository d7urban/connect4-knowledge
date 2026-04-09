use crate::board::{Cell, Column, Position, ROWS};
use crate::facts::PositionFacts;
use crate::rules::{
    RuleGenerator, RuleKind, Solution, controller_live, problem_groups_solved_by_cells,
    unique_cells, unique_columns, unique_group_ids,
};

pub struct SpecialbeforeRule;

#[derive(Clone, Copy)]
enum SpecialBeforePart {
    Claimeven { lower: Cell, upper: Cell },
    Vertical { lower: Cell, upper: Cell },
}

impl RuleGenerator for SpecialbeforeRule {
    fn rule_kind(&self) -> RuleKind {
        RuleKind::Specialbefore
    }

    fn generate(&self, position: &Position, facts: &PositionFacts) -> Vec<Solution> {
        let playable_cells: Vec<_> = position
            .legal_moves()
            .into_iter()
            .filter_map(|column| position.board.playable_cell(column).ok())
            .collect();

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

            let playable_in_group: Vec<_> = empty_cells
                .iter()
                .copied()
                .filter(|cell| is_directly_playable(position, *cell))
                .collect();
            if playable_in_group.is_empty() {
                continue;
            }

            let mut choices = Vec::new();
            let mut valid = true;
            for empty in &empty_cells {
                let options = specialbefore_options(position, *empty);
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
            enumerate_specialbefore_parts(&choices, 0, &mut Vec::new(), &mut combos);

            for group_playable in &playable_in_group {
                for extra in &playable_cells {
                    if extra.col == group_playable.col {
                        continue;
                    }

                    for combo in &combos {
                        let successor_cells: Vec<_> =
                            empty_cells.iter().map(|cell| Cell::new(cell.col, cell.row + 1)).collect();
                        let mut solved = problem_groups_solved_by_cells(facts, |candidate| {
                            successor_cells.iter().all(|cell| candidate.cells.contains(cell))
                                && candidate.cells.contains(extra)
                        });
                        solved.extend(problem_groups_solved_by_cells(facts, |candidate| {
                            candidate.cells.contains(group_playable) && candidate.cells.contains(extra)
                        }));

                        for part in combo {
                            match part {
                                SpecialBeforePart::Claimeven { upper, .. } => {
                                    solved.extend(problem_groups_solved_by_cells(facts, |candidate| {
                                        candidate.cells.contains(upper)
                                    }));
                                }
                                SpecialBeforePart::Vertical { lower, upper } => {
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

                        let involved_squares = unique_cells(
                            combo.iter().flat_map(|part| match part {
                                SpecialBeforePart::Claimeven { lower, upper }
                                | SpecialBeforePart::Vertical { lower, upper } => [*lower, *upper],
                            })
                            .chain([*group_playable, *extra]),
                        );
                        let involved_columns = unique_columns(&involved_squares);

                        solutions.push(Solution {
                            id: 0,
                            rule_kind: self.rule_kind(),
                            involved_squares,
                            involved_columns,
                            zugzwang_dependent: true,
                            solves_groups,
                            explanation: format!("Specialbefore on group {}", group.id),
                        });
                    }
                }
            }
        }

        solutions
    }
}

fn specialbefore_options(position: &Position, empty: Cell) -> Vec<SpecialBeforePart> {
    let mut options = Vec::new();

    if empty.is_even_row() && empty.row > 1 {
        let lower = Cell::new(empty.col, empty.row - 1);
        if position.board.get(lower).is_none() {
            options.push(SpecialBeforePart::Claimeven { lower, upper: empty });
        }
    }
    if empty.row < ROWS {
        let upper = Cell::new(empty.col, empty.row + 1);
        if position.board.get(upper).is_none() {
            options.push(SpecialBeforePart::Vertical { lower: empty, upper });
        }
    }

    options
}

fn enumerate_specialbefore_parts(
    choices: &[Vec<SpecialBeforePart>],
    idx: usize,
    current: &mut Vec<SpecialBeforePart>,
    out: &mut Vec<Vec<SpecialBeforePart>>,
) {
    if idx == choices.len() {
        out.push(current.clone());
        return;
    }

    for option in &choices[idx] {
        current.push(*option);
        enumerate_specialbefore_parts(choices, idx + 1, current, out);
        current.pop();
    }
}

fn is_directly_playable(position: &Position, cell: Cell) -> bool {
    position
        .board
        .playable_cell(Column(cell.col))
        .is_ok_and(|playable| playable.row == cell.row)
}
