use crate::rules::{RuleKind, Solution};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompatibilityConstraint {
    DisjointSquares,
    NoClaimevenBelowInverse,
    ColumnWiseDisjointOrEqual,
    DisjointSquaresAndInverseColumnsDisjointOrEqual,
}

pub fn are_compatible(left: &Solution, right: &Solution) -> bool {
    if left.id == right.id {
        return true;
    }

    for constraint in constraints_for(left.rule_kind, right.rule_kind) {
        let ok = match constraint {
            CompatibilityConstraint::DisjointSquares => disjoint_squares(left, right),
            CompatibilityConstraint::NoClaimevenBelowInverse => no_claimeven_below_inverse(left, right),
            CompatibilityConstraint::ColumnWiseDisjointOrEqual => column_wise_disjoint_or_equal(left, right),
            CompatibilityConstraint::DisjointSquaresAndInverseColumnsDisjointOrEqual => {
                disjoint_squares(left, right) && inverse_columns_disjoint_or_equal(left, right)
            }
        };
        if !ok {
            return false;
        }
    }

    true
}

pub fn build_incompatibility_matrix(solutions: &[Solution]) -> Vec<Vec<bool>> {
    let mut matrix = vec![vec![false; solutions.len()]; solutions.len()];
    for i in 0..solutions.len() {
        for j in (i + 1)..solutions.len() {
            let incompatible = !are_compatible(&solutions[i], &solutions[j]);
            matrix[i][j] = incompatible;
            matrix[j][i] = incompatible;
        }
    }
    matrix
}

fn constraints_for(left: RuleKind, right: RuleKind) -> Vec<CompatibilityConstraint> {
    use CompatibilityConstraint as C;
    use RuleKind as R;

    match (left, right) {
        (R::Claimeven, R::Claimeven)
        | (R::Claimeven, R::Baseinverse)
        | (R::Claimeven, R::Vertical)
        | (R::Claimeven, R::Baseclaim)
        | (R::Baseinverse, R::Baseinverse)
        | (R::Baseinverse, R::Vertical)
        | (R::Baseinverse, R::Aftereven)
        | (R::Baseinverse, R::Baseclaim)
        | (R::Baseinverse, R::Before)
        | (R::Baseinverse, R::Specialbefore)
        | (R::Vertical, R::Vertical)
        | (R::Vertical, R::Aftereven)
        | (R::Vertical, R::Lowinverse)
        | (R::Vertical, R::Highinverse)
        | (R::Vertical, R::Baseclaim)
        | (R::Vertical, R::Before)
        | (R::Vertical, R::Specialbefore)
        | (R::Aftereven, R::Baseclaim)
        | (R::Highinverse, R::Baseclaim)
        | (R::Baseclaim, R::Baseclaim) => vec![C::DisjointSquares],
        (R::Claimeven, R::Aftereven)
        | (R::Claimeven, R::Before)
        | (R::Claimeven, R::Specialbefore)
        | (R::Aftereven, R::Before)
        | (R::Aftereven, R::Specialbefore)
        | (R::Before, R::Before)
        | (R::Before, R::Specialbefore)
        | (R::Specialbefore, R::Specialbefore) => vec![C::ColumnWiseDisjointOrEqual],
        (R::Claimeven, R::Lowinverse)
        | (R::Claimeven, R::Highinverse)
        | (R::Aftereven, R::Lowinverse)
        | (R::Aftereven, R::Highinverse)
        | (R::Baseclaim, R::Lowinverse)
        | (R::Baseclaim, R::Highinverse) => vec![C::NoClaimevenBelowInverse],
        (R::Lowinverse, R::Lowinverse) | (R::Lowinverse, R::Highinverse) | (R::Highinverse, R::Highinverse) => {
            vec![C::DisjointSquaresAndInverseColumnsDisjointOrEqual]
        }
        (R::Lowinverse, R::Before)
        | (R::Lowinverse, R::Specialbefore)
        | (R::Highinverse, R::Before)
        | (R::Highinverse, R::Specialbefore) => {
            vec![C::NoClaimevenBelowInverse, C::ColumnWiseDisjointOrEqual]
        }
        _ => constraints_for(right, left),
    }
}

fn disjoint_squares(left: &Solution, right: &Solution) -> bool {
    left.involved_squares
        .iter()
        .all(|cell| !right.involved_squares.contains(cell))
}

fn column_wise_disjoint_or_equal(left: &Solution, right: &Solution) -> bool {
    for &left_col in &left.involved_columns {
        for &right_col in &right.involved_columns {
            if left_col == right_col {
                let left_cells: Vec<_> = left.involved_squares.iter().filter(|c| c.col == left_col).collect();
                let right_cells: Vec<_> = right.involved_squares.iter().filter(|c| c.col == right_col).collect();
                if left_cells != right_cells {
                    return false;
                }
            }
        }
    }
    true
}

fn inverse_columns_disjoint_or_equal(left: &Solution, right: &Solution) -> bool {
    let shared: Vec<_> = left
        .involved_columns
        .iter()
        .copied()
        .filter(|col| right.involved_columns.contains(col))
        .collect();
    shared.is_empty() || shared == left.involved_columns || shared == right.involved_columns
}

fn no_claimeven_below_inverse(left: &Solution, right: &Solution) -> bool {
    let (claimeven, inverse) = match (left.rule_kind, right.rule_kind) {
        (RuleKind::Claimeven, RuleKind::Lowinverse | RuleKind::Highinverse) => (left, right),
        (RuleKind::Lowinverse | RuleKind::Highinverse, RuleKind::Claimeven) => (right, left),
        (RuleKind::Aftereven | RuleKind::Before | RuleKind::Specialbefore, RuleKind::Lowinverse | RuleKind::Highinverse) => {
            (left, right)
        }
        (RuleKind::Lowinverse | RuleKind::Highinverse, RuleKind::Aftereven | RuleKind::Before | RuleKind::Specialbefore) => {
            (right, left)
        }
        _ => return true,
    };

    claimeven.involved_squares.iter().all(|claim_cell| {
        inverse
            .involved_squares
            .iter()
            .filter(|inverse_cell| inverse_cell.col == claim_cell.col)
            .all(|inverse_cell| claim_cell.row >= inverse_cell.row)
    })
}
