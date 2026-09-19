//! Questions slacker asks on its stdin, recognised from the exact formats in
//! its source. slacker prints a question without a newline, flushes, and
//! waits, so only the last, unfinished line of output can be one; the lines
//! above it carry the options or the numbered items.
//!
//! The four shapes, and where they come from:
//! - `... [y/N]`: `confirm()`
//! - `Choice [c/r/a]:` after `[x]word  text` lines: `confirm_conflicts`,
//!   `ask_dep_conflict`, `ask_protected_dep`
//! - `Enter numbers to VERB ...`: `select_packages`, `select_packages_pkgid`
//! - `N repo(s) to update. ...`: `cmd_update`
//!
//! Anything else is not guessed at: the caller treats it as unknown.

/// One answer to a `Choice [..]:` question.
#[derive(Debug, Clone, PartialEq)]
pub struct Opt {
    /// What to send, e.g. `c`.
    pub key: String,
    /// The word slacker prints, brackets removed: `[c]ontinue` -> `continue`.
    pub label: String,
    /// The explanation printed next to it.
    pub detail: String,
    /// slacker marks the answer an empty line gives with "(default)".
    pub default: bool,
}

/// One line of a numbered list.
#[derive(Debug, Clone, PartialEq)]
pub struct Item {
    pub number: u32,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Question {
    /// `Proceed with installation? [y/N]`. The capital letter is the answer
    /// an empty line (or end of input) gives.
    YesNo { text: String, default_yes: bool },
    /// Named options, one key each.
    Choice { heading: Option<String>, options: Vec<Opt> },
    /// Pick some of a numbered list, all of it, or none.
    Pick {
        heading: String,
        items: Vec<Item>,
        /// What to send for "all of them".
        all: &'static str,
        /// What to send for "none".
        none: &'static str,
    },
}

impl Question {
    /// The answer to send for a subset of a `Pick`, in slacker's own syntax
    /// (`parse_selection` accepts numbers separated by spaces).
    pub fn pick_answer(&self, chosen: &[u32]) -> Option<String> {
        let Question::Pick { items, all, none, .. } = self else { return None };
        Some(if chosen.is_empty() {
            none.to_string()
        } else if chosen.len() == items.len() {
            all.to_string()
        } else {
            chosen.iter().map(u32::to_string).collect::<Vec<_>>().join(" ")
        })
    }
}

/// Recognises the question slacker is waiting on, if any. `before` holds the
/// finished lines printed before the unfinished one, oldest first.
pub fn detect(before: &[&str], partial: &str) -> Option<Question> {
    let p = partial.trim();
    if p.is_empty() {
        return None;
    }
    yes_no(p)
        .or_else(|| choice(before, p))
        .or_else(|| package_pick(before, p))
        .or_else(|| repo_pick(before, p))
}

fn yes_no(p: &str) -> Option<Question> {
    let (text, default_yes) = if let Some(t) = p.strip_suffix("[y/N]") {
        (t, false)
    } else if let Some(t) = p.strip_suffix("[Y/n]") {
        (t, true)
    } else {
        return None;
    };
    let text = text.trim();
    (!text.is_empty()).then(|| Question::YesNo { text: text.to_string(), default_yes })
}

/// `Choice [s/r/a/b]:`
fn choice(before: &[&str], p: &str) -> Option<Question> {
    let keys = p.strip_prefix("Choice [")?.strip_suffix("]:")?;
    let keys: Vec<&str> = keys.split('/').collect();
    if keys.iter().any(|k| k.chars().count() != 1) {
        return None;
    }

    // The option lines sit directly above; the first line above them that
    // is not an option says what the question is about.
    let mut found: Vec<Opt> = Vec::new();
    let mut heading = None;
    for line in before.iter().rev() {
        if line.trim().is_empty() {
            if found.is_empty() {
                continue;
            }
            break;
        }
        match option_line(line) {
            Some(o) => found.push(o),
            None => {
                heading = Some(line.trim().to_string());
                break;
            }
        }
    }

    let options = keys
        .iter()
        .map(|k| {
            found.iter().find(|o| o.key == *k).cloned().unwrap_or(Opt {
                key: k.to_string(),
                label: k.to_string(),
                detail: String::new(),
                default: false,
            })
        })
        .collect();
    Some(Question::Choice { heading, options })
}

/// `[c]ontinue   install anyway`, `skip-[a]ll  keep ...`, `a[b]ort  cancel ...`:
/// one word holding a single bracketed key, then its explanation.
fn option_line(line: &str) -> Option<Opt> {
    let t = line.trim_start();
    let (token, rest) = t.split_once(char::is_whitespace)?;
    let open = token.find('[')?;
    let close = open + token[open..].find(']')?;
    let key = &token[open + 1..close];
    if key.chars().count() != 1 || !key.chars().all(char::is_alphanumeric) {
        return None;
    }
    let detail = rest.trim();
    if detail.is_empty() {
        return None;
    }
    let default = detail.contains("(default)");
    Some(Opt {
        key: key.to_string(),
        label: format!("{}{}{}", &token[..open], key, &token[close + 1..]),
        detail: detail.replace("(default)", "").trim().to_string(),
        default,
    })
}

/// `Enter numbers to install (e.g. 1 3 5 or 2-4), [Enter] for all, [n] to cancel:`
/// under `'install' matched 3 packages:` and `    1) [repo] name-ver`.
fn package_pick(before: &[&str], p: &str) -> Option<Question> {
    if !(p.starts_with("Enter numbers to ") && p.ends_with("[n] to cancel:")) {
        return None;
    }
    let mut items = Vec::new();
    let mut heading = String::new();
    for line in before.iter().rev() {
        let t = line.trim();
        if t.is_empty() {
            continue;
        }
        let numbered = t
            .split_once(") ")
            .and_then(|(n, rest)| Some((n.parse::<u32>().ok()?, rest)));
        match numbered {
            Some((n, rest)) => items.push(Item { number: n, text: rest.trim().to_string() }),
            None => {
                heading = t.to_string();
                break;
            }
        }
    }
    items.reverse();
    if items.is_empty() {
        return None;
    }
    Some(Question::Pick { heading, items, all: "", none: "n" })
}

/// `2 repo(s) to update. Update [a]ll / numbers (e.g. 1 2) / [n]one? [a]:`
/// under the table `   1  conraid     80  updates available`, where repos
/// that are current show `-` instead of a number.
fn repo_pick(before: &[&str], p: &str) -> Option<Question> {
    if !(p.contains("repo(s) to update.") && p.ends_with("[n]one? [a]:")) {
        return None;
    }
    let heading = p.split_once("repo(s) to update.").map(|(n, _)| format!("{}repo(s) to update", n))?;
    let mut items = Vec::new();
    for line in before.iter().rev() {
        let t = line.trim();
        if t.is_empty() {
            continue;
        }
        if t.chars().all(|c| c == '-') {
            break; // the rule under the table header
        }
        let mut f = t.split_whitespace();
        let (Some(num), Some(name), Some(pri)) = (f.next(), f.next(), f.next()) else { continue };
        let (Ok(n), Ok(_)) = (num.parse::<u32>(), pri.parse::<i32>()) else { continue };
        let status: Vec<&str> = f.collect();
        items.push(Item { number: n, text: format!("{name} \u{2014} {}", status.join(" ")) });
    }
    items.reverse();
    (!items.is_empty()).then(|| Question::Pick { heading: heading.trim().to_string(), items, all: "a", none: "n" })
}

#[cfg(test)]
mod tests {
    use super::*;

