use legato_core::{nodes::ports::PortBuilder, runtime::{context::Config, runtime::{Runtime, RuntimeBackend}}};

use crate::{
    ast::{BuildAstError, build_ast},
    ir::{IR, ValidationError, build_runtime_from_ir},
    parse::parse_legato_file,
};

pub mod ast;
#[macro_use]
pub mod ir;
pub mod parse;

#[derive(Debug)]
pub enum BuildApplicationError {
    ParseError(Box<dyn std::error::Error>),
    BuildAstError(BuildAstError),
    ValidationError(ValidationError),
}

pub fn build_application(
    graph: &String,
    config: Config,
) -> Result<(Runtime, RuntimeBackend), BuildApplicationError>
{

    let parsed = parse_legato_file(&graph).map_err(|x| BuildApplicationError::ParseError(x))?;
    let ast = build_ast(parsed).map_err(|x| BuildApplicationError::BuildAstError(x))?;
    let ir = IR::from(ast);

    let chans = config.channels;


    let (runtime, backend) = build_runtime_from_ir(
        ir,
        config,
        PortBuilder::default()
            .audio_out(chans)
            .build()
    );

    Ok((runtime, backend))
}
