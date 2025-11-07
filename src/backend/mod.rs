use crate::engine::runtime::Runtime;
use cpal::{FromSample, SizedSample};
use generic_array::ArrayLength;

// Here, we provide a couple of backends. CPAL is recommended for most usecases.
// We also will have a write_data_pipe, that can be used to easily pipe into Python
// to graph aliasing and other visualizations.

#[inline(always)]
pub fn write_data_cpal<const AF: usize, const CF: usize, C, Ci, T>(
    output: &mut [T],
    runtime: &mut Runtime<AF, CF, C, Ci>,
) where
    T: SizedSample + FromSample<f64>,
    C: ArrayLength,
    Ci: ArrayLength,
{
    let next_block = runtime.next_block(None);

    for (frame_index, frame) in output.chunks_mut(C::USIZE).enumerate() {
        for (channel, sample) in frame.iter_mut().enumerate() {
            let pipeline_next_frame = &next_block[channel];
            *sample = T::from_sample(pipeline_next_frame[frame_index] as f64);
        }
    }
}
