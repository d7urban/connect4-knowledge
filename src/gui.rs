use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::time::{Duration, Instant};

use eframe::egui::{self, Align2, Color32, FontId, Pos2, Rect, Sense, Stroke, StrokeKind, Vec2};

use crate::board::{COLUMNS, Cell, Color, Column, Position, ROWS};
use crate::book::BookEntry;
use crate::bookdb::{RedbBookStore, default_book_db_path};
use crate::policy::{DecisionBasis, choose_move};

const CELL_SIZE: f32 = 72.0;
const DROP_DURATION: Duration = Duration::from_millis(260);
const BOARD_BLUE: Color32 = Color32::from_rgb(34, 89, 168);
const HOLE: Color32 = Color32::from_rgb(237, 236, 227);
const WHITE_PIECE: Color32 = Color32::from_rgb(243, 196, 32);
const BLACK_PIECE: Color32 = Color32::from_rgb(209, 80, 52);

pub fn run_gui() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Connect 4 Knowledge")
            .with_inner_size([760.0, 720.0]),
        ..Default::default()
    };

    eframe::run_native(
        "Connect 4 Knowledge",
        options,
        Box::new(|_cc| Ok(Box::new(Connect4App::default()))),
    )
}

#[derive(Debug, Clone)]
struct FallingPiece {
    column: Column,
    color: Color,
    target_row: usize,
    started_at: Instant,
}

#[derive(Debug, Clone)]
struct EngineTraceEntry {
    ply: usize,
    color: Color,
    column: Column,
    basis: DecisionBasis,
    explanation: String,
}

#[derive(Debug)]
pub struct Connect4App {
    position: Position,
    animation: Option<FallingPiece>,
    engine_result_rx: Option<Receiver<(crate::policy::Decision, Vec<BookEntry>)>>,
    status: String,
    last_basis: Option<DecisionBasis>,
    engine_trace: Vec<EngineTraceEntry>,
    human_starts: bool,
}

impl Default for Connect4App {
    fn default() -> Self {
        Self {
            position: Position::new(),
            animation: None,
            engine_result_rx: None,
            status: String::new(),
            last_basis: None,
            engine_trace: Vec::new(),
            human_starts: true,
        }
    }
}

impl eframe::App for Connect4App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.poll_engine_result();
        self.finish_animation_if_ready();
        if self.animation.is_some() || self.engine_result_rx.is_some() {
            ctx.request_repaint_after(Duration::from_millis(16));
        }

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("Connect 4");
            ui.label(format!(
                "{} is the human player. {} uses the current engine policy.",
                color_name(self.human_color()),
                color_name(self.ai_color())
            ));

            ui.horizontal(|ui| {
                if ui.button("New Game").clicked() {
                    self.reset();
                }

                if let Some(basis) = &self.last_basis {
                    ui.label(format!("engine basis: {}", basis.label()));
                }
            });

            ui.horizontal(|ui| {
                ui.label("First move:");
                let human_selected = ui.selectable_label(self.human_starts, "Human");
                let ai_selected = ui.selectable_label(!self.human_starts, "AI");

                if human_selected.clicked() && !self.human_starts {
                    self.human_starts = true;
                    self.reset();
                }
                if ai_selected.clicked() && self.human_starts {
                    self.human_starts = false;
                    self.reset();
                }
            });

            if !self.status.is_empty() {
                ui.label(&self.status);
            }

            if let Some(entry) = self.engine_trace.last() {
                ui.label(format!(
                    "last AI move: {} {} via {}",
                    color_name(entry.color),
                    column_name(entry.column),
                    entry.basis.label()
                ));
                ui.label(format!("last AI reason: {}", entry.explanation));
            }

            if !self.engine_trace.is_empty() {
                ui.collapsing("AI move trace", |ui| {
                    for entry in self.engine_trace.iter().rev().take(12) {
                        ui.label(format!(
                            "ply {}: {} {} via {}",
                            entry.ply,
                            color_name(entry.color),
                            column_name(entry.column),
                            entry.basis.label()
                        ));
                    }
                });
            }

            ui.add_space(12.0);
            self.draw_board(ui, ctx);
        });
    }
}


impl Connect4App {
    fn reset(&mut self) {
        self.position = Position::new();
        self.animation = None;
        self.engine_result_rx = None;
        self.status.clear();
        self.last_basis = None;
        self.engine_trace.clear();
        println!(
            "new game: human={} ai={} first={}",
            color_name(self.human_color()),
            color_name(self.ai_color()),
            if self.human_starts { "human" } else { "ai" }
        );

        if self.position.side_to_move == self.ai_color() {
            self.spawn_engine_turn();
        }
    }

