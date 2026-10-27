use atomic_float::AtomicF32;
use std::{
    collections::HashMap,
    sync::{Arc, atomic::Ordering},
};

#[derive(Clone, Debug, PartialEq, thiserror::Error)]
pub enum ParamError {
    #[error("param not found")]
    ParamNotFound,
    #[error("param metadata not found")]
    ParamMetaNotFound,
    #[error("param '{0}' was already registered with different metadata")]
    ConflictingMeta(String),
    #[error("param '{0}' needs min <= default <= max")]
    InvalidRange(String),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ParamKey(usize);

/// The param store, hoisted up on the context for the audio graph.
///
/// This is laid out in this way, rather than individual pairs of atomics,
/// to hopefully provide better caching performance, and to
/// also make it a bit easier to serialize the state of a graph for presets
/// or other functionality in the future.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct ParamStore {
    data: Arc<[AtomicF32]>,
}

impl ParamStore {
    pub fn new(data: Arc<[AtomicF32]>) -> Self {
        Self { data }
    }

    #[inline(always)]
    pub fn get(&self, key: &ParamKey) -> Result<f32, ParamError> {
        self.data
            .get(key.0)
            .map(|v| v.load(Ordering::Relaxed))
            .ok_or(ParamError::ParamNotFound)
    }
}

/// A struct of meta information for a param, useful for debugging or visualizing in the UI thread
///
/// Note: You would not add smoothing behavior here, rather you would do so using a control rate lowpass node.
///
/// This is because it's easier to just block process the entire stream, we can use SIMD, etc.
#[derive(Debug, Clone, PartialEq)]
pub struct ParamMeta {
    pub name: String,
    pub min: f32,
    pub max: f32,
    pub default: f32,
}

impl Default for ParamMeta {
    fn default() -> Self {
        Self {
            name: "Uninitialized".into(),
            min: 0.0,
            max: 1.0,
            default: 0.0,
        }
    }
}

/// The "frontend" for the param store.
///
/// This is not realtime safe, and should be on a dedicated control thread.
#[derive(Debug, Clone)]
pub struct ParamStoreFrontend {
    // The underling container for our params
    store: Arc<[AtomicF32]>,
    // Box here because we want to discourage resizing after the builder constructs the pair
    meta: Box<[ParamMeta]>,
    // Lookup for name to param key
    param_lookup: HashMap<String, ParamKey>,
}

impl ParamStoreFrontend {
    pub fn new(
        store: Arc<[AtomicF32]>,
        meta: Box<[ParamMeta]>,
        param_lookup: HashMap<String, ParamKey>,
    ) -> Self {
        Self {
            store,
            meta,
            param_lookup,
        }
    }

    /// Set a parameter's value. Note: This will be clamped by the meta info for the param.
    #[inline(always)]
    pub fn set_param(&self, key: ParamKey, val: f32) -> Result<(), ParamError> {
        let meta = self.meta.get(key.0).ok_or(ParamError::ParamMetaNotFound)?;

        let clamped = val.clamp(meta.min, meta.max);

        if let Some(item) = self.store.get(key.0) {
            item.store(clamped, Ordering::Relaxed);
            return Ok(());
        }
        Err(ParamError::ParamNotFound)
    }

    #[inline(always)]
    pub fn get_param(&self, key: ParamKey) -> Result<f32, ParamError> {
        match self.store.get(key.0) {
            Some(inner) => Ok(inner.load(Ordering::Relaxed)),
            None => Err(ParamError::ParamNotFound),
        }
    }

    #[inline(always)]
    pub fn get_key(&self, name: &str) -> Result<ParamKey, ParamError> {
        match self.param_lookup.get(name) {
            Some(inner) => Ok(*inner),
            None => Err(ParamError::ParamNotFound),
        }
    }

    #[inline(always)]
    pub fn get_all(&self) -> Vec<f32> {
        self.store
            .iter()
            .map(|x| x.load(Ordering::Relaxed))
            .collect::<Vec<f32>>()
    }
}

#[derive(Default, Debug, Clone)]
pub struct ParamStoreBuilder {
    meta: Vec<ParamMeta>,
    param_lookup: HashMap<String, ParamKey>,
}

impl ParamStoreBuilder {
    /// Add a param, or return the existing key if `unique_name` is already
    /// registered with identical meta.
    pub fn add_param(
        &mut self,
        unique_name: String,
        meta: ParamMeta,
    ) -> Result<ParamKey, ParamError> {
        // Written so NaN in any field is rejected.
        if !(meta.min <= meta.default && meta.default <= meta.max) {
            return Err(ParamError::InvalidRange(unique_name));
        }

        if let Some(&key) = self.param_lookup.get(&unique_name) {
            return if self.meta[key.0] == meta {
                Ok(key)
            } else {
                Err(ParamError::ConflictingMeta(unique_name))
            };
        }

        let key = ParamKey(self.meta.len());
        self.meta.push(meta);
        self.param_lookup.insert(unique_name, key);

        Ok(key)
    }

    pub fn build(self) -> (ParamStoreFrontend, ParamStore) {
        let data_vec = self
            .meta
            .iter()
            .map(|x| AtomicF32::new(x.default))
            .collect::<Vec<AtomicF32>>();

        let data: Arc<[AtomicF32]> = Arc::from(data_vec);

        let store = ParamStore::new(data.clone());

        let frontend = ParamStoreFrontend::new(data, self.meta.into(), self.param_lookup);

        (frontend, store)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn meta(max: f32) -> ParamMeta {
        ParamMeta {
            name: "cutoff".into(),
            min: 0.0,
            max,
            default: 0.0,
        }
    }

    #[test]
    fn same_name_and_meta_shares_one_slot() {
        let mut b = ParamStoreBuilder::default();
        let a = b.add_param("cutoff".into(), meta(1.0)).unwrap();
        let c = b.add_param("cutoff".into(), meta(1.0)).unwrap();
        assert_eq!(a, c);

        let (frontend, _) = b.build();
        assert_eq!(frontend.get_all().len(), 1);
    }

    #[test]
    fn same_name_with_different_meta_is_rejected() {
        let mut b = ParamStoreBuilder::default();
        b.add_param("cutoff".into(), meta(1.0)).unwrap();
        assert_eq!(
            b.add_param("cutoff".into(), meta(2.0)),
            Err(ParamError::ConflictingMeta("cutoff".into()))
        );
    }

    #[test]
    fn default_outside_range_is_rejected() {
        let mut b = ParamStoreBuilder::default();
        for (min, max, default) in [(0.0, 1.0, 2.0), (0.0, 1.0, -1.0), (1.0, 0.0, 0.5)] {
            let m = ParamMeta {
                name: "cutoff".into(),
                min,
                max,
                default,
            };
            assert_eq!(
                b.add_param("cutoff".into(), m),
                Err(ParamError::InvalidRange("cutoff".into()))
            );
        }
    }

    #[test]
    fn nan_in_meta_is_rejected() {
        let mut b = ParamStoreBuilder::default();
        let m = ParamMeta {
            default: f32::NAN,
            ..meta(1.0)
        };
        assert_eq!(
            b.add_param("cutoff".into(), m),
            Err(ParamError::InvalidRange("cutoff".into()))
        );
    }

    #[test]
    fn distinct_names_get_distinct_slots() {
        let mut b = ParamStoreBuilder::default();
        let a = b.add_param("a.cutoff".into(), meta(1.0)).unwrap();
        let c = b.add_param("b.cutoff".into(), meta(2.0)).unwrap();
        assert_ne!(a, c);
    }
}
