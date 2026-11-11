//! Help › Diagnostics: a report of the system, the memory, the open file,
//! frame and operation times and the log, in English, without personal
//! paths, to copy or save and send when something goes wrong.

use std::fmt::Write as _;
use std::sync::Arc;
use std::time::{Duration, Instant};

use eframe::egui::{self, Key};

use super::*;
use crate::diag;
use crate::i18n::t;
use crate::theme::{self, palette};

/// The report is written again at most this often while it is shown.
const REFRESH: Duration = Duration::from_secs(1);

/// What the Diagnostics window keeps between frames.
#[derive(Default)]
pub(super) struct Diagnostics {
    pub(super) open: bool,
    /// The report, and when it was written.
    report: Option<(Instant, Arc<str>)>,
    /// The end of the log file, read when the window opened (earlier
    /// sessions don't change; this one's log is kept in memory).
    log_tail: Option<Vec<String>>,
    /// The graphics card, read once from the renderer.
    gpu: Option<String>,
    /// (generation, estimated bytes of the table and of the undo steps).
    sizes: Option<(u64, u64, u64)>,
    /// "Copied" or "Saved" shows until then.
    notice: Option<(Instant, String)>,
    pub(super) frames: FrameTimer,
}

/// Times whole frames: eframe's measure of each (the interface built,
/// laid out, painted, all its passes), less the time spent in the file
/// dialogs, which wait for the user.
#[derive(Default)]
pub(super) struct FrameTimer {
    /// The frame being built (egui's count) and when it began.
    frame: Option<(u64, Instant)>,
    /// The time spent in this frame's passes, for when eframe doesn't say.
    built: Duration,
    /// The time spent in file dialogs during this frame.
    waiting: Duration,
}

impl FrameTimer {
    /// At the start of each pass: the frame before, if this is a new one,
    /// is recorded.
    pub(super) fn start_pass(&mut self, frame_nr: u64, cpu_usage: Option<f32>, now: Instant) {
        if let Some((nr, began)) = self.frame {
            if nr == frame_nr {
                return;
            }
            // eframe gives the last frame's time once it is painted.
            let took = cpu_usage
                .and_then(|s| Duration::try_from_secs_f32(s).ok())
                .unwrap_or(self.built);
            diag::end_frame(began, took.saturating_sub(self.waiting));
        }
        self.frame = Some((frame_nr, now));
        self.built = Duration::ZERO;
        self.waiting = Duration::ZERO;
    }

    /// At the end of each pass begun at `started`.
    pub(super) fn end_pass(&mut self, started: Instant) {
        self.built += started.elapsed();
    }

    /// Time spent waiting for the user in a file dialog.
    pub(super) fn waited(&mut self, took: Duration) {
        self.waiting += took;
    }
}

impl Diagnostics {
    /// Shows `text` beside the buttons for a moment.
    pub(super) fn notice(&mut self, text: String) {
        self.notice = Some((Instant::now() + Duration::from_secs(2), text));
    }
}

impl CsvApp {
    /// The graphics card, read once.
    pub(super) fn read_gpu(&mut self, frame: &eframe::Frame) {
        if self.diagnostics.gpu.is_some() {
            return;
        }
        let Some(state) = frame.wgpu_render_state() else {
            self.diagnostics.gpu = Some("(no wgpu renderer)".to_owned());
            return;
        };
        let info = state.adapter.get_info();
        let driver = [info.driver.as_str(), info.driver_info.as_str()]
            .into_iter()
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join(" ");
        self.diagnostics.gpu = Some(format!(
            "{} ({:?}, {:?}, driver {driver})",
            info.name, info.backend, info.device_type
        ));
    }

