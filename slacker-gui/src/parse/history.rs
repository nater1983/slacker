//! `slacker history` (render_history):
//! `YYYY-MM-DD HH:MM  {sym} {label:<11}  {name:<w}  {detail}  [{source}]`

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Kind {
    Installed,
    Reinstalled,
    Upgraded,
    Removed,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Event {
    pub when: String,
    pub kind: Kind,
    pub name: String,
    pub detail: String,
    pub source: String,
}

#[derive(Debug, Default)]
pub struct History {
    pub events: Vec<Event>,
    pub messages: Vec<String>,
}

pub fn parse(text: &str) -> History {
    let mut out = History::default();
    for line in text.lines() {
        if line.trim().is_empty() {
            continue;
        }
        match parse_line(line) {
            Some(e) => out.events.push(e),
            None => out.messages.push(line.trim().to_string()),
        }
    }
    out
}

fn parse_line(line: &str) -> Option<Event> {
    if line.len() < 18 || !line.is_char_boundary(16) {
        return None;
    }
    let (when, rest) = line.split_at(16);
    let b = when.as_bytes();
    if !(b[4] == b'-' && b[7] == b'-' && b[10] == b' ' && b[13] == b':') {
        return None;
    }
    let rest = rest.strip_prefix("  ")?;
    let mut chars = rest.chars();
    let sym = chars.next()?;
    let rest = chars.as_str().strip_prefix(' ')?;
    let cols = super::split_columns(rest);
    if cols.len() != 4 {
        return None;
    }
    let kind = match (sym, cols[0]) {
        ('+', "installed") => Kind::Installed,
        ('\u{21BB}', "reinstalled") => Kind::Reinstalled,
        ('\u{2191}', "upgraded") => Kind::Upgraded,
        ('\u{2212}', "removed") => Kind::Removed,
        _ => return None,
    };
    let source = cols[3].strip_prefix('[')?.strip_suffix(']')?;
    Some(Event {
        when: when.to_string(),
        kind,
        name: cols[1].to_string(),
        detail: cols[2].to_string(),
        source: source.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse::fixture;

    #[test]
    fn all_kinds() {
        let h = parse(&fixture("history.txt"));
        assert!(h.messages.is_empty(), "{:?}", h.messages);
        let kinds: Vec<Kind> = h.events.iter().map(|e| e.kind).collect();
        assert_eq!(kinds, vec![Kind::Upgraded, Kind::Reinstalled, Kind::Installed, Kind::Removed]);
        assert_eq!(h.events[0].detail, "2.44-4 \u{2192} 2.44-5");
        assert_eq!(h.events[1].source, "SBo");
        assert_eq!(h.events[2].when, "2026-09-13 08:01");
        assert_eq!(h.events[3].name, "oldpkg");
    }

    #[test]
    fn empty_history_is_a_message() {
        let h = parse("No matching package history.\n");
        assert!(h.events.is_empty());
        assert_eq!(h.messages, vec!["No matching package history."]);
    }
}
