//! `slacker check-updates` (cmd_check_updates). Exit 100 means pending.

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum State {
    UpToDate,
    Pending,
    Unknown,
    Unreachable,
}

const STATES: [(&str, State); 4] = [
    ("up-to-date", State::UpToDate),
    ("updates pending", State::Pending),
    ("unknown (run update first)", State::Unknown),
    ("unreachable (check its URL)", State::Unreachable),
];

#[derive(Debug, Default)]
pub struct Updates {
    pub repos: Vec<(String, State)>,
    /// Summary and warning lines, in order.
    pub notes: Vec<String>,
}

pub fn parse(text: &str) -> Updates {
    let mut out = Updates::default();
    for line in text.lines() {
        if line.trim().is_empty() {
            continue;
        }
        let row = line.strip_prefix("  ").and_then(|l| {
            STATES.iter().find_map(|(word, st)| {
                let name = l.strip_suffix(word)?.trim_end();
                (!name.is_empty() && !name.contains(' ')).then(|| (name.to_string(), *st))
            })
        });
        match row {
            Some(r) => out.repos.push(r),
            None => out.notes.push(line.trim().to_string()),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse::fixture;

    #[test]
    fn captured_all_current() {
        let u = parse(&fixture("check-updates.txt"));
        assert_eq!(u.repos.len(), 8);
        assert!(u.repos.iter().all(|(_, s)| *s == State::UpToDate));
        assert_eq!(u.notes, vec!["Everything up-to-date."]);
    }

    #[test]
    fn mixed_states_and_warnings() {
        let u = parse(&fixture("check-updates-pending.txt"));
        assert_eq!(
            u.repos,
            vec![
                ("slackware".to_string(), State::Pending),
                ("conraid".to_string(), State::Unreachable),
                ("ponce".to_string(), State::Unknown),
            ]
        );
        assert_eq!(u.notes.len(), 3);
        assert!(u.notes[0].starts_with("WARNING: verification is OFF"));
    }
}
