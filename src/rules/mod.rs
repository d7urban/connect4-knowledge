use std::collections::BTreeSet;

use crate::board::{Cell, Position};
use crate::facts::PositionFacts;
use crate::groups::{Group, GroupState};

pub mod aftereven;
pub mod baseclaim;
pub mod baseinverse;
pub mod before;
pub mod claimeven;
pub mod highinverse;
pub mod lowinverse;
pub mod specialbefore;
pub mod vertical;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RuleKind {
    Claimeven,
    Baseinverse,
    Vertical,
    Aftereven,
    Lowinverse,
    Highinverse,
    Baseclaim,
    Before,
    Specialbefore,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Solution {
    pub id: usize,
    pub rule_kind: RuleKind,
    pub involved_squares: Vec<Cell>,
    pub involved_columns: Vec<usize>,
    pub zugzwang_dependent: bool,
    pub solves_groups: Vec<usize>,
    pub explanation: String,
}

pub trait RuleGenerator {
    fn rule_kind(&self) -> RuleKind;
    fn generate(&self, position: &Position, facts: &PositionFacts) -> Vec<Solution>;
}

pub fn generate_all(position: &Position, facts: &PositionFacts) -> Vec<Solution> {
    let generators: Vec<Box<dyn RuleGenerator>> = vec![
        Box::new(claimeven::ClaimevenRule),
        Box::new(baseinverse::BaseinverseRule),
        Box::new(vertical::VerticalRule),
        Box::new(aftereven::AfterevenRule),
        Box::new(lowinverse::LowinverseRule),
        Box::new(highinverse::HighinverseRule),
        Box::new(baseclaim::BaseclaimRule),
        Box::new(before::BeforeRule),
        Box::new(specialbefore::SpecialbeforeRule),
    ];

    let mut next_id = 0usize;
    let mut solutions = Vec::new();

    for generator in generators {
        for mut solution in generator.generate(position, facts) {
            if !solution.solves_groups.is_empty() {
                solution.id = next_id;
                next_id += 1;
                solutions.push(solution);
            }
        }
    }

    solutions
}

pub(crate) fn controller_live<'a>(
    facts: &'a PositionFacts,
) -> impl Iterator<Item = (&'a Group, &'a GroupState)> + 'a {
    facts
        .groups
        .iter()
        .zip(facts.states.iter())
        .filter(|(_, state)| match facts.controller_of_zugzwang {
            crate::board::Color::White => state.live_for_white,
            crate::board::Color::Black => state.live_for_black,
        })
}

pub(crate) fn problem_ids(facts: &PositionFacts) -> BTreeSet<usize> {
    facts.problems.iter().map(|problem| problem.group_id).collect()
}

pub(crate) fn problem_groups_solved_by_cells(
    facts: &PositionFacts,
    predicate: impl Fn(&Group) -> bool,
) -> Vec<usize> {
    let ids = problem_ids(facts);
    facts.groups
        .iter()
        .filter(|group| ids.contains(&group.id) && predicate(group))
        .map(|group| group.id)
        .collect()
}

pub(crate) fn unique_cells(cells: impl IntoIterator<Item = Cell>) -> Vec<Cell> {
    let mut seen = BTreeSet::new();
    cells.into_iter()
        .filter(|cell| seen.insert((cell.col, cell.row)))
        .collect()
}

pub(crate) fn unique_columns(cells: &[Cell]) -> Vec<usize> {
    let mut cols = BTreeSet::new();
    cells.iter()
        .filter_map(|cell| cols.insert(cell.col).then_some(cell.col))
        .collect()
}

pub(crate) fn unique_group_ids(ids: impl IntoIterator<Item = usize>) -> Vec<usize> {
    let mut seen = BTreeSet::new();
    ids.into_iter().filter(|id| seen.insert(*id)).collect()
}
