//! The only place that decides which slacker command lines the GUI runs
//! and whether each one runs as the desktop user or as root.
//!
//! The split follows slacker's own `requires_privilege`: search, info,
//! list-repos, status, check-updates and history are free for anyone;
//! everything that writes needs root.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Privilege {
    /// Runs as the desktop user; never changes the system.
    User,
    /// Runs as root through pkexec.
    Root,
}

#[derive(Clone, Debug)]
pub struct Spec {
    pub args: Vec<String>,
    pub privilege: Privilege,
}

fn user(args: &[&str]) -> Spec {
    Spec {
        args: args.iter().map(|a| a.to_string()).collect(),
        privilege: Privilege::User,
    }
}

/// Root commands always carry `--yes`: there is no terminal behind the GUI,
/// so slacker must not stop and wait for an answer on stdin. The GUI asks
/// for confirmation itself before running one of these.
fn root(args: &[&str], names: &[String]) -> Spec {
    let mut v: Vec<String> = args.iter().map(|a| a.to_string()).collect();
    v.extend(names.iter().cloned());
    v.push("--yes".to_string());
    Spec {
        args: v,
        privilege: Privilege::Root,
    }
}

/// slacker requires root even to list frozen rules and pins (they are not
/// in its `requires_privilege` read-only list). No `--yes`: listing asks
/// nothing.
fn root_query(args: &[&str]) -> Spec {
    Spec {
        args: args.iter().map(|a| a.to_string()).collect(),
        privilege: Privilege::Root,
    }
}

// ---- read-only, desktop user --------------------------------------------

pub fn version() -> Spec {
    user(&["--version"])
}

pub fn status() -> Spec {
    user(&["status"])
}

/// slacker's search takes exactly one exact name (case-insensitive).
pub fn search(name: &str) -> Spec {
    user(&["search", name])
}

/// info takes exactly one name.
pub fn info(name: &str) -> Spec {
    user(&["info", name])
}

pub fn list_repos() -> Spec {
    user(&["list-repos"])
}

/// Probes the official mirrors; prints suggestions, never edits `mirrors`.
pub fn find_mirror() -> Spec {
    user(&["find-mirror"])
}

pub fn check_updates() -> Spec {
    user(&["check-updates"])
}

pub fn history_recent(last: usize) -> Spec {
    let n = last.to_string();
    user(&["history", "--last", &n])
}

pub fn history_installed() -> Spec {
    user(&["history", "--installed"])
}

// ---- listing that slacker allows only as root -----------------------------

pub fn frozen_list() -> Spec {
    root_query(&["frozen"])
}

pub fn pin_list() -> Spec {
    root_query(&["pin"])
}

// ---- frozen rules and pins ------------------------------------------------
//
// `frozen RULE` and `pin REPO:PKG` print what they would write and then ask
// for confirmation. Run without `--yes`, with stdin closed, the answer is
// end-of-file, slacker's `confirm()` reads it as "no", prints "aborted —
// nothing changed" and writes nothing. That run is the preview; the same
// command with `--yes` applies it. (`--dry-run` is NOT used here: `frozen`
// and `pin` do not look at it.)

pub fn freeze_preview(rule: &str) -> Spec {
    root_query(&["frozen", rule])
}

pub fn freeze(rule: &str) -> Spec {
    root(&["frozen"], &[rule.to_string()])
}

pub fn pin_preview(repo: &str, package: &str) -> Spec {
    let target = format!("{repo}:{package}");
    root_query(&["pin", &target])
}

pub fn pin(repo: &str, package: &str) -> Spec {
    root(&["pin"], &[format!("{repo}:{package}")])
}

/// `pri-repo PRIORITY NAME` prints the change and asks; like `frozen`, the
/// run without `--yes` is the preview. Priorities are whole numbers; the GUI
/// only offers non-negative ones, so the value can never read as an option.
pub fn pri_repo_preview(priority: u32, name: &str) -> Spec {
    let p = priority.to_string();
    root_query(&["pri-repo", &p, name])
}

pub fn pri_repo(priority: u32, name: &str) -> Spec {
    root(&["pri-repo"], &[priority.to_string(), name.to_string()])
}

/// `unfrozen` removes the rule whose text matches exactly; it asks nothing.
pub fn unfreeze(rule: &str) -> Spec {
    root_query(&["unfrozen", rule])
}

/// `unpin` removes the package's pin; it asks nothing.
pub fn unpin(package: &str) -> Spec {
    root_query(&["unpin", package])
}

// ---- system changes, root -----------------------------------------------

pub fn update() -> Spec {
    root(&["update"], &[])
}

pub fn install_new() -> Spec {
    root(&["install-new"], &[])
}

pub fn upgrade_all() -> Spec {
    root(&["upgrade-all"], &[])
}

