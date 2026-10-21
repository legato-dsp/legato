//! Handles let a node hand the application one end of whatever it uses to
//! talk across threads: a ring buffer, an atomic, a triple buffer, or your own.
//!
//! A factory runs once per spawned instance, so it creates a fresh pair on
//! every call, keeps the audio-thread end inside the node, and exports the
//! other end with [`crate::builder::ResourceBuilderView::export_handle`]. After
//! building, the application takes it back from the
//! [`crate::LegatoFrontend`] by the instance's path, e.g. `keys` or
//! `voice.0.keys`.
//!
//! Legato never inspects a handle. It is stored as `Box<dyn Any + Send>` and
//! downcast when taken, which compares [`std::any::TypeId`]s, so asking for
//! the wrong type is an error rather than undefined behaviour.

use std::{
    any::{Any, type_name},
    cmp::Ordering,
    collections::BTreeMap,
};

use crate::spec::NodeDefinition;

/// A node that exports a handle of a known type.
///
/// Implementing this lets the application ask for a handle by node type, with
/// [`crate::LegatoFrontend::take_handle_of`], instead of naming the handle type
/// and risking a mismatch.
pub trait ExportsHandle: NodeDefinition {
    type Handle: Any + Send;
}

#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum HandleError {
    /// No handle was exported at this path (or under this prefix).
    #[error("no handle at '{path}' ({})", available_list(available))]
    NotFound {
        path: String,
        available: Vec<String>,
    },
    /// A handle exists at `path`, but it is not the requested type.
    #[error("{}", type_mismatch(path, expected, found))]
    TypeMismatch {
        path: String,
        expected: &'static str,
        found: &'static str,
    },
    /// The handle at `path` was exported by a different kind of node.
    #[error("handle at '{path}' was exported by a `{found}` node, not a `{expected}`")]
    WrongNodeKind {
        path: String,
        expected: &'static str,
        found: String,
    },
}

fn available_list(available: &[String]) -> String {
    if available.is_empty() {
        "no nodes exported handles".into()
    } else {
        format!("available: {}", available.join(", "))
    }
}

fn type_mismatch(path: &str, expected: &str, found: &str) -> String {
    if expected == found {
        format!(
            "handle at '{path}' is not a `{expected}`, although the names match: \
             this usually means two versions of the same crate are in the dependency tree"
        )
    } else {
        format!("handle at '{path}' is a `{found}`, not the requested `{expected}`")
    }
}

struct StoredHandle {
    value: Box<dyn Any + Send>,
    // Only used in error messages; type names are not guaranteed unique.
    type_name: &'static str,
    // Filled in by the builder once the factory has succeeded.
    node_kind: Option<String>,
}

/// Handles exported while building, keyed by instance path.
#[derive(Default)]
pub struct HandleStore {
    handles: BTreeMap<String, StoredHandle>,
}

impl std::fmt::Debug for HandleStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_map()
            .entries(self.handles.iter().map(|(path, h)| (path, h.type_name)))
            .finish()
    }
}

impl HandleStore {
    /// Store a handle at `path`. Returns `false` if one is already there.
    pub(crate) fn insert<H: Any + Send>(&mut self, path: &str, handle: H) -> bool {
        if self.handles.contains_key(path) {
            return false;
        }
        self.handles.insert(
            path.to_string(),
            StoredHandle {
                value: Box::new(handle),
                type_name: type_name::<H>(),
                node_kind: None,
            },
        );
        true
    }

    pub(crate) fn set_node_kind(&mut self, path: &str, kind: &str) {
        if let Some(h) = self.handles.get_mut(path) {
            h.node_kind = Some(kind.to_string());
        }
    }

    /// Paths with a handle still waiting to be taken, in instance order.
    pub fn paths(&self) -> Vec<String> {
        let mut paths: Vec<String> = self.handles.keys().cloned().collect();
        paths.sort_by(|a, b| natural_path_cmp(a, b));
        paths
    }

    pub fn take<H: Any + Send>(&mut self, path: &str) -> Result<H, HandleError> {
        self.check_type::<H>(path)?;
        let stored = self.handles.remove(path).expect("checked above");
        Ok(*stored.value.downcast::<H>().expect("checked above"))
    }

    pub fn take_of<N: ExportsHandle>(&mut self, path: &str) -> Result<N::Handle, HandleError> {
        self.check_kind(path, N::NAME)?;
        self.take::<N::Handle>(path)
    }

