//! DSL code lives here. This creates a separation that will let us later feature
//! gate the DSL. Legato can be used without the DSL by using the methods present
//! on the LegatoBuilder.

pub mod expand;
pub mod ir;
pub mod lower;
pub mod parse;
pub mod pipeline;
pub mod resolve;
pub mod spawn;