    pub(super) fn diagnostics_window(&mut self, ctx: &egui::Context) {
        if !self.diagnostics.open {
            self.diagnostics.log_tail = None;
            return;
        }
        let stale = self
            .diagnostics
            .report
            .as_ref()
            .is_none_or(|(at, _)| at.elapsed() >= REFRESH);
        if stale {
            let report = self.report(ctx);
            // It holds the names of the file, sheets and log lines.
            theme::cover(ctx, [report.as_str()]);
            self.diagnostics.report = Some((Instant::now(), report.into()));
        }
        ctx.request_repaint_after(REFRESH);
        let report = self
            .diagnostics
            .report
            .as_ref()
            .map_or_else(|| Arc::from(""), |(_, r)| Arc::clone(r));
        // Its margins in: what is left of the window for the content.
        let room =
            theme::dialog_room(ctx, egui::vec2(808.0, f32::INFINITY)) - egui::vec2(48.0, 48.0);
        theme::modal("diagnostics", ctx).show(ctx, |ui| {
            let p = palette(ui);
            ui.set_width(room.x);
            ui.heading(t!("diagnostics-title"));
            ui.add_space(4.0);
            ui.label(
                egui::RichText::new(t!("diagnostics-text"))
                    .size(13.0)
                    .color(p.muted),
            );
            ui.add_space(10.0);
            // The report scrolls in what the text and the buttons leave.
            let report_height =
                (room.y - ui.min_rect().height() - 36.0 - 14.0 - 36.0).clamp(60.0, 420.0);
            egui::Frame::new()
                .fill(p.bg)
                .stroke(egui::Stroke::new(1.0, p.line))
                .corner_radius(10)
                .inner_margin(12)
                .show(ui, |ui| {
                    egui::ScrollArea::vertical()
                        .max_height(report_height)
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            ui.add(
                                egui::Label::new(
                                    egui::RichText::new(&*report).font(theme::mono(11.0)),
                                )
                                .selectable(true),
                            );
                        });
                });
            ui.add_space(14.0);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if theme::primary_button(ui, &t!("diagnostics-copy"), 0.0).clicked() {
                    ui.ctx().copy_text(report.to_string());
                    self.diagnostics.notice(t!("diagnostics-copied"));
                    diag::log(diag::Level::Info, "diagnostics copied");
                }
                if theme::secondary_button(ui, &t!("diagnostics-save"), 0.0).clicked() {
                    self.file_request = Some(FileRequest::SaveReport(report.to_string()));
                }
                if theme::secondary_button(ui, &t!("diagnostics-close"), 0.0).clicked()
                    || theme::dialog_key(ui, Key::Escape)
                {
                    self.diagnostics.open = false;
                }
                if let Some((until, text)) = &self.diagnostics.notice
                    && Instant::now() < *until
                {
                    ui.label(egui::RichText::new(text).size(12.0).color(p.accent_ink));
                    ui.ctx().request_repaint_after(*until - Instant::now());
                }
            });
        });
    }

    /// Estimated bytes of the table and of the undo and redo steps (each
    /// text's own bytes and the room the lists keep).
    fn sizes(&mut self) -> (u64, u64) {
        if let Some((generation, table, undo)) = self.diagnostics.sizes
            && generation == self.generation
        {
            return (table, undo);
        }
        let table: u64 = self.table.rows().iter().map(table::row_bytes).sum();
        let undo = self
            .undo
            .iter()
            .chain(&self.redo)
            .map(|(_, bytes)| bytes)
            .sum();
        self.diagnostics.sizes = Some((self.generation, table, undo));
        (table, undo)
    }

    /// The report, in English (to be read by whoever helps).
    pub(super) fn report(&mut self, ctx: &egui::Context) -> String {
        let mut r = String::new();
        let up = diag::uptime().as_secs();
        let _ = writeln!(
            r,
            "CSV Edit {} diagnostics, {} (up {}h {:02}m {:02}s)",
            env!("CARGO_PKG_VERSION"),
            diag::humantime(std::time::SystemTime::now()),
            up / 3600,
            up / 60 % 60,
            up % 60
        );

        let _ = writeln!(r, "\nSystem");
        let _ = writeln!(
            r,
            "  OS: {} {}",
            std::env::consts::OS,
            std::env::consts::ARCH
        );
        if let Some(gpu) = &self.diagnostics.gpu {
            let _ = writeln!(r, "  GPU: {gpu}");
        }
        let screen = ctx.content_rect();
        let _ = writeln!(
            r,
            "  Window: {:.0} x {:.0} points, scale {:.2}",
            screen.width(),
            screen.height(),
            ctx.pixels_per_point()
        );
        let _ = writeln!(
            r,
            "  Language: {}, theme: {:?} ({}), row height: {:?}, column names: {:?}",
            crate::i18n::lang().code(),
            ctx.options(|o| o.theme_preference),
            if ctx.global_style().visuals.dark_mode {
                "dark"
            } else {
                "light"
            },
            self.density,
            columns::labels()
        );

        let _ = writeln!(r, "\nMemory");
        match diag::process_memory() {
            Some(m) => {
                let private = m
                    .private
                    .map_or_else(String::new, |p| format!(", private {}", diag::bytes(p)));
                let _ = writeln!(r, "  Process: {} in RAM{private}", diag::bytes(m.resident));
            }
            None => {
                let _ = writeln!(r, "  Process: (unknown on this system)");
            }
        }
        let (table, undo) = self.sizes();
        let cells = self.table.row_count() * self.table.col_count();
        let _ = writeln!(r, "  Table: about {} for {cells} cells", diag::bytes(table));
        let _ = writeln!(
            r,
            "  Undo: {} steps, redo: {} steps, about {}",
            self.undo.len(),
            self.redo.len(),
            diag::bytes(undo)
        );
        let fonts = theme::loaded_fonts();
        let built_in: u64 = fonts.iter().filter(|f| !f.2).map(|f| f.1).sum();
        let _ = writeln!(r, "  Fonts built in: {}", diag::bytes(built_in));
        let system: Vec<String> = fonts
            .iter()
            .filter(|f| f.2)
            .map(|f| format!("{} {}", f.0, diag::bytes(f.1)))
            .collect();
        let _ = writeln!(
            r,
            "  System fonts loaded: {}",
            if system.is_empty() {
                "none (no text has needed them)".to_owned()
            } else {
                system.join(", ")
            }
        );

        let _ = writeln!(r, "\nDocument");
        // Its kind, not its name: the report is sent to others.
        let name = self
            .path
            .as_deref()
            .or(self.origin.as_deref())
            .map_or_else(|| "untitled".to_owned(), |p| format!("*.{}", file_kind(p)));
        let source = match (&self.path, &self.origin) {
            (_, Some(_)) if self.path.is_none() => "workbook (saved as CSV)",
            (Some(_), _) => "CSV file",
            _ => "new",
        };
        let size = self
            .path
            .as_deref()
            .or(self.origin.as_deref())
            .and_then(|p| std::fs::metadata(p).ok())
            .map_or_else(String::new, |m| {
                format!(", {} on disk", diag::bytes(m.len()))
            });
        let _ = writeln!(r, "  File: {name} ({source}{size})");
        let _ = writeln!(
            r,
            "  Rows x columns: {} x {}, header row: {}",
            self.table.row_count(),
            self.table.col_count(),
            if self.body_start() == 1 { "yes" } else { "no" }
        );
        let _ = writeln!(
            r,
            "  Columns shown: {}",
            self.shown.as_ref().map_or_else(
                || "all".to_owned(),
                |s| format!("{} of {}", s.len(), self.table.col_count())
            )
        );
        let f = &self.format;
        let _ = writeln!(
            r,
            "  Format: delimiter {:?}{}, quote {:?}, quote all: {}, trailing delimiter: {}, {:?}, {:?}, {} lines kept from the top, protect formulas: {}",
            f.delimiter,
            if f.other_delimiters.is_empty() {
                String::new()
            } else {
                format!(" (also {:?})", f.other_delimiters)
            },
            f.quote,
            f.quote_all,
            f.trailing_delimiter,
            f.encoding,
            f.line_ending,
            f.preamble.lines().count(),
            self.protect_formulas
        );
        let _ = writeln!(r, "  Left out at reading: {:?}", self.losses);
        let _ = writeln!(r, "  Workbook formulas: {}", self.formulas.len());
        let needle_len = self.needle().chars().count();
        let found = if needle_len > 0 {
            format!("{needle_len} characters, {} matches", self.match_count())
        } else {
            "off".to_owned()
        };
        let _ = writeln!(r, "  Find: {found}");
        let _ = writeln!(
            r,
            "  Unsaved changes: {}, editing: {}",
            if self.dirty() { "yes" } else { "no" },
            if self.editing.is_some() { "yes" } else { "no" }
        );

        let _ = writeln!(r, "\nFrames");
        for (label, seconds) in [("Last 10 s", 10), ("Last 5 min", 300)] {
            let s = diag::frames(seconds);
            let _ = writeln!(
                r,
                "  {label}: {} frames, average {}, longest {}",
                s.frames,
                diag::ms(s.average),
                diag::ms(s.max)
            );
        }
        let freezes = diag::freezes();
        let _ = writeln!(
            r,
            "  Freezes (over {}): {}",
            diag::ms(diag::FREEZE),
            freezes.len()
        );
        for freeze in freezes.iter().rev().take(10) {
            let ran: Vec<String> = freeze
                .ran
                .iter()
                .map(|(n, d)| format!("{n} {}", diag::ms(*d)))
                .collect();
            let _ = writeln!(
                r,
                "    at {}: {}{}",
                clock(freeze.at),
                diag::ms(freeze.took),
                if ran.is_empty() {
                    String::new()
                } else {
                    format!(" ({})", ran.join(", "))
                }
            );
        }

        let _ = writeln!(r, "\nOperations (longest first)");
        for (name, s) in diag::operations() {
            let _ = writeln!(
                r,
                "  {name:<16} x{:<5} average {:>9}  longest {:>9}  last {:>9}",
                s.count,
                diag::ms(s.total / s.count.max(1)),
                diag::ms(s.max),
                diag::ms(s.last)
            );
        }

        let _ = writeln!(r, "\nLog (this session)");
        let entries = diag::entries();
        if entries.is_empty() {
            let _ = writeln!(r, "  (nothing)");
        }
        for e in entries.iter().rev().take(60).rev() {
            let _ = writeln!(r, "  [{}] {:?}: {}", clock(e.at), e.level, e.message);
        }
        let _ = writeln!(r, "\nLog file (latest lines, earlier sessions included)");
        let tail = self
            .diagnostics
            .log_tail
            .get_or_insert_with(|| diag::log_file_tail(30));
        for line in tail {
            let _ = writeln!(r, "  {line}");
        }
        r
    }
}

/// 01:02:03 since the start.
fn clock(d: Duration) -> String {
    let s = d.as_secs();
    format!("{:02}:{:02}:{:02}", s / 3600, s / 60 % 60, s % 60)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frames_leave_out_the_time_in_file_dialogs() {
        let mut timer = FrameTimer::default();
        let began = Instant::now();
        timer.start_pass(1_000_001, None, began);
        // The user browses for five seconds in a dialog.
        timer.waited(Duration::from_secs(5));
        // Another pass of the same frame records nothing.
        timer.start_pass(1_000_001, None, began);
        // eframe measured the whole frame, dialog included.
        timer.start_pass(1_000_002, Some(5.01), Instant::now());
        assert!(
            diag::freezes()
                .iter()
                .all(|f| f.took < Duration::from_secs(4))
        );
        assert_eq!(timer.waiting, Duration::ZERO);
        assert_eq!(timer.frame.map(|(nr, _)| nr), Some(1_000_002));
    }
}