    fn draw_board(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let board_size = Vec2::new(COLUMNS as f32 * CELL_SIZE, (ROWS as f32 + 0.8) * CELL_SIZE);
        let (response, painter) = ui.allocate_painter(board_size, Sense::click());
        let rect = response.rect;
        let board_rect = Rect::from_min_size(
            Pos2::new(rect.left(), rect.top() + 0.8 * CELL_SIZE),
            Vec2::new(COLUMNS as f32 * CELL_SIZE, ROWS as f32 * CELL_SIZE),
        );

        painter.rect(
            board_rect.expand(8.0),
            16.0,
            BOARD_BLUE,
            Stroke::new(0.0, Color32::TRANSPARENT),
            StrokeKind::Outside,
        );

        let hovered_column = response
            .hover_pos()
            .filter(|pos| board_rect.expand2(Vec2::new(0.0, CELL_SIZE)).contains(*pos))
            .map(|pos| ((pos.x - board_rect.left()) / CELL_SIZE).floor() as usize)
            .filter(|col| *col < COLUMNS);

        if let Some(column) = hovered_column
            && self.animation.is_none()
            && self.engine_result_rx.is_none()
            && self.position.winner.is_none()
            && !self.position.is_draw()
            && self.position.side_to_move == self.human_color()
        {
            let center = cell_center(board_rect, Cell::new(column, ROWS + 1), true);
            painter.circle_filled(center, CELL_SIZE * 0.36, preview_color(self.position.side_to_move));
        }

        for col in 0..COLUMNS {
            for row in 1..=ROWS {
                let center = cell_center(board_rect, Cell::new(col, row), false);
                painter.circle_filled(center, CELL_SIZE * 0.38, HOLE);
                if let Some(color) = self.position.board.get(Cell::new(col, row)) {
                    painter.circle_filled(center, CELL_SIZE * 0.34, piece_color(color));
                }
            }
        }

        if let Some(animation) = &self.animation {
            let center = animated_center(board_rect, animation);
            painter.circle_filled(center, CELL_SIZE * 0.34, piece_color(animation.color));
        }

        if response.clicked()
            && let Some(pointer_pos) = response.interact_pointer_pos()
            && let Some(column) = column_from_pointer(board_rect, pointer_pos)
        {
            self.try_human_move(Column(column), ctx);
        }

        if let Some(winner) = self.position.winner {
            let text = format!("{} wins", color_name(winner));
            painter.text(
                rect.center_top() + Vec2::new(0.0, 4.0),
                Align2::CENTER_TOP,
                text,
                FontId::proportional(28.0),
                Color32::BLACK,
            );
        } else if self.position.is_draw() {
            painter.text(
                rect.center_top() + Vec2::new(0.0, 4.0),
                Align2::CENTER_TOP,
                "Draw",
                FontId::proportional(28.0),
                Color32::BLACK,
            );
        }
    }

    fn try_human_move(&mut self, column: Column, ctx: &egui::Context) {
        if self.animation.is_some()
            || self.engine_result_rx.is_some()
            || self.position.side_to_move != self.human_color()
        {
            return;
        }

        let Ok(target) = self.position.board.playable_cell(column) else {
            self.status = "column is full".to_string();
            return;
        };

        self.status.clear();
        self.animation = Some(FallingPiece {
            column,
            color: self.position.side_to_move,
            target_row: target.row,
            started_at: Instant::now(),
        });
        ctx.request_repaint();
    }

    fn finish_animation_if_ready(&mut self) {
        let Some(animation) = &self.animation else {
            return;
        };

        if animation.started_at.elapsed() < DROP_DURATION {
            return;
        }

        let animation = self.animation.take().expect("animation checked above");
        if let Err(err) = self.position.apply_move(animation.column) {
            self.status = format!("move failed: {err}");
            return;
        }

        let ply = current_ply(&self.position);
        let mover = animation.color;
        if mover == self.human_color() {
            println!(
                "ply {ply}: human {} {}",
                color_name(mover),
                column_name(animation.column)
            );
        }

        if let Some(winner) = self.position.winner {
            println!("game over: {} wins", color_name(winner));
            return;
        }

        if self.position.is_draw() {
            println!("game over: draw");
            return;
        }

        if self.position.side_to_move == self.ai_color() {
            self.spawn_engine_turn();
        }
    }

    fn human_color(&self) -> Color {
        if self.human_starts {
            Color::White
        } else {
            Color::Black
        }
    }

    fn ai_color(&self) -> Color {
        self.human_color().opponent()
    }

