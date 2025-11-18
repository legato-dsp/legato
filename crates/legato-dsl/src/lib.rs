use generic_array::{ArrayLength};
use std::ops::Mul;
use legato_core::{application::Application, engine::{node::FrameSize, port::Ports, runtime::RuntimeBackend}, nodes::utils::port_utils::generate_audio_outputs};
use typenum::{Prod, U0, U2};

use crate::{ast::{BuildAstError, build_ast}, ir::{IR, ValidationError, build_runtime_from_ir}, parse::parse_legato_file};

pub mod ast;
pub mod ir;
pub mod parse;


#[derive(Debug)]
pub enum BuildApplicationError {
    ParseError(Box<dyn std::error::Error>),
    BuildAstError(BuildAstError),
    ValidationError(ValidationError)
}

pub struct ApplicationConfig {
    pub intitial_capacity: usize,
    pub sample_rate: usize,
    pub control_rate: usize,
}

pub fn build_application<AF, CF, C>(graph: &String, config: ApplicationConfig) -> Result<(Application<AF, CF, C>, RuntimeBackend), BuildApplicationError> where
    AF: FrameSize + Mul<U2>,
    Prod<AF, U2>: FrameSize,
    CF: FrameSize,
    C: ArrayLength 
{
    let parsed = parse_legato_file(&graph).map_err(|x| BuildApplicationError::ParseError(x))?;
    let ast = build_ast(parsed).map_err(|x| BuildApplicationError::BuildAstError(x))?;
    let ir = IR::<AF, CF>::from(ast);

    let (runtime, backend) = build_runtime_from_ir::<AF, CF, C, U0>(ir, config.intitial_capacity, config.sample_rate as u32, config.control_rate, Ports::<C, C, U0, U0> {
        audio_inputs: None,
        audio_outputs: Some(generate_audio_outputs()),
        control_inputs: None,
        control_outputs: None,
    });

    Ok((Application::new(runtime), backend))
}