// For now, we are just using u8 values. Maybe Enums for note names in the future

use crate::mini_graph::node::Node;
use crate::mini_graph::bang::{Bang};

pub struct MidiToF {}
impl MidiToF {
    #[inline(always)]
    fn midi_to_f(&self, midi: u8) -> f32 {
      440.0 * (f32::powf(2.0, (midi as f32 - 69.0) / 12.0) )
    }
}
impl<const N: usize, const C: usize> Node<N, C> for MidiToF {
    fn handle_bang(&mut self, inputs: &[Bang], output: &mut Bang) {
        if let Some(input) = inputs.get(0) {
            match *input {
                Bang::BangMidi(msg) => {
                    *output = Bang::BangF32(self.midi_to_f(msg.key))
                },
                _ => ()
            } 
        }
    }
}


#[cfg(test)]
mod tests {
    use super::*;

    const EPS: f32 = 0.01;

    fn approx_eq(a: f32, b: f32) -> bool {
        (a - b).abs() < EPS
    }

    #[test]
    fn test_known_notes() {
        let node = MidiToF {};
        let cases = [
            (0u8,    8.1758),     // C-1
            (32u8, 51.91),        // G#1
            (69u8, 440.0000),     // A4
            (103u8, 3135.96),      // G7
            (127u8, 12543.85)     // G9
        ];

        for &(midi, expected) in &cases {
            let freq = node.midi_to_f(midi);
            assert!(
                approx_eq(freq, expected),
                "midi_to_f({}) = {}, expected ~{}",
                midi,
                freq,
                expected
            );
        }
    }
}
