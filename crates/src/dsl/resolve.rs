//! Endpoint resolution — the single place that turns the multiplicity authored
//! in the DSL (node selectors, port slices, fan-in/out) into a definite wiring.
//!
//! The arity rules are NumPy-style **broadcasting**: two selections are
//! compatible when they are the same length (zip), or one of them has length 1
//! (broadcast); otherwise it is an error. Historically this rule was
//! re-implemented in [`crate::dsl::expand`], [`crate::dsl::spawn`] and the
//! builder; [`broadcast`] is the one shared implementation.

use crate::dsl::{ir::*, pipeline::GraphPass};

/// Resolve a strided/sliced [`Port`] to the concrete single-port index for the
/// `i`-th instance of a multiplicity; non-multi ports pass through unchanged.
///
/// This is the per-instance counterpart to [`broadcast`] and is shared by every
/// pass that fans a port range across instances ([`crate::dsl::expand`],
/// [`crate::dsl::spawn`]).
pub fn port_for_instance(port: &Port, i: usize) -> Port {
    match port {
        Port::Stride { start, stride, .. } => Port::Index(start + i * stride),
        Port::Slice(start, _) => Port::Index(start + i),
        other => other.clone(),
    }
}

/// The two selections of a connection could not be matched under the
/// broadcasting rules (i.e. an `n:m` connection with `n != m`, neither being 1).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BroadcastError {
    pub src: usize,
    pub snk: usize,
}

impl std::fmt::Display for BroadcastError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "cannot broadcast connection of arity {}:{} (neither side is 1 and lengths differ)",
            self.src, self.snk
        )
    }
}

/// How a source selection maps onto a sink selection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Plan<S, T> {
    /// Equal-length (incl. 1:1): pair element-wise.
    Zip(Vec<(S, T)>),
    /// One source broadcast to many sinks.
    OneToMany(S, Vec<T>),
    /// Many sources reduced into one sink (fan-in).
    ManyToOne(Vec<S>, T),
}

/// Match a source selection against a sink selection under broadcasting rules:
///
/// - `(n, n)` → [`Plan::Zip`] (includes the `1:1` case)
/// - `(1, n)` with `n > 1` → [`Plan::OneToMany`]
/// - `(n, 1)` with `n > 1` → [`Plan::ManyToOne`]
/// - either side empty → empty [`Plan::Zip`] (nothing to wire)
/// - otherwise → [`BroadcastError`]
pub fn broadcast<S: Clone, T: Clone>(srcs: &[S], snks: &[T]) -> Result<Plan<S, T>, BroadcastError> {
    match (srcs.len(), snks.len()) {
        (0, _) | (_, 0) => Ok(Plan::Zip(Vec::new())),
        (s, t) if s == t => Ok(Plan::Zip(
            srcs.iter().cloned().zip(snks.iter().cloned()).collect(),
        )),
        (1, _) => Ok(Plan::OneToMany(srcs[0].clone(), snks.to_vec())),
        (_, 1) => Ok(Plan::ManyToOne(srcs.to_vec(), snks[0].clone())),
        (s, t) => Err(BroadcastError { src: s, snk: t }),
    }
}

/// The final graph pass: every macro has been flattened and every multi-node
/// spawned, so every edge can now be reduced to a definite selector-free
/// [`Pin`] -> [`Pin`] wiring that maps 1:1 onto the builder's connect calls.
///
/// `NodeSelector` only ever expresses node multiplicity, which earlier passes
/// have already materialised into distinct nodes; this pass therefore collapses
/// every remaining selector to [`NodeSelector::Single`], leaving port-level
/// resolution (`Named`/`None`/arity/fan-mix) to the builder, where the
/// instantiated port layouts are known.
#[derive(Default)]
pub struct ResolvePass;

impl GraphPass for ResolvePass {
    fn name(&self) -> &'static str {
        "ResolvePass"
    }

    fn run(&self, mut graph: IRGraph) -> IRGraph {
        debug_assert!(
            !graph.has_unresolved_macros(),
            "ResolvePass: macros must be expanded before resolving"
        );

        // Rebuild every edge as a finalized Pin -> Pin wire. Selectors are pure
        // node-multiplicity sugar already consumed by expand/spawn, so they are
        // normalised to `Single` here to give a single, canonical post-resolve form.
        for edge in graph.take_edges() {
            graph.connect_pin(
                Pin::new(edge.source, edge.source_port),
                Pin::new(edge.sink, edge.sink_port),
            );
        }

        graph
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_to_one_is_zip() {
        assert_eq!(broadcast(&[1], &['a']), Ok(Plan::Zip(vec![(1, 'a')])));
    }

    #[test]
    fn equal_lengths_zip() {
        assert_eq!(
            broadcast(&[1, 2, 3], &['a', 'b', 'c']),
            Ok(Plan::Zip(vec![(1, 'a'), (2, 'b'), (3, 'c')]))
        );
    }

    #[test]
    fn single_source_broadcasts() {
        assert_eq!(
            broadcast(&[1], &['a', 'b', 'c']),
            Ok(Plan::OneToMany(1, vec!['a', 'b', 'c']))
        );
    }

    #[test]
    fn single_sink_fans_in() {
        assert_eq!(
            broadcast(&[1, 2, 3], &['a']),
            Ok(Plan::ManyToOne(vec![1, 2, 3], 'a'))
        );
    }

    #[test]
    fn mismatched_arity_errors() {
        assert_eq!(
            broadcast(&[1, 2, 3], &['a', 'b']),
            Err(BroadcastError { src: 3, snk: 2 })
        );
    }

    #[test]
    fn empty_selection_is_noop() {
        assert_eq!(broadcast::<i32, char>(&[], &['a']), Ok(Plan::Zip(vec![])));
        assert_eq!(broadcast::<i32, char>(&[1], &[]), Ok(Plan::Zip(vec![])));
    }
}
