use std::fmt;

use serde::{Deserialize, Serialize};

pub const ROWS: usize = 6;
pub const COLUMNS: usize = 7;
pub const WIN_LEN: usize = 4;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Color {
    White,
    Black,
}

impl Color {
    pub fn opponent(self) -> Self {
        match self {
            Self::White => Self::Black,
            Self::Black => Self::White,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Column(pub usize);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Cell {
    pub col: usize,
    pub row: usize,
}

impl Cell {
    pub const fn new(col: usize, row: usize) -> Self {
        Self { col, row }
    }

    pub const fn is_odd_row(self) -> bool {
        self.row % 2 == 1
    }

    pub const fn is_even_row(self) -> bool {
        !self.is_odd_row()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MoveError {
    ColumnOutOfRange,
    ColumnFull,
    GameAlreadyOver,
}

impl fmt::Display for MoveError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ColumnOutOfRange => write!(f, "column out of range"),
            Self::ColumnFull => write!(f, "column is full"),
            Self::GameAlreadyOver => write!(f, "game is already over"),
        }
    }
}

impl std::error::Error for MoveError {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Board {
    cells: [[Option<Color>; ROWS]; COLUMNS],
    heights: [usize; COLUMNS],
}

impl Default for Board {
    fn default() -> Self {
        Self {
            cells: [[None; ROWS]; COLUMNS],
            heights: [0; COLUMNS],
        }
    }
}

impl Board {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn get(&self, cell: Cell) -> Option<Color> {
        if cell.col >= COLUMNS || cell.row == 0 || cell.row > ROWS {
            return None;
        }
        self.cells[cell.col][cell.row - 1]
    }

    pub fn heights(&self) -> [usize; COLUMNS] {
        self.heights
    }

    pub fn legal_moves(&self) -> Vec<Column> {
        (0..COLUMNS)
            .filter(|&col| self.heights[col] < ROWS)
            .map(Column)
            .collect()
    }

    pub fn playable_cell(&self, column: Column) -> Result<Cell, MoveError> {
        if column.0 >= COLUMNS {
            return Err(MoveError::ColumnOutOfRange);
        }
        let height = self.heights[column.0];
        if height >= ROWS {
            return Err(MoveError::ColumnFull);
        }
        Ok(Cell::new(column.0, height + 1))
    }

    pub fn place(&mut self, column: Column, color: Color) -> Result<Cell, MoveError> {
        let cell = self.playable_cell(column)?;
        self.cells[cell.col][cell.row - 1] = Some(color);
        self.heights[cell.col] += 1;
        Ok(cell)
    }

    pub fn is_full(&self) -> bool {
        self.heights.iter().all(|&height| height == ROWS)
    }

    pub fn canonical_key(&self) -> [u8; COLUMNS * ROWS] {
        let mut normal = [0u8; COLUMNS * ROWS];
        let mut mirror = [0u8; COLUMNS * ROWS];

        for col in 0..COLUMNS {
            for row in 0..ROWS {
                let idx = col * ROWS + row;
                let mirrored_col = COLUMNS - 1 - col;
                let mirrored_idx = col * ROWS + row;
                normal[idx] = encode_cell(self.cells[col][row]);
                mirror[mirrored_idx] = encode_cell(self.cells[mirrored_col][row]);
            }
        }

        if normal <= mirror { normal } else { mirror }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Position {
    pub board: Board,
    pub side_to_move: Color,
    pub winner: Option<Color>,
    pub move_count: usize,
}

impl Default for Position {
    fn default() -> Self {
        Self {
            board: Board::default(),
            side_to_move: Color::White,
            winner: None,
            move_count: 0,
        }
    }
}

impl Position {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn legal_moves(&self) -> Vec<Column> {
        if self.winner.is_some() {
            Vec::new()
        } else {
            self.board.legal_moves()
        }
    }

    pub fn apply_move(&mut self, column: Column) -> Result<Cell, MoveError> {
        if self.winner.is_some() {
            return Err(MoveError::GameAlreadyOver);
        }

        let color = self.side_to_move;
        let cell = self.board.place(column, color)?;
        self.move_count += 1;

        if has_connect_four(&self.board, color) {
            self.winner = Some(color);
        } else {
            self.side_to_move = color.opponent();
        }

        Ok(cell)
    }

    pub fn is_draw(&self) -> bool {
        self.winner.is_none() && self.board.is_full()
    }
}

fn encode_cell(cell: Option<Color>) -> u8 {
    match cell {
        None => 0,
        Some(Color::White) => 1,
        Some(Color::Black) => 2,
    }
}

fn has_connect_four(board: &Board, color: Color) -> bool {
    for col in 0..COLUMNS {
        for row in 1..=ROWS {
            let start = Cell::new(col, row);
            if board.get(start) != Some(color) {
                continue;
            }

            let directions = [(1isize, 0isize), (0, 1), (1, 1), (1, -1)];
            for (dc, dr) in directions {
                let mut matched = true;
                for step in 1..WIN_LEN {
                    let next_col = col as isize + dc * step as isize;
                    let next_row = row as isize + dr * step as isize;
                    if next_col < 0
                        || next_col >= COLUMNS as isize
                        || next_row <= 0
                        || next_row > ROWS as isize
                        || board.get(Cell::new(next_col as usize, next_row as usize)) != Some(color)
                    {
                        matched = false;
                        break;
                    }
                }
                if matched {
                    return true;
                }
            }
        }
    }
    false
}
