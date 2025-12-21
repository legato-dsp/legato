use std::{collections::HashMap, sync::{Arc, atomic::Ordering}};
use atomic_float::AtomicF32;

#[derive(Clone, Debug, PartialEq)]
pub enum ParamError {
    ParamNotFound,
    ParamMetaNotFound
}

#[derive(Clone, Debug, PartialEq, Hash)]
pub struct ParamKey(usize);

/// The param store, hoisted up on the context for the audio graph.
/// 
/// This is laid out in this way, rather than individual pairs of atomics,
/// to hopefully provide better caching performance, and to
/// also make it a bit easier to serialize the state of a graph for presets
/// or other functionality in the future.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct ParamStore {
    data: Arc<[AtomicF32]>
}

impl ParamStore {
    pub fn new(data: Arc<[AtomicF32]>) -> Self {
        Self {
            data
        }
    }

    #[inline(always)]
    pub fn get(&self, key: &ParamKey) -> Result<f32, ParamError> {
        self.data.get(key.0)
            .map(|v| v.load(Ordering::Relaxed))
            .ok_or(ParamError::ParamNotFound)
    }

    #[inline(always)]
    pub unsafe fn get_unchecked(&self, key: &ParamKey) -> f32 {
        // ParamKeys should not be changed at runtime, there may be a level of safety here that is acceptible.
        unsafe { self.data.get_unchecked(key.0).load(Ordering::Relaxed) }
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
    pub default: f32
}

impl Default for ParamMeta {
    fn default() -> Self {
        Self {
            name: "Uninitialized".into(),
            min: 0.0,
            max: 1.0,
            default: 0.0
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
    param_lookup: HashMap<String, ParamKey>
}

impl ParamStoreFrontend {
    pub fn new(store: Arc<[AtomicF32]>, meta: Box<[ParamMeta]>, param_lookup: HashMap<String, ParamKey>) -> Self {
        Self { 
            store,
            meta, 
            param_lookup
        }
    }
    
    /// Set a parameter's value. Note: This will be clamped by the meta info for the param.
    pub fn set_param(&self, key: ParamKey, val: f32) -> Result<(), ParamError> {
        let meta = self.meta.get(key.0).ok_or(ParamError::ParamMetaNotFound)?;
        
        let clamped = val.clamp(meta.min, meta.max);

        if let Some(item) = self.store.get(key.0) {
            item.store(clamped, Ordering::Relaxed);
            return Ok(())
        }
        Err(ParamError::ParamNotFound)
    }

    /// A function to ignore the meta information, and unsafely set a param.
    pub unsafe fn set_param_unchecked_no_clamp(&self, key: ParamKey, val: f32) {
        unsafe { self.store.get_unchecked(key.0).swap(val, Ordering::Relaxed); }
    }

    pub fn get_param(&self, key: ParamKey) -> Result<f32, ParamError> {
        match self.store.get(key.0) {
            Some(inner) => Ok(inner.load(Ordering::Relaxed)),
            None => Err(ParamError::ParamNotFound)
        }
    }

    pub fn get_key(&self, name: &'static str) -> Result<ParamKey, ParamError> {
        dbg!(&self.param_lookup);
        match self.param_lookup.get(name) {
            Some(inner) => Ok(inner.clone()),
            None => Err(ParamError::ParamNotFound)
        }
    }

    pub fn get_all(&self) -> Vec<f32> {
        self.store.iter().map(|x| x.load(Ordering::Relaxed)).collect::<Vec<f32>>()
    }
}

#[derive(Default, Debug, Clone)]
pub struct ParamStoreBuilder {
    meta: Vec<ParamMeta>,
    param_lookup: HashMap<String, ParamKey>
}

impl ParamStoreBuilder {
    // Add a param to our builder, with the given meta information.
    pub fn add_param(&mut self, unique_name: String, meta: ParamMeta) -> ParamKey {
        let key = ParamKey(self.meta.len());
        self.meta.push(meta);

        // Put it in this string to key lookup for later use in the frontend
        self.param_lookup.insert(unique_name, key.clone());
    
        key
    }

    pub fn build(self) -> (ParamStoreFrontend, ParamStore) {
        let data_vec = self.meta.iter().map(|x| {
            // Quickly check bounds on param
            assert!(x.default <= x.max);
            assert!(x.default >= x.min);
            AtomicF32::new(x.default)
        }).collect::<Vec<AtomicF32>>();

        let data: Arc<[AtomicF32]> = Arc::from(data_vec);

        let store = ParamStore::new(data.clone());

        let frontend = ParamStoreFrontend::new(data, self.meta.into(), self.param_lookup);

        (frontend, store)
    }
}