pub fn install(names: &[String]) -> Spec {
    root(&["install"], names)
}

pub fn reinstall(names: &[String]) -> Spec {
    root(&["reinstall"], names)
}

pub fn remove(names: &[String]) -> Spec {
    root(&["remove"], names)
}

/// Checks a typed freeze rule. Spaces are allowed (`@repo PATTERN`); a
/// leading `-` is not, so the text can never become an option.
pub fn rule_text(input: &str) -> Result<String, String> {
    let t = input.trim();
    if t.is_empty() {
        return Err("Type a rule.".to_string());
    }
    if t.starts_with('-') {
        return Err("A rule cannot start with \u{201c}-\u{201d}.".to_string());
    }
    if t.chars().any(char::is_control) {
        return Err("The rule contains control characters.".to_string());
    }
    Ok(t.to_string())
}

/// Checks one typed package name. Anything starting with `-` is refused so
/// typed text can never turn into a slacker option.
pub fn single_name(input: &str) -> Result<String, String> {
    let t = input.trim();
    if t.is_empty() {
        return Err("Type a package name.".to_string());
    }
    if t.chars().any(char::is_whitespace) {
        return Err("Search takes one exact package name.".to_string());
    }
    if t.starts_with('-') {
        return Err(format!("\u{201c}{t}\u{201d} starts with \u{201c}-\u{201d} and would be read as an option."));
    }
    if t.chars().any(char::is_control) {
        return Err("The name contains control characters.".to_string());
    }
    Ok(t.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn root_commands_end_with_yes() {
        let n = vec!["vim".to_string()];
        for s in [update(), install_new(), upgrade_all(), install(&n), reinstall(&n), remove(&n)] {
            assert_eq!(s.privilege, Privilege::Root);
            assert_eq!(s.args.last().map(String::as_str), Some("--yes"));
        }
    }

    #[test]
    fn read_only_commands_run_as_user() {
        for s in [
            version(),
            status(),
            search("vim"),
            info("vim"),
            list_repos(),
            check_updates(),
            find_mirror(),
            history_recent(10),
            history_installed(),
        ] {
            assert_eq!(s.privilege, Privilege::User);
            assert!(!s.args.iter().any(|a| a == "--yes"));
        }
    }

    #[test]
    fn root_listings_carry_no_arguments() {
        for (s, cmd) in [(frozen_list(), "frozen"), (pin_list(), "pin")] {
            assert_eq!(s.privilege, Privilege::Root);
            // No argument means "list"; anything more would add a rule.
            assert_eq!(s.args, vec![cmd]);
        }
    }

    #[test]
    fn previews_never_carry_yes_and_applies_always_do() {
        for s in [freeze_preview("@testing kernel-generic"), pin_preview("alienbob", "vlc")] {
            assert_eq!(s.privilege, Privilege::Root);
            assert!(!s.args.iter().any(|a| a == "--yes"), "{:?}", s.args);
        }
        assert_eq!(freeze_preview("@testing kernel-generic").args, vec!["frozen", "@testing kernel-generic"]);
        assert_eq!(freeze("kde/").args, vec!["frozen", "kde/", "--yes"]);
        assert_eq!(pin_preview("alienbob", "vlc").args, vec!["pin", "alienbob:vlc"]);
        assert_eq!(pin("alienbob", "vlc").args, vec!["pin", "alienbob:vlc", "--yes"]);
        assert_eq!(pri_repo_preview(61, "alienbob").args, vec!["pri-repo", "61", "alienbob"]);
        assert_eq!(pri_repo(61, "alienbob").args, vec!["pri-repo", "61", "alienbob", "--yes"]);
        assert_eq!(pri_repo_preview(61, "alienbob").privilege, Privilege::Root);
        assert_eq!(unfreeze("fcitx5*").args, vec!["unfrozen", "fcitx5*"]);
        assert_eq!(unpin("vlc").args, vec!["unpin", "vlc"]);
    }

    #[test]
    fn typed_rules_are_checked() {
        assert_eq!(rule_text(" @alienbob vlc ").unwrap(), "@alienbob vlc");
        assert!(rule_text("").is_err());
        assert!(rule_text("--yes").is_err());
    }

    #[test]
    fn search_and_info_take_one_argument() {
        assert_eq!(search("vim").args, vec!["search", "vim"]);
        assert_eq!(info("vim").args, vec!["info", "vim"]);
        assert_eq!(history_recent(300).args, vec!["history", "--last", "300"]);
    }

    #[test]
    fn typed_names_are_checked() {
        assert_eq!(single_name("  vim ").unwrap(), "vim");
        assert!(single_name("  ").is_err());
        assert!(single_name("vim emacs").is_err());
        assert!(single_name("--yes").is_err());
        assert!(single_name("a\u{7}b").is_err());
    }
}
