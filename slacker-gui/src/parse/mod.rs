//! Parsers for slacker's human-readable output.
//!
//! Each parser follows the exact `println!` formats in slacker's source and is
//! tested against captured output under `fixtures/`. A line a parser does not
//! recognise is never guessed at: it is kept verbatim in `unrecognized`, and
//! the page shows it as-is next to whatever was understood.

pub mod history;
pub mod mirrors;
pub mod repos;
pub mod rules;
pub mod search;
pub mod status;
pub mod updates;

/// Splits on runs of two or more spaces, the column separator slacker uses
/// between padded fields.
pub fn split_columns(s: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut rest = s.trim();
    while !rest.is_empty() {
        match rest.find("  ") {
            Some(i) => {
                out.push(&rest[..i]);
                rest = rest[i..].trim_start();
            }
            None => {
                out.push(rest);
                break;
            }
        }
    }
    out
}

#[cfg(test)]
pub(crate) fn fixture(name: &str) -> String {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures").join(name);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

#[cfg(test)]
mod tests {
    #[test]
    fn columns() {
        assert_eq!(
            super::split_columns("  a b   c  d "),
            vec!["a b", "c", "d"]
        );
        assert!(super::split_columns("   ").is_empty());
    }
}
