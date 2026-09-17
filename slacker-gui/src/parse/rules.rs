//! `slacker frozen` and `slacker pin` without arguments (cmd_frozen, cmd_pin):
//! the current freeze rules and the current pins.

#[derive(Debug, Default, PartialEq)]
pub struct Frozen {
    pub rules: Vec<String>,
    pub unrecognized: Vec<String>,
}

#[derive(Debug, Default, PartialEq)]
pub struct Pins {
    /// (package, repo)
    pub pins: Vec<(String, String)>,
    pub unrecognized: Vec<String>,
}

pub fn parse_frozen(text: &str) -> Frozen {
    let mut out = Frozen::default();
    let mut in_list = false;
    for line in text.lines() {
        let t = line.trim();
        if t.is_empty() || t == "No frozen rules set." || t.starts_with("to add a rule:") {
            continue;
        }
        if t == "Current frozen rules:" {
            in_list = true;
            continue;
        }
        match line.strip_prefix("  ") {
            Some(rule) if in_list => out.rules.push(rule.trim_end().to_string()),
            _ => out.unrecognized.push(line.to_string()),
        }
    }
    out
}

pub fn parse_pins(text: &str) -> Pins {
    let mut out = Pins::default();
    let mut in_list = false;
    for line in text.lines() {
        let t = line.trim();
        if t.is_empty() || t == "No pins set." || t.starts_with("to add a pin:") {
            continue;
        }
        if t == "Current pins (package -> repo):" {
            in_list = true;
            continue;
        }
        let pin = line
            .strip_prefix("  ")
            .filter(|_| in_list)
            .and_then(|l| l.split_once(" -> "))
            .map(|(p, r)| (p.trim().to_string(), r.trim().to_string()))
            .filter(|(p, r)| !p.is_empty() && !r.is_empty() && !p.contains(' ') && !r.contains(' '));
        match pin {
            Some(p) => out.pins.push(p),
            None => out.unrecognized.push(line.to_string()),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frozen_empty_from_capture() {
        let text = "No frozen rules set.\n  to add a rule: `frozen <rule>`, e.g. `frozen vlc`, `frozen kde/`, `frozen \"@alienbob vlc\"`\n";
        assert_eq!(parse_frozen(text), Frozen::default());
    }

    #[test]
    fn frozen_rules() {
        let text = "Current frozen rules:\n  sbopkg\n  fcitx5*\n  @testing xf86-.*-202.*\n  @alienbob 100% vlc\n  kde/\n  to add a rule: `frozen <rule>`, e.g. `frozen vlc`\n";
        let f = parse_frozen(text);
        assert!(f.unrecognized.is_empty());
        assert_eq!(f.rules, vec!["sbopkg", "fcitx5*", "@testing xf86-.*-202.*", "@alienbob 100% vlc", "kde/"]);
    }

    #[test]
    fn pins_empty_from_capture() {
        let text = "No pins set.\n  to add a pin: `pin repo:package`, e.g. `pin alienbob:vlc`\n";
        assert_eq!(parse_pins(text), Pins::default());
    }

    #[test]
    fn pins_listed() {
        let text = "Current pins (package -> repo):\n  vlc -> alienbob\n  python3.11 -> conraid\n  gtk+ -> slackware\n  to add a pin: `pin repo:package`, e.g. `pin alienbob:vlc`\n";
        let p = parse_pins(text);
        assert!(p.unrecognized.is_empty());
        assert_eq!(
            p.pins,
            vec![
                ("vlc".to_string(), "alienbob".to_string()),
                ("python3.11".to_string(), "conraid".to_string()),
                ("gtk+".to_string(), "slackware".to_string()),
            ]
        );
    }

    #[test]
    fn refused_authorization_is_not_a_list() {
        let p = parse_pins("Error executing command as another user: Not authorized\n");
        assert!(p.pins.is_empty());
        assert_eq!(p.unrecognized.len(), 1);
    }
}
