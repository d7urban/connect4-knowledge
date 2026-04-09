use crate::board::{Board, COLUMNS, Cell, Color, ROWS, WIN_LEN};

pub const STANDARD_GROUP_COUNT: usize = 69;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Orientation {
    Horizontal,
    Vertical,
    RisingDiagonal,
    FallingDiagonal,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Group {
    pub id: usize,
    pub cells: [Cell; WIN_LEN],
    pub orientation: Orientation,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GroupState {
    pub group_id: usize,
    pub white_count: usize,
    pub black_count: usize,
    pub empty_count: usize,
    pub live_for_white: bool,
    pub live_for_black: bool,
}

pub fn standard_groups() -> Vec<Group> {
    let mut groups = Vec::with_capacity(STANDARD_GROUP_COUNT);
    let mut id = 0usize;

    for row in 1..=ROWS {
        for col in 0..=COLUMNS - WIN_LEN {
            groups.push(Group {
                id,
                cells: core::array::from_fn(|i| Cell::new(col + i, row)),
                orientation: Orientation::Horizontal,
            });
            id += 1;
        }
    }

    for col in 0..COLUMNS {
        for row in 1..=ROWS - (WIN_LEN - 1) {
            groups.push(Group {
                id,
                cells: core::array::from_fn(|i| Cell::new(col, row + i)),
                orientation: Orientation::Vertical,
            });
            id += 1;
        }
    }

    for col in 0..=COLUMNS - WIN_LEN {
        for row in 1..=ROWS - (WIN_LEN - 1) {
            groups.push(Group {
                id,
                cells: core::array::from_fn(|i| Cell::new(col + i, row + i)),
                orientation: Orientation::RisingDiagonal,
            });
            id += 1;
        }
    }

    for col in 0..=COLUMNS - WIN_LEN {
        for row in WIN_LEN..=ROWS {
            groups.push(Group {
                id,
                cells: core::array::from_fn(|i| Cell::new(col + i, row - i)),
                orientation: Orientation::FallingDiagonal,
            });
            id += 1;
        }
    }

    groups
}

pub fn evaluate_groups(board: &Board, groups: &[Group]) -> Vec<GroupState> {
    groups
        .iter()
        .map(|group| {
            let mut white_count = 0usize;
            let mut black_count = 0usize;

            for &cell in &group.cells {
                match board.get(cell) {
                    Some(Color::White) => white_count += 1,
                    Some(Color::Black) => black_count += 1,
                    None => {}
                }
            }

            let empty_count = WIN_LEN - white_count - black_count;
            GroupState {
                group_id: group.id,
                white_count,
                black_count,
                empty_count,
                live_for_white: black_count == 0,
                live_for_black: white_count == 0,
            }
        })
        .collect()
}