    /// Take every handle at or under `prefix`, ordered by instance index.
    ///
    /// Matching is by path segment, so `voice` matches `voice.0.keys` but not
    /// `voice_b`. Either every match is taken or none are.
    pub fn take_all<H: Any + Send>(
        &mut self,
        prefix: &str,
    ) -> Result<Vec<(String, H)>, HandleError> {
        let paths = self.matching(prefix)?;
        for path in &paths {
            self.check_type::<H>(path)?;
        }
        Ok(paths
            .into_iter()
            .map(|path| {
                let h = self.take::<H>(&path).expect("checked above");
                (path, h)
            })
            .collect())
    }

    pub fn take_all_of<N: ExportsHandle>(
        &mut self,
        prefix: &str,
    ) -> Result<Vec<(String, N::Handle)>, HandleError> {
        for path in &self.matching(prefix)? {
            self.check_kind(path, N::NAME)?;
        }
        self.take_all::<N::Handle>(prefix)
    }

    fn not_found(&self, path: &str) -> HandleError {
        HandleError::NotFound {
            path: path.to_string(),
            available: self.paths(),
        }
    }

    fn check_type<H: Any + Send>(&self, path: &str) -> Result<(), HandleError> {
        let stored = self.handles.get(path).ok_or_else(|| self.not_found(path))?;
        if stored.value.is::<H>() {
            Ok(())
        } else {
            Err(HandleError::TypeMismatch {
                path: path.to_string(),
                expected: type_name::<H>(),
                found: stored.type_name,
            })
        }
    }

    fn check_kind(&self, path: &str, expected: &'static str) -> Result<(), HandleError> {
        let stored = self.handles.get(path).ok_or_else(|| self.not_found(path))?;
        match &stored.node_kind {
            Some(kind) if kind == expected => Ok(()),
            other => Err(HandleError::WrongNodeKind {
                path: path.to_string(),
                expected,
                found: other.clone().unwrap_or_else(|| "<unknown>".into()),
            }),
        }
    }

    fn matching(&self, prefix: &str) -> Result<Vec<String>, HandleError> {
        let paths: Vec<String> = self
            .paths()
            .into_iter()
            .filter(|p| path_has_prefix(p, prefix))
            .collect();
        if paths.is_empty() {
            return Err(self.not_found(prefix));
        }
        Ok(paths)
    }
}

/// Whether `path` is `prefix` or lies under it, compared segment by segment.
fn path_has_prefix(path: &str, prefix: &str) -> bool {
    match path.strip_prefix(prefix) {
        Some(rest) => rest.is_empty() || rest.starts_with('.'),
        None => false,
    }
}

/// Order paths so instance indices compare numerically: `v.2` before `v.10`.
fn natural_path_cmp(a: &str, b: &str) -> Ordering {
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

    #[test]
    fn prefix_matches_whole_segments_only() {
        assert!(path_has_prefix("voice", "voice"));
        assert!(path_has_prefix("voice.0.keys", "voice"));
        assert!(path_has_prefix("voice.0.keys", "voice.0"));
        assert!(!path_has_prefix("voice_b.0", "voice"));
        assert!(!path_has_prefix("voice.10", "voice.1"));
    }

    #[test]
    fn instance_indices_sort_numerically() {
        let mut store = HandleStore::default();
        for i in [10, 2, 0, 1] {
            assert!(store.insert(&format!("v.{i}"), i));
        }
        assert_eq!(store.paths(), vec!["v.0", "v.1", "v.2", "v.10"]);
    }

    #[test]
    fn wrong_type_leaves_the_handle_in_place() {
        let mut store = HandleStore::default();
        store.insert("keys", 7_u32);

        let err = store.take::<f32>("keys").unwrap_err();
        assert!(matches!(err, HandleError::TypeMismatch { .. }));
        assert_eq!(store.take::<u32>("keys"), Ok(7));
        assert!(matches!(
            store.take::<u32>("keys"),
            Err(HandleError::NotFound { .. })
        ));
    }

    #[test]
    fn take_all_is_all_or_nothing() {
        let mut store = HandleStore::default();
        store.insert("v.0", 0_u32);
        store.insert("v.1", 1.0_f32);

        assert!(store.take_all::<u32>("v").is_err());
        assert_eq!(store.paths(), vec!["v.0", "v.1"]);
    }

    #[test]
    fn duplicate_insert_is_rejected() {
        let mut store = HandleStore::default();
        assert!(store.insert("keys", 1_u32));
        assert!(!store.insert("keys", 2_u32));
    }
}
