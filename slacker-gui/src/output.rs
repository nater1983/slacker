//! Cleaning of raw command output, and the monospace text views that show it.

use gtk::prelude::*;

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

/// A read-only monospace view on `buffer` that follows new output.
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
    let follow = view.clone();
    let m = mark.clone();
    buffer.connect_changed(move |_| follow.scroll_mark_onscreen(&m));
    let start = view.clone();
    gtk::glib::idle_add_local_once(move || start.scroll_mark_onscreen(&mark));
    scroller
}

#[cfg(test)]
mod tests {
    use super::clean;

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
