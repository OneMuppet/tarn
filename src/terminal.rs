//! Raw-mode terminal handling — std only, no crates.
//!
//! We don't link termios/crossterm. Instead we shell out to `stty`, which is
//! present on every POSIX system. `RawMode` is a guard: it flips the terminal
//! into raw mode + the alternate screen on creation, and *always* restores it on
//! drop — including on panic, which is why the binary must NOT use
//! `panic = "abort"`.

use std::io::{self, Read, Write};
use std::process::Command;

/// A decoded keypress.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key {
    Char(char),
    Ctrl(u8), // the letter byte, e.g. b's' for Ctrl-S
    Enter,
    Tab,
    Backspace,
    Delete,
    Esc,
    Up,
    Down,
    Left,
    Right,
    Home,
    End,
    PageUp,
    PageDown,
}

/// Guard that holds the terminal in raw mode for its lifetime.
pub struct RawMode {
    /// The original `stty -g` settings, restored on drop.
    original: String,
}

impl RawMode {
    /// Enter raw mode and the alternate screen.
    pub fn enable() -> io::Result<RawMode> {
        // Capture current settings so we can restore them exactly.
        let original = stty(&["-g"])?;

        // -echo:    don't echo typed chars
        // -icanon:  read byte-at-a-time, not line-at-a-time
        // -isig:    let us see Ctrl-C/Ctrl-Z as bytes instead of signals
        // min 0 / time 1: reads return after ~100ms even with no input, so we can
        //                 tell a lone ESC from an escape sequence and idle cheaply.
        run_stty(&["-echo", "-icanon", "-isig", "min", "0", "time", "1"])?;

        // Switch to the alternate screen buffer so we don't clobber scrollback.
        let mut out = io::stdout();
        out.write_all(b"\x1b[?1049h")?;
        out.flush()?;

        Ok(RawMode {
            original: original.trim().to_string(),
        })
    }
}

impl Drop for RawMode {
    fn drop(&mut self) {
        // Leave the alternate screen and make sure the cursor is visible again.
        let mut out = io::stdout();
        let _ = out.write_all(b"\x1b[?1049l\x1b[?25h");
        let _ = out.flush();
        // Restore the original terminal settings verbatim.
        if !self.original.is_empty() {
            let _ = run_stty(&[&self.original]);
        }
    }
}

/// Terminal size as (rows, cols). Falls back to (24, 80) if `stty` can't tell us.
pub fn size() -> (usize, usize) {
    if let Ok(out) = stty(&["size"]) {
        let mut it = out.split_whitespace();
        if let (Some(r), Some(c)) = (it.next(), it.next()) {
            if let (Ok(r), Ok(c)) = (r.parse(), c.parse()) {
                return (r, c);
            }
        }
    }
    (24, 80)
}

/// Read one decoded key, blocking (idling on the stty timeout) until something
/// arrives.
pub fn read_key() -> io::Result<Key> {
    loop {
        if let Some(b) = read_byte()? {
            return decode(b);
        }
        // 0-byte read = timeout with no input; keep waiting.
    }
}

/// Read a single byte. `Ok(None)` means the stty timeout elapsed with no input
/// (in raw mode a 0-byte read is a timeout, not EOF).
fn read_byte() -> io::Result<Option<u8>> {
    let mut buf = [0u8; 1];
    match io::stdin().read(&mut buf)? {
        0 => Ok(None),
        _ => Ok(Some(buf[0])),
    }
}

/// Turn a leading byte (plus any follow-on bytes) into a `Key`.
fn decode(b: u8) -> io::Result<Key> {
    match b {
        0x1b => decode_escape(),
        b'\r' | b'\n' => Ok(Key::Enter),
        0x7f => Ok(Key::Backspace),
        b'\t' => Ok(Key::Tab),
        // Other control bytes: map back to the letter, e.g. 0x13 -> Ctrl('s').
        c if c < 0x20 => Ok(Key::Ctrl(c | 0x60)),
        // ASCII printable.
        c if c < 0x80 => Ok(Key::Char(c as char)),
        // Start of a UTF-8 multibyte sequence.
        c => decode_utf8(c),
    }
}

/// We just read ESC (0x1b). Decide whether it's a lone Escape or a CSI/SS3
/// sequence by peeking at the next byte within the timeout.
fn decode_escape() -> io::Result<Key> {
    let next = match read_byte()? {
        Some(b) => b,
        None => return Ok(Key::Esc), // nothing followed -> real Escape
    };

    if next != b'[' && next != b'O' {
        // Not a sequence we understand; treat the ESC as Escape.
        return Ok(Key::Esc);
    }

    let code = match read_byte()? {
        Some(b) => b,
        None => return Ok(Key::Esc),
    };

    match code {
        b'A' => Ok(Key::Up),
        b'B' => Ok(Key::Down),
        b'C' => Ok(Key::Right),
        b'D' => Ok(Key::Left),
        b'H' => Ok(Key::Home),
        b'F' => Ok(Key::End),
        // Numeric sequences like "[3~" (Delete), "[5~" (PageUp), "[6~" (PageDown).
        b'0'..=b'9' => {
            // Consume the trailing '~'.
            let _ = read_byte()?;
            match code {
                b'1' | b'7' => Ok(Key::Home),
                b'4' | b'8' => Ok(Key::End),
                b'3' => Ok(Key::Delete),
                b'5' => Ok(Key::PageUp),
                b'6' => Ok(Key::PageDown),
                _ => Ok(Key::Esc),
            }
        }
        _ => Ok(Key::Esc),
    }
}

/// Finish decoding a UTF-8 character whose leading byte we already have.
fn decode_utf8(lead: u8) -> io::Result<Key> {
    let extra = if lead >= 0xf0 {
        3
    } else if lead >= 0xe0 {
        2
    } else if lead >= 0xc0 {
        1
    } else {
        0 // stray continuation byte; nothing sensible to do
    };

    let mut bytes = vec![lead];
    for _ in 0..extra {
        // Continuation bytes arrive immediately; loop in case of timeout jitter.
        loop {
            if let Some(b) = read_byte()? {
                bytes.push(b);
                break;
            }
        }
    }

    match std::str::from_utf8(&bytes) {
        Ok(s) => match s.chars().next() {
            Some(c) => Ok(Key::Char(c)),
            None => Ok(Key::Esc),
        },
        Err(_) => Ok(Key::Esc),
    }
}

/// Run `stty <args>` and capture its stdout. stdin is inherited (our controlling
/// terminal), which is exactly what stty needs to read/set.
fn stty(args: &[&str]) -> io::Result<String> {
    let out = Command::new("stty").args(args).output()?;
    Ok(String::from_utf8_lossy(&out.stdout).to_string())
}

/// Run `stty <args>` for effect, ignoring its (empty) stdout.
fn run_stty(args: &[&str]) -> io::Result<()> {
    Command::new("stty").args(args).status()?;
    Ok(())
}
