//! Selections: one way to name nodes after the graph is built, shared by
//! handle lookup and batches.
//!
//! A selection is dot-separated segments, each a name with an optional DSL
//! instance selector: `voice(*).osc`, `voice(1..3)`, `saw(0)`. A numeric
//! segment is the same as an index, so `voice.0.keys` is `voice(0).keys`.
//!
//! Matching is by whole segments, so `op` never matches `op1`, and covers
//! everything below the selection: `voice` selects every node in every
//! `voice` instance, as a bare name does in the DSL.

use std::cmp::Ordering;

use chumsky::{extra::Err, prelude::*};

use crate::dsl::{ir::NodeSelector, parse::node_selector};

#[derive(Debug, Clone, PartialEq)]
struct Segment {
    name: String,
    instances: NodeSelector,
}

/// A parsed selection; see the [module docs](self).
#[derive(Debug, Clone, PartialEq)]
pub struct Selection {
    segments: Vec<Segment>,
}

#[derive(Debug, Clone, PartialEq, thiserror::Error)]
#[error("invalid selection '{selection}': {reason}")]
pub struct SelectionError {
    pub selection: String,
    pub reason: String,
}

enum Part {
    Name(String, Option<NodeSelector>),
    Index(usize),
}

fn parser<'a>() -> impl Parser<'a, &'a str, Vec<Part>, Err<Rich<'a, char>>> {
    let name = text::ascii::ident()
        .map(ToString::to_string)
        .then(node_selector().or_not())
        .map(|(n, sel)| Part::Name(n, sel));
    let index = text::digits(10)
        .to_slice()
        .map(|s: &str| Part::Index(s.parse().unwrap()));
    choice((name, index))
        .separated_by(just('.'))
        .at_least(1)
        .collect()
        .then_ignore(end())
}

impl Selection {
    pub fn parse(selection: &str) -> Result<Self, SelectionError> {
        let err = |reason: String| SelectionError {
            selection: selection.to_string(),
            reason,
        };
        let parts = parser().parse(selection).into_result().map_err(|errs| {
            err(errs
                .iter()
                .map(|e| e.to_string())
                .collect::<Vec<_>>()
                .join("; "))
        })?;

        let mut segments: Vec<Segment> = Vec::new();
        for part in parts {
            match part {
                Part::Name(name, sel) => segments.push(Segment {
                    name,
                    instances: sel.unwrap_or(NodeSelector::Single),
                }),
                // `voice.0` is `voice(0)`, so it needs a name without a selector.
                Part::Index(i) => match segments.last_mut() {
                    Some(seg) if seg.instances == NodeSelector::Single => {
                        seg.instances = NodeSelector::Index(i)
                    }
                    _ => {
                        return Err(err(format!(
                            "index {i} must follow a name without a selector"
                        )));
                    }
                },
            }
        }
        Ok(Self { segments })
    }

    /// Whether `path`, a built node's name such as `voice.0.osc`, is selected.
    pub fn matches(&self, path: &str) -> bool {
        let mut path = path.split('.').peekable();
        for seg in &self.segments {
            if path.next() != Some(seg.name.as_str()) {
                return false;
            }
            // A numeric segment is the instance index; a lone instance has none
            // and counts as instance 0. The count isn't needed to match.
            let index = match path.peek().and_then(|s| s.parse::<usize>().ok()) {
                Some(i) => {
                    path.next();
                    i
                }
                None => 0,
            };
            if !seg.instances.selects(index, usize::MAX) {
                return false;
            }
        }
        true
    }

    /// The selected paths, in instance order.
    pub fn filter<'a>(&self, paths: impl IntoIterator<Item = &'a str>) -> Vec<&'a str> {
        let mut out: Vec<&str> = paths.into_iter().filter(|p| self.matches(p)).collect();
        out.sort_by(|a, b| natural_path_cmp(a, b));
        out
    }
}

/// Order paths so instance indices compare numerically: `v.2` before `v.10`.
pub fn natural_path_cmp(a: &str, b: &str) -> Ordering {
    let mut a_segs = a.split('.');
    let mut b_segs = b.split('.');
    loop {
        match (a_segs.next(), b_segs.next()) {
            (None, None) => return Ordering::Equal,
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
            (Some(x), Some(y)) => {
                let ord = match (x.parse::<u64>(), y.parse::<u64>()) {
                    (Ok(x), Ok(y)) => x.cmp(&y),
                    _ => x.cmp(y),
                };
                if ord != Ordering::Equal {
                    return ord;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sel(s: &str) -> Selection {
        Selection::parse(s).unwrap()
    }

    const PATHS: [&str; 7] = [
        "voice.0.osc",
        "voice.1.osc",
        "voice.10.osc",
        "voice.0.amp",
        "voice_b.osc",
        "lone.osc",
        "op1",
    ];

    fn pick(s: &str) -> Vec<&'static str> {
        sel(s).filter(PATHS)
    }

    #[test]
    fn whole_segments_only() {
        assert_eq!(
            pick("voice"),
            ["voice.0.amp", "voice.0.osc", "voice.1.osc", "voice.10.osc"]
        );
        assert_eq!(pick("op"), Vec::<&str>::new());
        assert_eq!(pick("voice.1"), ["voice.1.osc"]);
    }

    #[test]
    fn instance_selectors() {
        assert_eq!(
            pick("voice(*).osc"),
            ["voice.0.osc", "voice.1.osc", "voice.10.osc"]
        );
        assert_eq!(pick("voice(1..11).osc"), ["voice.1.osc", "voice.10.osc"]);
        assert_eq!(pick("voice(0)"), pick("voice.0"));
        // A lone instance is instance 0.
        assert_eq!(pick("lone(0).osc"), ["lone.osc"]);
        assert_eq!(pick("lone(1)"), Vec::<&str>::new());
    }

    #[test]
    fn malformed_selections_are_errors() {
        for bad in ["", "voice.", "voice(*).0", "0", "voice(", "voice osc"] {
            assert!(Selection::parse(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn instance_indices_sort_numerically() {
        let mut paths = vec!["v.10", "v.2", "v.0", "v.1"];
        paths.sort_by(|a, b| natural_path_cmp(a, b));
        assert_eq!(paths, ["v.0", "v.1", "v.2", "v.10"]);
    }
}
