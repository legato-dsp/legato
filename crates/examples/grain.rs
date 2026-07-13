use std::{f32::consts::TAU, path::Path};

use legato::{
    builder::{LegatoBuilder, ResourceBuilderView, Unconfigured, ValidationError},
    config::Config,
    dsl::ir::DSLParams,
    interface::AudioInterface,
    midi::{MidiPortKind, start_midi_thread},
    msg::{NodeMessage, RtValue},
    node::DynNode,
    persample::{PerSample, PerSampleNode},
    ports::{PortBuilder, Ports},
    ring::RingBuffer,
    spec::NodeDefinition,
};

const REF_SR: f32 = 29761.0;

#[inline(always)]
fn allpass(ring: &mut RingBuffer, x: f32, g: f32, delay: f32) -> f32 {
    let d = ring.get_delay_cubic(delay);
    let w = x + g * d;
    ring.push(w);
    d - g * w
}

#[derive(Clone)]
pub struct Plate480 {
    ports: Ports,
    // params
    decay: f32,
    damping: f32,
    bandwidth: f32,
    mix: f32,
    // input chain
    predelay: RingBuffer,
    predelay_s: usize,
    diff: [RingBuffer; 4],
    diff_d: [f32; 4],
    idiff1: f32,
    idiff2: f32,
    // The two figure-eight branches
    map_l: RingBuffer,
    map_r: RingBuffer,
    map_l_d: f32,
    map_r_d: f32,
    del_a: RingBuffer,
    del_b: RingBuffer,
    del_c: RingBuffer,
    del_d: RingBuffer,
    a_d: usize,
    b_d: usize,
    c_d: usize,
    d_d: usize,
    ap_l2: RingBuffer,
    ap_r2: RingBuffer,
    ap_l2_d: f32,
    ap_r2_d: f32,
    dd1: f32,
    dd2: f32,
    // modulation
    excursion: f32,
    lfo_phase: f32,
    lfo_inc: f32,
    // state
    bw_state: f32,
    damp_l: f32,
    damp_r: f32,
    tank_l: f32,
    tank_r: f32,
    // final 7 output taps, one for each channel
    yl: [usize; 7],
    yr: [usize; 7],
}

/// Inspired by the Lexicon 480L
///
/// Would highly suggest this resource for understanding:
///
/// https://www.youtube.com/watch?v=Il_qdtQKnqk
impl Plate480 {
    pub fn new(
        sr: usize,
        predelay_ms: f32,
        decay: f32,
        damping: f32,
        bandwidth: f32,
        mix: f32,
    ) -> Self {
        // A few anon-functions for making scaling, rings, etc.
        let scale = sr as f32 / REF_SR;
        let s = |n: f32| (n * scale).round();
        let su = |n: f32| s(n) as usize;

        let ring = |max_read: f32| RingBuffer::new(su(max_read) + 8);

        let predelay_s = ((predelay_ms / 1000.0) * sr as f32).round() as usize;

        Self {
            ports: PortBuilder::default().audio_in(2).audio_out(2).build(),
            decay: decay.clamp(0.0, 0.9),
            damping: damping.clamp(0.0, 0.999),
            bandwidth: bandwidth.clamp(0.0, 1.0),
            mix: mix.clamp(0.0, 1.0),
            predelay: RingBuffer::new(predelay_s + 8),
            predelay_s,
            diff: [ring(142.0), ring(107.0), ring(379.0), ring(277.0)],
            diff_d: [s(142.0), s(107.0), s(379.0), s(277.0)],
            idiff1: 0.75,
            idiff2: 0.625,

            map_l: ring(672.0 + 16.0),
            map_r: ring(908.0 + 16.0),
            map_l_d: s(672.0),
            map_r_d: s(908.0),
            del_a: ring(4453.0),
            del_b: ring(3720.0),
            del_c: ring(4217.0),
            del_d: ring(3163.0),
            a_d: su(4453.0),
            b_d: su(3720.0),
            c_d: su(4217.0),
            d_d: su(3163.0),
            ap_l2: ring(1800.0),
            ap_r2: ring(2656.0),
            ap_l2_d: s(1800.0),
            ap_r2_d: s(2656.0),
            dd1: 0.70,
            dd2: 0.50,
            excursion: s(8.0),
            lfo_phase: 0.0,
            lfo_inc: 0.7 / sr as f32,
            bw_state: 0.0,
            damp_l: 0.0,
            damp_r: 0.0,
            tank_l: 0.0,
            tank_r: 0.0,
            yl: [
                su(266.0),
                su(2974.0),
                su(1913.0),
                su(1996.0),
                su(1990.0),
                su(187.0),
                su(1066.0),
            ],
            yr: [
                su(353.0),
                su(3627.0),
                su(1228.0),
                su(2673.0),
                su(2111.0),
                su(335.0),
                su(121.0),
            ],
        }
    }
}