    // The texts below are what slacker prints with colour off (stdout is a
    // pipe), copied from the println!/print! calls in its main.rs.

    fn lines(s: &str) -> Vec<&str> {
        s.lines().collect()
    }

    #[test]
    fn confirm_is_yes_no_with_no_as_default() {
        let q = detect(&[], "Proceed with installation? [y/N] ").unwrap();
        assert_eq!(q, Question::YesNo { text: "Proceed with installation?".into(), default_yes: false });
    }

    #[test]
    fn conflicts_choice_reads_every_option() {
        let before = lines(
            "  ATTENTION: 1 package conflict with what is already installed:\n    \
             foo conflicts with the installed bar-2.0-x86_64-1\n    \
             [c]ontinue   install anyway, leave the conflicting package(s)\n    \
             [r]emove     removepkg bar first, then continue\n    \
             [a]bort      cancel, change nothing (default)",
        );
        let Some(Question::Choice { heading, options }) = detect(&before, "  Choice [c/r/a]: ") else {
            panic!("not a choice")
        };
        assert_eq!(heading.as_deref(), Some("foo conflicts with the installed bar-2.0-x86_64-1"));
        let keys: Vec<&str> = options.iter().map(|o| o.key.as_str()).collect();
        assert_eq!(keys, ["c", "r", "a"]);
        assert_eq!(options[0].label, "continue");
        assert_eq!(options[1].detail, "removepkg bar first, then continue");
        assert!(options[2].default && !options[0].default);
        assert_eq!(options[2].detail, "cancel, change nothing");
    }

