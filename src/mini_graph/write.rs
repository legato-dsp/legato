use cpal::{FromSample, SizedSample};

use crate::mini_graph::audio_graph::DynamicAudioGraph;

// / The function that takes an input from the audio pipeline, 
// / and delivers it to the CPAL slice. The CPAL slice is a 
// / frame of a certain buffer size. If you request a buffer size of 256,
// / with 2 channels, the output will have a length of 512. This function
// / also takes ownership of the audio pipeline.
#[inline(always)]
pub fn write_data<const BUFFER_SIZE: usize, const CHANNEL_COUNT: usize, T>(
    output: &mut [T],
    audio_graph: &mut DynamicAudioGraph<BUFFER_SIZE, CHANNEL_COUNT>
)
where
    T: SizedSample + FromSample<f64>,
{    
    
    let next_pipeline_buffer = audio_graph.next_block(None);

    for (frame_index, frame) in output.chunks_mut(CHANNEL_COUNT).enumerate() {
        for (channel, sample) in frame.iter_mut().enumerate() {
            let pipeline_next_frame = &next_pipeline_buffer[channel];
            *sample = T::from_sample(pipeline_next_frame[frame_index] as f64);
        }
    }
}