    fn spawn_engine_turn(&mut self) {
        let position = self.position.clone();
        let (tx, rx) = mpsc::channel();
        self.engine_result_rx = Some(rx);
        self.status = "AI is thinking...".to_string();

        let spawn_result = std::thread::Builder::new()
            .name("connect4-engine".to_string())
            .stack_size(16 * 1024 * 1024)
            .spawn(move || {
                let store = RedbBookStore::open_or_create(default_book_db_path());
                let result = match store {
                    Ok(store) => {
                        let book_entry = store.get(&position).ok().flatten();
                        let decision = choose_move(&position, book_entry);
                        let entries = decision.certified_entries.clone();
                        (decision, entries)
                    }
                    Err(err) => (
                        crate::policy::Decision {
                            selected_move: position.legal_moves().first().copied(),
                            basis: DecisionBasis::Fallback,
                            proof: None,
                            explanation: format!("failed to open book db: {err}"),
                            certified_entries: Vec::new(),
                        },
                        Vec::new(),
                    ),
                };
                let _ = tx.send(result);
            });

        if let Err(err) = spawn_result {
            self.engine_result_rx = None;
            self.status = format!("failed to start AI worker: {err}");
        }
    }

    fn poll_engine_result(&mut self) {
        let Some(rx) = self.engine_result_rx.take() else {
            return;
        };

        match rx.try_recv() {
            Ok((decision, entries)) => {
                if let Ok(store) = RedbBookStore::open_or_create(default_book_db_path()) {
                    for entry in &entries {
                        if let Err(err) = store.insert(entry) {
                            self.status = format!("AI move chosen, but saving book db failed: {err}");
                            self.last_basis = Some(decision.basis.clone());
                            if let Some(column) = decision.selected_move
                                && let Ok(target) = self.position.board.playable_cell(column)
                            {
                                self.animation = Some(FallingPiece {
                                    column,
                                    color: self.position.side_to_move,
                                    target_row: target.row,
                                    started_at: Instant::now(),
                                });
                            }
                            return;
                        }
                    }
                }
                self.status = decision.explanation.clone();
                self.last_basis = Some(decision.basis.clone());
                if let Some(column) = decision.selected_move {
                    let ai_color = self.position.side_to_move;
                    let ply = current_ply(&self.position) + 1;
                    println!(
                        "ply {ply}: ai {} {} via {} - {}",
                        color_name(ai_color),
                        column_name(column),
                        decision.basis.label(),
                        decision.explanation
                    );
                    self.engine_trace.push(EngineTraceEntry {
                        ply,
                        color: ai_color,
                        column,
                        basis: decision.basis.clone(),
                        explanation: decision.explanation.clone(),
                    });
                }

                if let Some(column) = decision.selected_move
                    && let Ok(target) = self.position.board.playable_cell(column)
                {
                    self.animation = Some(FallingPiece {
                        column,
                        color: self.position.side_to_move,
                        target_row: target.row,
                        started_at: Instant::now(),
                    });
                }
            }
            Err(TryRecvError::Empty) => {
                self.engine_result_rx = Some(rx);
            }
            Err(TryRecvError::Disconnected) => {
                self.status = "Engine failed to return a move".to_string();
            }
        }
    }
}

fn animated_center(board_rect: Rect, animation: &FallingPiece) -> Pos2 {
    let start = Pos2::new(
        board_rect.left() + (animation.column.0 as f32 + 0.5) * CELL_SIZE,
        board_rect.top() - CELL_SIZE * 0.45,
    );
    let end = cell_center(board_rect, Cell::new(animation.column.0, animation.target_row), false);
    let t = (animation.started_at.elapsed().as_secs_f32() / DROP_DURATION.as_secs_f32()).clamp(0.0, 1.0);
    let eased = ease_out_cubic(t);
    Pos2::new(start.x, start.y + (end.y - start.y) * eased)
}

fn ease_out_cubic(t: f32) -> f32 {
    1.0 - (1.0 - t).powi(3)
}

fn cell_center(board_rect: Rect, cell: Cell, preview_row: bool) -> Pos2 {
    let row_offset = if preview_row {
        0.0
    } else {
        (ROWS - cell.row) as f32 + 0.5
    };

    Pos2::new(
        board_rect.left() + (cell.col as f32 + 0.5) * CELL_SIZE,
        board_rect.top() + row_offset * CELL_SIZE,
    )
}

fn column_from_pointer(board_rect: Rect, pointer_pos: Pos2) -> Option<usize> {
    if !board_rect.expand2(Vec2::new(0.0, CELL_SIZE)).contains(pointer_pos) {
        return None;
    }
    let column = ((pointer_pos.x - board_rect.left()) / CELL_SIZE).floor() as usize;
    (column < COLUMNS).then_some(column)
}

fn piece_color(color: Color) -> Color32 {
    match color {
        Color::White => WHITE_PIECE,
        Color::Black => BLACK_PIECE,
    }
}

fn preview_color(color: Color) -> Color32 {
    let base = piece_color(color);
    Color32::from_rgba_unmultiplied(base.r(), base.g(), base.b(), 170)
}

fn color_name(color: Color) -> &'static str {
    match color {
        Color::White => "Yellow",
        Color::Black => "Red",
    }
}

fn column_name(column: Column) -> char {
    (b'a' + column.0 as u8) as char
}

fn current_ply(position: &Position) -> usize {
    position.board.canonical_key().iter().filter(|&&cell| cell != 0).count()
}
