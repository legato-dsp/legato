use std::collections::BTreeSet;

use legato_core::runtime::builder::AddNode;

use crate::ir::{ValidationError, params::Params};

pub struct NodeSpec {
    pub required: BTreeSet<String>,
    pub optional: BTreeSet<String>,
    pub build: fn(&Params) -> Result<AddNode, ValidationError>,
}

macro_rules! param_list {
    ($($param:expr),* $(,)?) => {
        {
            let mut set = BTreeSet::new();
            $(set.insert(String::from($param));)*
            set
        }
    };
}

#[macro_export]
macro_rules! node_spec {
    (
        $name:expr,
        required = [$($req:expr),*],
        optional = [$($opt:expr),*],
        build = $build:expr
    ) => {
        {
            let mut req_params = std::collections::BTreeSet::new();
            $(req_params.insert(String::from($req));)*

            let mut opt_params = std::collections::BTreeSet::new();
            $(opt_params.insert(String::from($opt));)*

            (
                $name,
                NodeSpec {
                    required: req_params,
                    optional: opt_params,
                    build: $build,
                }
            )
        }
    };
}

