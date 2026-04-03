/// It became clear after a while, that delay lines,
/// samplers, granular, etc. need the same underlying
/// abstraction, a window into a slice, with a few fractional
/// indexing utilities.
///
/// This primative can be used to make delay lines, samplers, etc.
///
/// This also allows us to handle all of these resources as one giant
/// continous buffer, which gives better cache locality.
pub struct Window<'a> {
    data: &'a [f32],
}

impl<'a> Window<'a> {}