impl PerSampleNode for Plate480 {
    fn ports(&self) -> &Ports {
        &self.ports
    }

    fn tick(&mut self, inp: &[f32], out: &mut [f32]) {
        let (dry_l, dry_r) = (inp[0], inp[1]);
        let mono = (dry_l + dry_r) * 0.5;

        // predelay + input bandwidth lowpass
        self.predelay.push(mono);
        let mut x = self.predelay.get_offset(self.predelay_s);
        self.bw_state = self.bandwidth * x + (1.0 - self.bandwidth) * self.bw_state;
        x = self.bw_state;

        // input diffusers
        x = allpass(&mut self.diff[0], x, self.idiff1, self.diff_d[0]);
        x = allpass(&mut self.diff[1], x, self.idiff1, self.diff_d[1]);
        x = allpass(&mut self.diff[2], x, self.idiff2, self.diff_d[2]);
        let diffused = allpass(&mut self.diff[3], x, self.idiff2, self.diff_d[3]);

        // modulated allpass excursion (quadrature LFO across the two branches)
        let exc_l = self.excursion * (self.lfo_phase * TAU).sin();
        let exc_r = self.excursion * (self.lfo_phase * TAU + TAU * 0.25).sin();
        self.lfo_phase = (self.lfo_phase + self.lfo_inc).fract();

        // figure-8: each branch takes diffused input + the *other* branch's last tail
        let left_in = diffused + self.tank_r;
        let right_in = diffused + self.tank_l;

        // left branch
        let t = allpass(&mut self.map_l, left_in, -self.dd1, self.map_l_d + exc_l);
        self.del_a.push(t);
        let a_out = self.del_a.get_offset(self.a_d);
        self.damp_l = (1.0 - self.damping) * a_out + self.damping * self.damp_l;
        let s = allpass(
            &mut self.ap_l2,
            self.damp_l * self.decay,
            self.dd2,
            self.ap_l2_d,
        );
        self.del_b.push(s);
        self.tank_l = self.del_b.get_offset(self.b_d);

        // right branch
        let t = allpass(&mut self.map_r, right_in, -self.dd1, self.map_r_d + exc_r);
        self.del_c.push(t);
        let c_out = self.del_c.get_offset(self.c_d);
        self.damp_r = (1.0 - self.damping) * c_out + self.damping * self.damp_r;
        let s = allpass(
            &mut self.ap_r2,
            self.damp_r * self.decay,
            self.dd2,
            self.ap_r2_d,
        );
        self.del_d.push(s);
        self.tank_r = self.del_d.get_offset(self.d_d);

        // stereo output taps
        let wet_l = 0.6
            * (self.del_c.get_offset(self.yl[0]) + self.del_c.get_offset(self.yl[1])
                - self.ap_r2.get_offset(self.yl[2])
                + self.del_d.get_offset(self.yl[3])
                - self.del_a.get_offset(self.yl[4])
                - self.ap_l2.get_offset(self.yl[5])
                - self.del_b.get_offset(self.yl[6]));

        let wet_r = 0.6
            * (self.del_a.get_offset(self.yr[0]) + self.del_a.get_offset(self.yr[1])
                - self.ap_l2.get_offset(self.yr[2])
                + self.del_b.get_offset(self.yr[3])
                - self.del_c.get_offset(self.yr[4])
                - self.ap_r2.get_offset(self.yr[5])
                - self.del_d.get_offset(self.yr[6]));

        out[0] = dry_l * (1.0 - self.mix) + wet_l * self.mix;
        out[1] = dry_r * (1.0 - self.mix) + wet_r * self.mix;
    }

