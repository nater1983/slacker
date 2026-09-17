//! Finds the slacker binary. A normal user's PATH on Slackware has no sbin
//! directories, so the GUI uses absolute paths and says which one it picked.

use std::path::{Path, PathBuf};

/// Searched in this order. The polkit policy in `data/` has one action per
/// path (a test below keeps the two lists in step).
pub const CANDIDATES: [&str; 4] = [
    "/usr/bin/slacker",
    "/usr/sbin/slacker",
    "/usr/local/bin/slacker",
    "/usr/local/sbin/slacker",
];
pub const ENV_OVERRIDE: &str = "SLACKER_BIN";

pub struct Resolved {
    pub path: PathBuf,
    pub found: bool,
    pub notes: Vec<String>,
}

pub fn slacker_binary() -> Resolved {
    if let Some(p) = std::env::var_os(ENV_OVERRIDE) {
        let path = PathBuf::from(p);
        let mut notes = vec![format!("Using {ENV_OVERRIDE}.")];
        if !path.is_absolute() {
            notes.push(format!("{ENV_OVERRIDE} must be an absolute path."));
            return Resolved { path, found: false, notes };
        }
        let found = is_executable(&path);
        if !found {
            notes.push(format!("{} is not an executable file.", path.display()));
        }
        return Resolved { path, found, notes };
    }

    let present: Vec<&str> = CANDIDATES
        .iter()
        .copied()
        .filter(|c| is_executable(Path::new(c)))
        .collect();

    match present.as_slice() {
        [] => Resolved {
            path: PathBuf::from(CANDIDATES[0]),
            found: false,
            notes: vec![format!(
                "slacker was not found in {}. Set {ENV_OVERRIDE} to its full path.",
                CANDIDATES.join(", ")
            )],
        },
        [only] => Resolved {
            path: PathBuf::from(only),
            found: true,
            notes: Vec::new(),
        },
        [first, rest @ ..] => Resolved {
            path: PathBuf::from(first),
            found: true,
            notes: vec![format!(
                "Also found {}. Check that {first} is the build you expect, or set {ENV_OVERRIDE}.",
                rest.join(", ")
            )],
        },
    }
}

fn is_executable(p: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(p)
        .map(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::CANDIDATES;

    const POLICY: &str = include_str!("../data/nl.slackware.forge.rizitis.slacker.policy");

    #[test]
    fn every_candidate_has_a_polkit_action() {
        for c in CANDIDATES {
            let annotation = format!(
                "<annotate key=\"org.freedesktop.policykit.exec.path\">{c}</annotate>"
            );
            assert!(POLICY.contains(&annotation), "no polkit action for {c}");
        }
        assert_eq!(POLICY.matches("<action id=").count(), CANDIDATES.len());
    }

    #[test]
    fn every_action_keeps_authorization() {
        assert_eq!(POLICY.matches(">auth_admin_keep<").count(), 3 * CANDIDATES.len());
        assert!(!POLICY.contains("auth_admin<"));
    }
}
