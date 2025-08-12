use mini_graph::{Port, PortError};
use mini_graph_macros::Port;

#[test]
fn macro_port_test_basic() {
    #[derive(Port, PartialEq, Debug)]
    enum OscillatorInputs {
        Freq,
        FM,
    }

    assert_eq!(OscillatorInputs::Freq.into_index(), 0);
    assert_eq!(OscillatorInputs::FM.into_index(), 1);

    assert_eq!(OscillatorInputs::from_index(0), Ok(OscillatorInputs::Freq));
    assert_eq!(OscillatorInputs::from_index(1), Ok(OscillatorInputs::FM));
    assert_eq!(OscillatorInputs::from_index(2), Err(PortError::InvalidPort));
}