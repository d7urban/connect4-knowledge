use crate::board::{Board, COLUMNS, Cell, Color, Position};
use crate::groups::{Group, GroupState, evaluate_groups, standard_groups};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThreatKind {
    Direct,
    Odd,
    Even,
    Combination,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Problem {
    pub group_id: usize,
    pub empty_cells: Vec<Cell>,
    pub threat_kind: ThreatKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EvaluationMask {
    pub enabled_columns: [bool; COLUMNS],
}

impl Default for EvaluationMask {
    fn default() -> Self {
        Self {
            enabled_columns: [true; COLUMNS],
        }
    }
}

#[derive(Debug, Clone)]
pub struct PositionFacts {
    pub groups: Vec<Group>,
    pub states: Vec<GroupState>,
    pub problems: Vec<Problem>,
    pub controller_of_zugzwang: Color,
    pub mask: EvaluationMask,
}

pub fn analyze_position(position: &Position, mask: EvaluationMask) -> PositionFacts {
    let groups = standard_groups();
    let states = evaluate_groups(&position.board, &groups);
    let controller_of_zugzwang = position.side_to_move.opponent();
    let opponent = controller_of_zugzwang.opponent();
    let problems = collect_problems(&position.board, &groups, &states, opponent, &mask);

    PositionFacts {
        groups,
        states,
        problems,
        controller_of_zugzwang,
        mask,
    }
}

fn collect_problems(
    board: &Board,
    groups: &[Group],
    states: &[GroupState],
    attacker: Color,
    mask: &EvaluationMask,
) -> Vec<Problem> {
    groups
        .iter()
        .zip(states.iter())
        .filter_map(|(group, state)| {
            let live = match attacker {
                Color::White => state.live_for_white,
                Color::Black => state.live_for_black,
            };

            if !live {
                return None;
            }

            let empty_cells: Vec<_> = group
                .cells
                .iter()
                .copied()
                .filter(|cell| board.get(*cell).is_none() && mask.enabled_columns[cell.col])
                .collect();

            if empty_cells.is_empty() {
                return None;
            }

            Some(Problem {
                group_id: group.id,
                threat_kind: classify_threat(&empty_cells),
                empty_cells,
            })
        })
        .collect()
}

fn classify_threat(empty_cells: &[Cell]) -> ThreatKind {
    match empty_cells.len() {
        1 => ThreatKind::Direct,
        2 if empty_cells.iter().any(|cell| cell.is_odd_row()) => ThreatKind::Odd,
        2 => ThreatKind::Even,
        _ => ThreatKind::Combination,
    }
}
