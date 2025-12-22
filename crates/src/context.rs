use crate::{
    config::Config,
    params::{ParamError, ParamKey},
    resources::Resources,
};

/// The AudioContext struct contains information about the current audio graph, as well as
/// some resources that are hosted up for nodes to access within a specific runtime.
///
/// This prevents complex state sharing or unsafe ptr logic when using things like shared buffers
/// for delay lines or samples.
#[derive(Clone)]
pub struct AudioContext {
    config: Config,
    resources: Resources,
}

impl AudioContext {
    pub fn new(config: Config) -> Self {
        Self {
            config,
            resources: Resources::default(),
        }
    }
    /// For a time being, this is a quick hack inside oversampling. I would recommend not using, as it does not reflex internal state!!!
    pub fn set_sample_rate(&mut self, sr: usize) {
        self.config.sample_rate = sr;
    }
    /// For a time being, this is a quick hack inside oversampling. I would recommend not using, as it does not reflex internal state!!!
    pub fn set_block_size(&mut self, block_size: usize) {
        self.config.block_size = block_size;
    }
    pub fn get_config(&self) -> Config {
        self.config
    }
    pub fn get_resources(&self) -> &Resources {
        &self.resources
    }
    pub fn set_resources(&mut self, resources: Resources) {
        self.resources = resources;
    }
    pub fn get_resources_mut(&mut self) -> &mut Resources {
        &mut self.resources
    }
    pub fn get_param(&self, key: &ParamKey) -> Result<f32, ParamError> {
        self.resources.get_param(key)
    }
    pub unsafe fn get_param_unchecked(&self, key: &ParamKey) -> f32 {
        unsafe { self.resources.get_param_unchecked(key) }
    }
}
