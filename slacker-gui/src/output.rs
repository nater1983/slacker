//! Cleaning of raw command output, and the monospace text views that show it.

use std::cell::Cell;

use gtk::prelude::*;

/// How a piece of output relates to what came before it.
#[derive(Clone, Copy, PartialEq)]
pub enum Chunk {
    /// One or more finished lines.
    Line,
    /// A line still being redrawn in place: slacker's download counter
    /// prints with a carriage return and no newline, so without this the
    /// window would show nothing at all for the length of a transfer.
    Progress,
}

/// Where a chunk of output goes.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Write {
    /// Start a new line.
    New,
    /// Overwrite the line the command is redrawing.
    OverLast,
}

/// The whole rule, kept pure so it can be tested without a display: output
/// goes on a new line unless the last line is one the command was still
/// redrawing — a further redraw replaces it, and so does the finished line,
/// which otherwise would appear twice.
pub fn write_for(redrawing: bool, _kind: Chunk) -> Write {
    if redrawing {
        Write::OverLast
    } else {
        Write::New
    }
}

/// How many lines to drop from the front to keep at most `max_lines`.
fn overflow(line_count: i32, max_lines: i32) -> i32 {
    (line_count - max_lines).max(0)
}

/// Writes command output into a buffer the way a terminal would: each
/// redraw replaces the previous one, and the finished line replaces the
/// last redraw rather than appearing under it.
pub struct Feed {
    buffer: gtk::TextBuffer,
    /// Whether the last line written is one the command is still redrawing.
    redrawing: Cell<bool>,
    max_lines: Option<i32>,
}

impl Feed {
    pub fn new(buffer: &gtk::TextBuffer) -> Self {
        Self { buffer: buffer.clone(), redrawing: Cell::new(false), max_lines: None }
    }

    /// As `new`, but keeping only the last `max_lines` lines.
    pub fn capped(buffer: &gtk::TextBuffer, max_lines: i32) -> Self {
        Self { max_lines: Some(max_lines), ..Self::new(buffer) }
    }

    pub fn buffer(&self) -> &gtk::TextBuffer {
        &self.buffer
    }

    pub fn push(&self, text: &str, kind: Chunk) {
        let redrawing = self.redrawing.replace(kind == Chunk::Progress);
        match write_for(redrawing, kind) {
            Write::OverLast => replace_last_line(&self.buffer, text),
            Write::New => append(&self.buffer, text),
        }
        if kind == Chunk::Line {
            self.cap();
        }
    }

    /// A line of the GUI's own (the command line, the exit status). It ends
    /// whatever was being redrawn.
    pub fn line(&self, text: &str) {
        self.redrawing.set(false);
        append(&self.buffer, text);
        self.cap();
    }

    fn cap(&self) {
        if let Some(max) = self.max_lines {
            trim(&self.buffer, max);
        }
    }
}