    #[test]
    fn keys_inside_a_word_are_found() {
        // ask_dep_conflict: skip-[a]ll and a[b]ort
        let before = lines(
            "    [s]kip      keep the installed version (default)\n    \
             [r]eplace   install the conraid's version instead\n    \
             skip-[a]ll  keep installed for this and all later conflicts\n    \
             a[b]ort     cancel the whole operation, change nothing more",
        );
        let Some(Question::Choice { options, .. }) = detect(&before, "  Choice [s/r/a/b]: ") else {
            panic!("not a choice")
        };
        let labels: Vec<&str> = options.iter().map(|o| o.label.as_str()).collect();
        assert_eq!(labels, ["skip", "replace", "skip-all", "abort"]);
        assert!(options[0].default);
    }

    #[test]
    fn protected_dep_choice_has_its_heading() {
        let before = lines(
            "\n  'libfoo' (needed by 'bar'):\n    \
             [k]eep      keep the installed SBo (default)\n    \
             [r]eplace   install conraid instead\n    \
             keep-[a]ll  keep this and every remaining one\n    \
             [q]uit      cancel the whole operation, change nothing",
        );
        let Some(Question::Choice { heading, options }) = detect(&before, "  Choice [k/r/a/q]: ") else {
            panic!("not a choice")
        };
        assert_eq!(heading.as_deref(), Some("'libfoo' (needed by 'bar'):"));
        assert_eq!(options.len(), 4);
        assert_eq!(options[3].label, "quit");
    }

    #[test]
    fn package_picker_lists_the_numbered_lines() {
        let before = lines(
            "'upgrade' matched 3 packages:\n    \
             1) [slackware] glibc-2.44-x86_64-5\n    \
             2) [slackware] vim-9.1.2-x86_64-1\n    \
             3) [conraid] foo-1.0-x86_64-1cf -> foo-1.1-x86_64-1cf",
        );
        let q = detect(
            &before,
            "Enter numbers to upgrade (e.g. 1 3 5 or 2-4), [Enter] for all, [n] to cancel: ",
        )
        .unwrap();
        let Question::Pick { heading, items, .. } = &q else { panic!("not a pick") };
        assert_eq!(heading, "'upgrade' matched 3 packages:");
        assert_eq!(items.len(), 3);
        assert_eq!(items[0], Item { number: 1, text: "[slackware] glibc-2.44-x86_64-5".into() });
        // All -> Enter, none -> n, a subset -> the numbers.
        assert_eq!(q.pick_answer(&[1, 2, 3]).as_deref(), Some(""));
        assert_eq!(q.pick_answer(&[]).as_deref(), Some("n"));
        assert_eq!(q.pick_answer(&[1, 3]).as_deref(), Some("1 3"));
    }

    #[test]
    fn update_offers_only_the_repos_that_need_it() {
        let before = lines(
            "   #  Repo        Pri  Status\n  \
             -------------------------------------\n  \
             -  patches     200  up-to-date\n   \
             1  slackware   100  updates available\n   \
             2  conraid      80  unreachable (will retry)\n",
        );
        let q = detect(&before, "2 repo(s) to update. Update [a]ll / numbers (e.g. 1 2) / [n]one? [a]: ").unwrap();
        let Question::Pick { heading, items, all, none } = &q else { panic!("not a pick") };
        assert_eq!(heading, "2 repo(s) to update");
        assert_eq!(items.len(), 2);
        assert_eq!(items[1].text, "conraid \u{2014} unreachable (will retry)");
        assert_eq!((*all, *none), ("a", "n"));
        assert_eq!(q.pick_answer(&[2]).as_deref(), Some("2"));
    }

    #[test]
    fn a_progress_line_is_not_a_question() {
        assert_eq!(detect(&[], "    glibc-2.44-x86_64-5.txz: 80.0 / 100.0 MB (80%)    "), None);
        assert_eq!(detect(&[], "Upgrading glibc..."), None);
        assert_eq!(detect(&[], ""), None);
    }

    #[test]
    fn a_choice_without_option_lines_still_answers_with_the_keys() {
        let Some(Question::Choice { options, .. }) = detect(&[], "Choice [y/n]:") else { panic!() };
        assert_eq!(options.iter().map(|o| o.label.as_str()).collect::<Vec<_>>(), ["y", "n"]);
    }
}
