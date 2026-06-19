//! The interactive, full-screen text editor.
//!
//! The model is deliberately simple: the document is a `Vec<Vec<char>>` (one
//! vector of chars per line), so the cursor is a plain (column, row) pair of
//! char indices and UTF-8 "just works" for editing. We redraw the entire frame
//! into one `String` and write it in a single flush per keypress.

use crate::terminal::{self, Key, RawMode};
use std::fs;
use std::io::{self, Write};
use std::path::PathBuf;

/// A copper accent (truecolor) to echo the Patina family. Used sparingly.
const COPPER: &str = "\x1b[38;2;199;117;46m";
const RESET: &str = "\x1b[0m";
/// Inverse video for the status bar.
const INVERSE: &str = "\x1b[7m";

/// How many spaces a Tab inserts. (Documented in the README.)
const TAB_WIDTH: usize = 4;

pub struct Editor {
    rows: Vec<Vec<char>>,
    cx: usize, // cursor column (char index into the current row)
    cy: usize, // cursor row (index into rows)
    row_offset: usize,
    col_offset: usize,
    screen_rows: usize, // text area height (excludes the status bar)
    screen_cols: usize,
    filename: PathBuf,
    dirty: bool,
    status: String,
    /// True once a quit has been "armed" by a first Ctrl-Q on a dirty buffer.
    quit_armed: bool,
}

impl Editor {
    /// Build an editor for `path`, loading the file if it exists.
    pub fn open(path: PathBuf) -> io::Result<Editor> {
        let rows = match fs::read_to_string(&path) {
            Ok(text) => {
                let mut rows: Vec<Vec<char>> = text.lines().map(|l| l.chars().collect()).collect();
                if rows.is_empty() {
                    rows.push(Vec::new());
                }
                rows
            }
            // A missing file just means a fresh, empty buffer.
            Err(ref e) if e.kind() == io::ErrorKind::NotFound => vec![Vec::new()],
            Err(e) => return Err(e),
        };

        let (rows_n, cols_n) = terminal::size();
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| path.to_string_lossy().to_string());

