//! `slacker find-mirror` (cmd_find_mirror). Read-only: slacker only
//! suggests; changing the mirror stays a manual edit of the `mirrors` file.
//!
//! Ranked rows: `"  {:>2}. {:<3} {:>7} {:<13} {url}[ (yours)]"`.

#[derive(Debug, Clone, PartialEq)]
pub enum Freshness {
    InSync,
    /// e.g. "1d 23h"
    Behind(String),
    /// upstream could not be reached
    Unknown,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Mirror {
    pub rank: u32,
    pub country: String,
    pub latency_ms: u32,
    pub freshness: Freshness,
    pub url: String,
    /// The host of the mirror currently configured.
    pub yours: bool,
}

#[derive(Debug, Default)]
pub struct FindMirror {
    /// `Some(date)` when freshness was checked against upstream.
    pub validated: Option<String>,
    pub upstream_unreachable: bool,
    /// "(50 of 64 reachable)"
    pub reachable: Option<(u32, u32)>,
    pub ranked: Vec<Mirror>,
    /// Lines to put in the `mirrors` file, fastest first.
    pub suggested: Vec<String>,
    pub unrecognized: Vec<String>,
}

pub fn parse(text: &str) -> FindMirror {
    let mut out = FindMirror::default();
    let mut in_suggestions = false;
    for line in text.lines() {
        let t = line.trim();
        if t.is_empty()
            || t == "Finding the fastest up-to-date Slackware mirror"
            || (t.starts_with("probing ") && t.ends_with("mirrors in parallel ..."))
            || t == "(slacker will not change your mirror automatically)"
            || t == "to use it, make this the single active line in your `mirrors` file:"
        {
            continue;
        }
        if let Some(rest) = t.strip_prefix("\u{2713} freshness validated against upstream ") {
            out.validated = Some(
                rest.split_once('(')
                    .and_then(|(_, d)| d.strip_suffix(')'))
                    .unwrap_or(rest)
                    .to_string(),
            );
            continue;
        }
        if t.starts_with("upstream unreachable") {
            out.upstream_unreachable = true;
            continue;
        }
        if t.starts_with("Top ") && t.contains(" mirrors: (") {
            out.reachable = t
                .split_once('(')
                .and_then(|(_, r)| r.strip_suffix(" reachable)"))
                .and_then(|r| r.split_once(" of "))
                .and_then(|(a, b)| Some((a.parse().ok()?, b.parse().ok()?)));
            continue;
        }
        if t.starts_with("Fastest ") && t.ends_with("in your `mirrors` file:") {
            in_suggestions = true;
            continue;
        }
        if t.starts_with("Fastest: ") {
            in_suggestions = true;
            continue;
        }
        if in_suggestions && line.starts_with("    ") && is_url(t) && !t.contains(' ') {
            out.suggested.push(t.to_string());
            continue;
        }
        match parse_row(line) {
            Some(m) => out.ranked.push(m),
            None => out.unrecognized.push(line.to_string()),
        }
    }
    out
}

fn is_url(s: &str) -> bool {
    s.starts_with("https://") || s.starts_with("http://") || s.starts_with("file://")
}

fn parse_row(line: &str) -> Option<Mirror> {
    // "  " + rank right-aligned to 2 + ". "
    let rest = line.strip_prefix("  ")?;
    let (rank, rest) = rest.split_once(". ")?;
    let rank: u32 = rank.trim().parse().ok()?;
    if !rest.is_ascii() || rest.len() < 12 {
        return None;
    }
    let country = rest[..3].trim().to_string();
    let rest = rest[3..].strip_prefix(' ')?;
    let latency = rest[..7].trim();
    let latency_ms: u32 = latency.strip_suffix("ms")?.parse().ok()?;
    let rest = rest[7..].strip_prefix(' ')?;
    // The URL is the first token that looks like one; freshness is before it.
    let at = ["https://", "http://", "file://"]
        .iter()
        .filter_map(|p| rest.find(p))
        .min()?;
    let fresh = rest[..at].trim();
    let mut url = rest[at..].trim();
    let yours = match url.strip_suffix("(yours)") {
        Some(u) => {
            url = u.trim_end();
            true
        }
        None => false,
    };
    if url.contains(' ') {
        return None;
    }
    let freshness = match fresh {
        "in sync" => Freshness::InSync,
        "?" => Freshness::Unknown,
        f => Freshness::Behind(f.strip_suffix(" behind")?.to_string()),
    };
    Some(Mirror { rank, country, latency_ms, freshness, url: url.to_string(), yours })
}

/// What the page offers as lines for the `mirrors` file.
#[derive(Debug, PartialEq)]
pub enum Suggestions {
    /// Mirrors slacker found in sync with upstream, fastest first.
    InSync(Vec<String>),
    /// Upstream could not be reached: slacker's own proposals, unchecked.
    Unchecked(Vec<String>),
    /// Freshness was checked and none of the ranked mirrors is in sync.
    NoneInSync,
}

/// slacker proposes the fastest mirrors even when they are behind upstream.
/// A mirror days behind must not be offered, so when freshness was checked
/// only in-sync mirrors are suggested. The release/arch part of the line
/// (`slackware64-current/`) is taken from slacker's own proposals, never
/// worked out here.
pub fn suggestions(f: &FindMirror, limit: usize) -> Suggestions {
    if f.validated.is_none() {
        return Suggestions::Unchecked(f.suggested.clone());
    }
    // slacker's proposal = base URL + "/" + release dir + "/".
    let suffix = f.suggested.iter().find_map(|line| {
        f.ranked.iter().find_map(|m| {
            line.strip_prefix(&format!("{}/", m.url.trim_end_matches('/')))
                .filter(|rest| !rest.is_empty())
        })
    });
    let Some(suffix) = suffix else {
        return Suggestions::NoneInSync;
    };
    let lines: Vec<String> = f
        .ranked
        .iter()
        .filter(|m| m.freshness == Freshness::InSync)
        .take(limit)
        .map(|m| format!("{}/{}", m.url.trim_end_matches('/'), suffix))
        .collect();
    if lines.is_empty() {
        Suggestions::NoneInSync
    } else {
        Suggestions::InSync(lines)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse::fixture;

    #[test]
    fn captured_ranking() {
        let f = parse(&fixture("find-mirror.txt"));
        assert!(f.unrecognized.is_empty(), "{:?}", f.unrecognized);
        assert_eq!(f.validated.as_deref(), Some("Mon Sep 14 22:19:04 UTC 2026"));
        assert_eq!(f.reachable, Some((50, 64)));
        assert_eq!(f.ranked.len(), 7);
        let first = &f.ranked[0];
        assert_eq!(
            (first.rank, first.country.as_str(), first.latency_ms),
            (1, "bg", 205)
        );
        assert_eq!(first.freshness, Freshness::Behind("1d 23h".into()));
        assert_eq!(first.url, "https://mirrors.netix.net/slackware/");
        assert_eq!(f.ranked[1].freshness, Freshness::Behind("1d".into()));
        assert_eq!(f.ranked[2].freshness, Freshness::InSync);
        assert!(f.ranked.iter().all(|m| !m.yours));
        assert_eq!(f.suggested.len(), 3);
        assert_eq!(f.suggested[0], "https://mirrors.netix.net/slackware/slackware64-current/");
    }

    #[test]
    fn upstream_down_single_suggestion_and_yours() {
        let f = parse(&fixture("find-mirror-single.txt"));
        assert!(f.unrecognized.is_empty(), "{:?}", f.unrecognized);
        assert!(f.upstream_unreachable && f.validated.is_none());
        let m = &f.ranked[0];
        assert_eq!((m.latency_ms, m.yours), (76, true));
        assert_eq!(m.freshness, Freshness::Unknown);
        assert_eq!(m.url, "https://mirror.koddos.net/slackware/");
        assert_eq!(f.suggested, vec!["https://mirror.koddos.net/slackware/slackware64-current/"]);
    }

    #[test]
    fn stale_mirrors_are_never_suggested() {
        let f = parse(&fixture("find-mirror.txt"));
        // slacker proposes netix (1d 23h behind) and sox (1d behind) first.
        assert!(f.suggested[0].contains("netix"));
        assert_eq!(
            suggestions(&f, 3),
            Suggestions::InSync(vec![
                "https://mirror.telepoint.bg/slackware/slackware64-current/".into(),
                "https://mirror.wheel.sk/slackware/slackware64-current/".into(),
                "https://mirror.koddos.net/slackware/slackware64-current/".into(),
            ])
        );
    }

    #[test]
    fn unchecked_freshness_keeps_slackers_proposals() {
        let f = parse(&fixture("find-mirror-single.txt"));
        assert_eq!(
            suggestions(&f, 3),
            Suggestions::Unchecked(vec!["https://mirror.koddos.net/slackware/slackware64-current/".into()])
        );
    }

    #[test]
    fn nothing_in_sync_means_no_suggestion() {
        let mut f = parse(&fixture("find-mirror.txt"));
        for m in &mut f.ranked {
            m.freshness = Freshness::Behind("2d".into());
        }
        assert_eq!(suggestions(&f, 3), Suggestions::NoneInSync);
    }

    #[test]
    fn errors_are_kept() {
        let f = parse("Finding the fastest up-to-date Slackware mirror\nslacker: error: no https mirrors found in the mirror list\n");
        assert!(f.ranked.is_empty());
        assert_eq!(f.unrecognized.len(), 1);
    }
}