    fn handle_msg(&mut self, msg: NodeMessage) {
        if let NodeMessage::SetParam(p) = msg
            && let RtValue::F32(v) = p.value
        {
            match p.param_name {
                "decay" => self.decay = v.clamp(0.0, 0.9),
                "damping" => self.damping = v.clamp(0.0, 0.999),
                "bandwidth" => self.bandwidth = v.clamp(0.0, 1.0),
                "mix" => self.mix = v.clamp(0.0, 1.0),
                _ => {}
            }
        }
    }
}

impl NodeDefinition for Plate480 {
    const NAME: &'static str = "plate480";
    const DESCRIPTION: &'static str = "Dattorro/Lexicon-480L-style plate reverb (per-sample tank)";
    const REQUIRED_PARAMS: &'static [&'static str] = &[];
    const OPTIONAL_PARAMS: &'static [&'static str] =
        &["predelay", "decay", "damping", "bandwidth", "mix"];

    fn create(
        rb: &mut ResourceBuilderView,
        p: &DSLParams,
    ) -> Result<Box<dyn DynNode>, ValidationError> {
        let sr = rb.get_config().sample_rate;
        let predelay = p.get_f32("predelay").unwrap_or(0.0);
        let decay = p.get_f32("decay").unwrap_or(0.7);
        let damping = p.get_f32("damping").unwrap_or(0.3);
        let bandwidth = p.get_f32("bandwidth").unwrap_or(0.9995);
        let mix = p.get_f32("mix").unwrap_or(0.3);
        Ok(Box::new(PerSample::new(Plate480::new(
            sr, predelay, decay, damping, bandwidth, mix,
        ))))
    }
}

fn main() {
    let graph = String::from(
        r#"
        patch voice(
            attack = 1200.0,
            decay = 300.0,
            sustain = 0.8,
            release = 700.0
        ) {
            in freq gate

            audio {
                sine: lfo { freq: 0.1 },
                grain { sampler_name: "main", chans: 2, size: 70, shape: 0.5, scan: 0.05 },
                adsr { attack: $attack, decay: $decay, sustain: $sustain, release: $release, chans: 2 },
            }

            control { 
                map { range: [-1.0, 1.0], new_range: [100, 300] }
            }

            lfo >> map
            map >> grain.size

            freq >> grain.freq
            gate >> grain.trig

            gate >> adsr.gate
            grain >> adsr[1..3]

            { adsr }
        }

        patches {
            voice * 3 { },
        }
        
        audio {
            track_mixer { tracks: 3, chans_per_track: 2 },
        }

        midi {
            poly_voice { chan: 0, voices: 3 }
        }

        user {
            plate480: verb { predelay: 32.0, decay: 0.4, damping: 0.3, mix: 0.8 }
        }

        poly_voice[0:10:3] >> voice(*).gate
        poly_voice[1:10:3] >> voice(*).freq

        voice(*)[0] >> track_mixer[0:6:2]
        voice(*)[1] >> track_mixer[1:6:2]

        track_mixer >> verb

        { verb }
    "#,
    );

    let config = Config {
        sample_rate: 44_100,
        block_size: 4096,
        channels: 2,
        rt_capacity: 0,
    };

    let ports = PortBuilder::default().audio_out(2).build();

    let midi_rt_fe = start_midi_thread(
        256,
        "my_port",
        MidiPortKind::Index(0),
        MidiPortKind::Index(0),
        "my_port",
    )
    .unwrap();

    let (app, mut frontend) = LegatoBuilder::<Unconfigured>::new(config, ports)
        .set_midi_runtime(midi_rt_fe)
        .register_node("user", Plate480::spec())
        .build_dsl(&graph);

    dbg!(&app);

    frontend
        .load_sample(
            &String::from("main"),
            Path::new("../samples/guitar.wav"),
            2,
            config.sample_rate as u32,
        )
        .expect("Could not load sample!");

    #[cfg(target_os = "macos")]
    let host = cpal::host_from_id(cpal::HostId::CoreAudio).expect("JACK host not available");

    #[cfg(target_os = "linux")]
    let host = cpal::host_from_id(cpal::HostId::Jack).expect("JACK host not available");

    AudioInterface::builder(&host, config)
        .build(app)
        .expect("Failed to start audio")
        .run_forever();
}