/// Turns raw bytes into display text: lossy UTF-8, ANSI escape sequences
/// removed, and carriage-return redraws (progress lines) reduced to the
/// last state, the way a terminal would show them. No trailing newline.
pub fn clean(raw: &[u8]) -> String {
    let text = String::from_utf8_lossy(raw);
    let plain = strip_ansi(&text);
    let body = plain.strip_suffix('\n').unwrap_or(&plain);
    body.split('\n')
        .map(|line| {
            let line = line.trim_end_matches('\r');
            line.rsplit('\r').next().unwrap_or(line)
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn strip_ansi(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '\u{1b}' {
            out.push(c);
            continue;
        }
        match chars.peek() {
            // CSI: ESC [ params... final byte in 0x40..=0x7e
            Some('[') => {
                chars.next();
                for f in chars.by_ref() {
                    if ('\u{40}'..='\u{7e}').contains(&f) {
                        break;
                    }
                }
            }
            // OSC: ESC ] ... terminated by BEL or ESC \
            Some(']') => {
                chars.next();
                while let Some(f) = chars.next() {
                    if f == '\u{7}' {
                        break;
                    }
                    if f == '\u{1b}' && chars.peek() == Some(&'\\') {
                        chars.next();
                        break;
                    }
                }
            }
            Some(_) => {
                chars.next();
            }
            None => {}
        }
    }
    out
}

/// Appends one block of text as its own line(s).
pub fn append(buffer: &gtk::TextBuffer, text: &str) {
    let mut end = buffer.end_iter();
    if end.offset() > 0 {
        buffer.insert(&mut end, "\n");
    }
    buffer.insert(&mut end, text);
}

/// Replaces the last line, for output that redraws itself in place
/// (slacker's download counter prints with a carriage return).
pub fn replace_last_line(buffer: &gtk::TextBuffer, text: &str) {
    let mut start = buffer.end_iter();
    start.set_line_offset(0);
    let mut end = buffer.end_iter();
    buffer.delete(&mut start, &mut end);
    let mut at = buffer.end_iter();
    buffer.insert(&mut at, text);
}

/// Drops the oldest lines once the buffer is longer than `max_lines`.
/// Without this the command log grows for as long as the window is open —
/// one `show-changelog` alone is tens of thousands of lines.
pub fn trim(buffer: &gtk::TextBuffer, max_lines: i32) {
    let extra = overflow(buffer.line_count(), max_lines);
    if extra == 0 {
        return;
    }
    let Some(mut cut) = buffer.iter_at_line(extra) else { return };
    let mut start = buffer.start_iter();
    buffer.delete(&mut start, &mut cut);
}

/// A read-only monospace view on `buffer` that follows new output.
///
/// The view owns what it adds to the buffer: the runner's log outlives every
/// dialog that shows it, so the scroll handler and its mark are removed when
/// the view goes away. Otherwise each reopening would leave another dead view
/// attached to the buffer, scrolled on every line of output for ever after.
pub fn terminal_view(buffer: &gtk::TextBuffer) -> gtk::ScrolledWindow {
    let view = gtk::TextView::builder()
        .buffer(buffer)
        .editable(false)
        .cursor_visible(false)
        .monospace(true)
        .wrap_mode(gtk::WrapMode::WordChar)
        .left_margin(16)
        .right_margin(16)
        .top_margin(14)
        .bottom_margin(14)
        .build();
    view.add_css_class("terminal");
    let scroller = gtk::ScrolledWindow::builder()
        .child(&view)
        .hexpand(true)
        .vexpand(true)
        .build();
    scroller.add_css_class("terminal-card");

    // Follow new output, and start at the end when opened on an old log.
    let mark = buffer.create_mark(None, &buffer.end_iter(), false);
    let follow = view.downgrade();
    let m = mark.clone();
    let handler = buffer.connect_changed(move |_| {
        if let Some(v) = follow.upgrade() {
            v.scroll_mark_onscreen(&m);
        }
    });
    {
        let (b, m) = (buffer.clone(), mark.clone());
        let handler = std::cell::Cell::new(Some(handler));
        view.connect_destroy(move |_| {
            if let Some(h) = handler.take() {
                b.disconnect(h);
                b.delete_mark(&m);
            }
        });
    }
    let start = view.clone();
    gtk::glib::idle_add_local_once(move || start.scroll_mark_onscreen(&mark));
    scroller
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The same rule `Feed` applies, over a list of lines instead of a
    /// GTK buffer (which can only be touched on the main thread).
    fn simulate(steps: &[(&str, Chunk)]) -> String {
        let mut lines: Vec<String> = Vec::new();
        let mut redrawing = false;
        for (text, kind) in steps {
            match write_for(redrawing, *kind) {
                Write::OverLast => {
                    lines.pop();
                    lines.extend(text.split('\n').map(str::to_string));
                }
                Write::New => lines.extend(text.split('\n').map(str::to_string)),
            }
            redrawing = *kind == Chunk::Progress;
        }
        lines.join("\n")
    }

    #[test]
    fn a_redrawn_line_is_replaced_never_repeated() {
        // What a download looks like: several redraws of one line, then the
        // finished line, then ordinary output.
        let out = simulate(&[
            ("Upgrade (1):", Chunk::Line),
            ("  glibc: 20 MB", Chunk::Progress),
            ("  glibc: 60 MB", Chunk::Progress),
            ("  glibc: 100 MB", Chunk::Progress),
            ("  glibc: 100 MB", Chunk::Line),
            ("Done.", Chunk::Line),
        ]);
        assert_eq!(out, "Upgrade (1):\n  glibc: 100 MB\nDone.");
    }

    #[test]
    fn the_chunk_that_finishes_a_redraw_may_carry_more_lines() {
        let out = simulate(&[
            ("  glibc: 20 MB", Chunk::Progress),
            ("  glibc: 100 MB\nUpgrading glibc...\nDone.", Chunk::Line),
        ]);
        assert_eq!(out, "  glibc: 100 MB\nUpgrading glibc...\nDone.");
    }

    #[test]
    fn plain_output_is_never_overwritten() {
        let out = simulate(&[("one", Chunk::Line), ("two", Chunk::Line)]);
        assert_eq!(out, "one\ntwo");
    }

    #[test]
    fn the_log_drops_only_what_is_over_the_cap() {
        assert_eq!(overflow(10, 4_000), 0);
        assert_eq!(overflow(4_000, 4_000), 0);
        assert_eq!(overflow(4_007, 4_000), 7);
    }

    #[test]
    fn strips_colour_codes() {
        assert_eq!(clean(b"\x1b[1;31mFROZEN\x1b[0m vim\n"), "FROZEN vim");
    }

    #[test]
    fn keeps_last_progress_state() {
        assert_eq!(clean(b"get  10%\rget  50%\rget 100%\n"), "get 100%");
        assert_eq!(clean(b"line one\r\nline two\r\n"), "line one\nline two");
    }

    #[test]
    fn strips_osc_sequences() {
        assert_eq!(clean(b"\x1b]0;title\x07ok\n"), "ok");
    }

    #[test]
    fn survives_invalid_utf8() {
        assert_eq!(clean(b"a\xffb\n"), "a\u{fffd}b");
    }
}