        Ok(Editor {
            rows,
            cx: 0,
            cy: 0,
            row_offset: 0,
            col_offset: 0,
            screen_rows: rows_n.saturating_sub(1).max(1),
            screen_cols: cols_n.max(1),
            filename: path,
            dirty: false,
            status: format!("{name}  —  ^S save   ^Q quit"),
            quit_armed: false,
        })
    }

    /// Run the edit loop until the user quits.
    pub fn run(&mut self) -> io::Result<()> {
        let _raw = RawMode::enable()?; // restores the terminal on drop
        loop {
            self.scroll();
            self.render()?;
            let key = terminal::read_key()?;
            if !self.handle(key) {
                break;
            }
        }
        Ok(())
    }

    /// Handle one key. Returns `false` when the editor should quit.
    fn handle(&mut self, key: Key) -> bool {
        // Any key other than a second Ctrl-Q disarms the quit confirmation.
        let was_armed = self.quit_armed;
        self.quit_armed = false;

        match key {
            Key::Ctrl(b'q') => {
                if self.dirty && !was_armed {
                    self.quit_armed = true;
                    self.status = "Unsaved changes — Ctrl-Q again to quit".to_string();
                    return true;
                }
                return false;
            }
            Key::Ctrl(b's') => self.save(),

            Key::Up => self.move_up(),
            Key::Down => self.move_down(),
            Key::Left => self.move_left(),
            Key::Right => self.move_right(),
            Key::Home => self.cx = 0,
            Key::End => self.cx = self.current_len(),
            Key::PageUp => {
                for _ in 0..self.screen_rows {
                    self.move_up();
                }
            }
            Key::PageDown => {
                for _ in 0..self.screen_rows {
                    self.move_down();
                }
            }

            Key::Enter => self.insert_newline(),
            Key::Backspace => self.backspace(),
            Key::Delete => self.delete(),
            Key::Tab => {
                for _ in 0..TAB_WIDTH {
                    self.insert_char(' ');
                }
            }
            Key::Char(c) => self.insert_char(c),

            Key::Esc | Key::Ctrl(_) => {}
        }
        true
    }

    // ---- editing -----------------------------------------------------------

    fn current_len(&self) -> usize {
        self.rows[self.cy].len()
    }

    fn insert_char(&mut self, c: char) {
        let row = &mut self.rows[self.cy];
        let at = self.cx.min(row.len());
        row.insert(at, c);
        self.cx = at + 1;
        self.dirty = true;
    }

    fn insert_newline(&mut self) {
        let at = self.cx.min(self.current_len());
        let tail = self.rows[self.cy].split_off(at);
        self.rows.insert(self.cy + 1, tail);
        self.cy += 1;
        self.cx = 0;
        self.dirty = true;
    }

    fn backspace(&mut self) {
        if self.cx > 0 {
            self.rows[self.cy].remove(self.cx - 1);
            self.cx -= 1;
            self.dirty = true;
        } else if self.cy > 0 {
            // Merge this line onto the end of the previous one.
            let line = self.rows.remove(self.cy);
            self.cy -= 1;
            self.cx = self.current_len();
            self.rows[self.cy].extend(line);
            self.dirty = true;
        }
    }

    fn delete(&mut self) {
        if self.cx < self.current_len() {
            self.rows[self.cy].remove(self.cx);
            self.dirty = true;
        } else if self.cy + 1 < self.rows.len() {
            // Pull the next line up onto this one.
            let next = self.rows.remove(self.cy + 1);
            self.rows[self.cy].extend(next);
            self.dirty = true;
        }
    }

    // ---- movement ----------------------------------------------------------

    fn move_up(&mut self) {
        if self.cy > 0 {
            self.cy -= 1;
            self.cx = self.cx.min(self.current_len());
        }
    }

    fn move_down(&mut self) {
        if self.cy + 1 < self.rows.len() {
            self.cy += 1;
            self.cx = self.cx.min(self.current_len());
        }
    }

    fn move_left(&mut self) {
        if self.cx > 0 {
            self.cx -= 1;
        } else if self.cy > 0 {
            self.cy -= 1;
            self.cx = self.current_len();
        }
    }

    fn move_right(&mut self) {
        if self.cx < self.current_len() {
            self.cx += 1;
        } else if self.cy + 1 < self.rows.len() {
            self.cy += 1;
            self.cx = 0;
        }
    }

    // ---- file --------------------------------------------------------------

    fn save(&mut self) {
        let mut text: String = self
            .rows
            .iter()
            .map(|row| row.iter().collect::<String>())
            .collect::<Vec<_>>()
            .join("\n");
        text.push('\n'); // single trailing newline

        match fs::write(&self.filename, text) {
            Ok(()) => {
                self.dirty = false;
                self.status = "Saved".to_string();
            }
            Err(e) => self.status = format!("Save failed: {e}"),
        }
    }

    // ---- viewport ----------------------------------------------------------

    /// Adjust offsets so the cursor stays on screen.
    fn scroll(&mut self) {
        if self.cy < self.row_offset {
            self.row_offset = self.cy;
        }
        if self.cy >= self.row_offset + self.screen_rows {
            self.row_offset = self.cy - self.screen_rows + 1;
        }
        if self.cx < self.col_offset {
            self.col_offset = self.cx;
        }
        if self.cx >= self.col_offset + self.screen_cols {
            self.col_offset = self.cx - self.screen_cols + 1;
        }
    }

    // ---- rendering ---------------------------------------------------------

    /// Draw the whole frame in a single write + flush.
    fn render(&self) -> io::Result<()> {
        let mut buf = String::new();
        buf.push_str("\x1b[?25l"); // hide cursor while drawing
        buf.push_str("\x1b[H"); // home

        // Text area.
        for screen_row in 0..self.screen_rows {
            let file_row = self.row_offset + screen_row;
            if file_row < self.rows.len() {
                let row = &self.rows[file_row];
                if self.col_offset < row.len() {
                    let visible: String = row[self.col_offset..]
                        .iter()
                        .take(self.screen_cols)
                        .collect();
                    buf.push_str(&visible);
                }
            } else {
                buf.push_str(&format!("{COPPER}~{RESET}"));
            }
            buf.push_str("\x1b[K\r\n"); // clear to EOL, next line
        }

        // Status bar (inverse video), padded to full width.
        let name = self
            .filename
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();
        let left = format!(" {}{} ", name, if self.dirty { "*" } else { "" });
        let right = format!(
            "{}  ln {}:{}   ^S save   ^Q quit ",
            self.status,
            self.cy + 1,
            self.cx + 1
        );

        let mut bar = left.clone();
        let used = left.chars().count() + right.chars().count();
        if used < self.screen_cols {
            bar.push_str(&" ".repeat(self.screen_cols - used));
        }
        bar.push_str(&right);
        // Clamp to width so we never wrap the bar.
        let bar: String = bar.chars().take(self.screen_cols).collect();
        buf.push_str(&format!("{INVERSE}{bar}{RESET}"));

        // Place the real cursor (1-based), then show it.
        let cy = self.cy - self.row_offset + 1;
        let cx = self.cx - self.col_offset + 1;
        buf.push_str(&format!("\x1b[{cy};{cx}H"));
        buf.push_str("\x1b[?25h");

        let mut out = io::stdout();
        out.write_all(buf.as_bytes())?;
        out.flush()
    }
}
