use crate::{
    ast::{BuildAstError},
    ir::{ValidationError},
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