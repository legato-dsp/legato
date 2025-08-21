// use assert_no_alloc::*;
// use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
// use cpal::{BufferSize, BuildStreamError, FromSample, SampleRate, SizedSample, StreamConfig};

// #[cfg(debug_assertions)]
// #[global_allocator]
// static A: AllocDisabler = AllocDisabler;

// const SAMPLE_RATE: u32 = 48_000;
// const FRAME_SIZE: usize = 1024;
// const CHANNEL_COUNT: usize = 2;

// #[inline(always)]
// pub fn write_data<const BUFFER_SIZE: usize, const CHANNEL_COUNT: usize, T>(
//     output: &mut [T],
//     audio_graph: &mut DynamicAudioGraph<BUFFER_SIZE, CHANNEL_COUNT>
// )
// where
//     T: SizedSample + FromSample<f64>,
// {    
    
//     let next_pipeline_buffer = audio_graph.next_block(None);

//     for (frame_index, frame) in output.chunks_mut(CHANNEL_COUNT).enumerate() {
//         for (channel, sample) in frame.iter_mut().enumerate() {
//             let pipeline_next_frame = &next_pipeline_buffer[channel];
//             *sample = T::from_sample(pipeline_next_frame[frame_index] as f64);
//         }
//     }
// }

// fn run<T>(device: &cpal::Device, config: &cpal::StreamConfig) -> Result<(), BuildStreamError>
// where
//     T: SizedSample + FromSample<f64> {

   

//     // Build CPAL output stream
//     let stream = device.build_output_stream(
//         config,
//         move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
//             assert_no_alloc(|| write_data::<FRAME_SIZE, CHANNEL_COUNT, f32>(data, &mut audio_graph))
//         },
//         |err| eprintln!("An output stream error occurred: {}", err),
//         None,
//     )?;

//     stream.play().unwrap();


//     std::thread::park(); // Keep alive

//     Ok(())
// }


// fn main() {
//     let host = cpal::host_from_id(cpal::HostId::Jack).expect("JACK host not available");
//     let device = host.default_output_device().expect("No output device available");

//     let config = StreamConfig {
//         channels: CHANNEL_COUNT as u16,
//         sample_rate: SampleRate(SAMPLE_RATE),
//         buffer_size: BufferSize::Fixed(FRAME_SIZE as u32),
//     };

//     run::<f32>(&device, &config.into()).unwrap();
// }

fn main(){
    todo!()
}