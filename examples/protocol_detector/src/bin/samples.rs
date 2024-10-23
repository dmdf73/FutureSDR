use clap::Parser;
use lora::utils::CodeRate;
use std::time::Duration;

use futuresdr::anyhow::Result;
use futuresdr::async_io::Timer;
use futuresdr::num_complex::Complex32;
use futuresdr::runtime::buffer::circular::Circular;
use futuresdr::runtime::Flowgraph;
use futuresdr::runtime::Pmt;
use futuresdr::runtime::Runtime;
use lora::utils::Bandwidth;
use lora::utils::SpreadingFactor;

use futuresdr::blocks::FileSink;
use futuresdr::blocks::*;
use protocol_detector::{generate_zadoff_chu, Protocol, Sequence};
use rand_distr::{Distribution, Normal};

const PAD_FRONT: usize = 30;
const PAD_TAIL: usize = 30;
const INTERVAL: usize = 30000;

#[derive(Parser, Debug)]
#[clap(version)]
struct Args {
    #[clap(long, default_value_t = 0.001)]
    tx_interval: f32,
    #[clap(long, value_enum, default_value_t = SpreadingFactor::SF7)]
    spreading_factor: SpreadingFactor,
    #[clap(long, value_enum, default_value_t = Bandwidth::BW125)]
    bandwidth: Bandwidth,
    #[clap(long, default_value_t = 1)]
    oversampling: usize,
    #[clap(long, default_value_t = 0x0816)]
    sync_word: usize,
    #[clap(long, default_value_t = false)]
    soft_decoding: bool,
    #[clap(long, value_enum, default_value_t = CodeRate::CR_4_5)]
    code_rate: CodeRate,
    #[clap(long, default_value_t = 64)]
    sequence_length: u32,
    #[clap(long, default_value = "11")]
    sync_root: u32,
    #[clap(long, default_value = "17")]
    wifi_root: u32,
    #[clap(long, default_value = "23")]
    lora_root: u32,
    #[clap(long, default_value = "25")]
    zigbee_root: u32,
    #[clap(long, default_value = "30")]
    snr_db: f32,
}

fn main() -> Result<()> {
    let args = Args::parse();

    let mut fg = Flowgraph::new();

    let sync_sequence = generate_zadoff_chu(args.sync_root, args.sequence_length, 0, 0);
    let wifi_sequence = generate_zadoff_chu(args.wifi_root, args.sequence_length, 0, 0);
    let lora_sequence = generate_zadoff_chu(args.lora_root, args.sequence_length, 0, 0);
    let zigbee_sequence = generate_zadoff_chu(args.zigbee_root, args.sequence_length, 0, 0);

    let pad = vec![Complex32::new(0.0, 0.0); PAD_FRONT];

    let wifi_protocol = [
        pad.clone(),
        sync_sequence.clone(),
        wifi_sequence.clone(),
        pad.clone(),
    ]
    .concat();
    let lora_protocol = [
        pad.clone(),
        sync_sequence.clone(),
        lora_sequence.clone(),
        pad.clone(),
    ]
    .concat();
    let zigbee_protocol = [
        pad.clone(),
        sync_sequence.clone(),
        zigbee_sequence.clone(),
        pad.clone(),
    ]
    .concat();

    let protocols = vec![wifi_protocol, lora_protocol, zigbee_protocol];
    let signal_power: f32 =
        protocols[0].iter().map(|x| x.norm_sqr()).sum::<f32>() / protocols[0].len() as f32;

    let mut sample_counter = 0;
    let mut protocol_index = 0;
    let mut is_inserting_protocol = false;
    let mut protocol_sample_index = 0;

    let normal = Normal::new(0.0f32, 1.0).unwrap();

    let mut random_samples_left = 0;
    let mut protocol_inserted = true;

    let source = move || {
        if protocol_inserted && random_samples_left == 0 {
            random_samples_left = INTERVAL;
            protocol_inserted = false;
            protocol_sample_index = 0;
        }

        let sample =
            if !protocol_inserted && protocol_sample_index < protocols[protocol_index].len() {
                let sample = protocols[protocol_index][protocol_sample_index];
                protocol_sample_index += 1;

                if protocol_sample_index >= protocols[protocol_index].len() {
                    protocol_inserted = true;
                    protocol_index = (protocol_index + 1) % 3;
                }

                sample
            } else {
                random_samples_left -= 1;
                Complex32::new(
                    normal.sample(&mut rand::thread_rng()),
                    normal.sample(&mut rand::thread_rng()),
                )
            };

        sample
    };

    let src = fg.add_block(Source::new(source));

    let snr_linear = 10.0_f32.powf(args.snr_db / 10.0);
    let noise_power = signal_power / snr_linear;
    let noise_std_dev = noise_power.sqrt() / 2.0_f32.sqrt();

    let normal_noise = Normal::new(0.0f32, noise_std_dev).unwrap();
    let noise = fg.add_block(Apply::new(move |i: &Complex32| -> Complex32 {
        let re = normal_noise.sample(&mut rand::thread_rng());
        let imag = normal_noise.sample(&mut rand::thread_rng());
        i + Complex32::new(re, imag)
    }));

    let head = fg.add_block(Head::<Complex32>::new(1000000));
    let file_sink = fg.add_block(FileSink::<Complex32>::new("output_after_noise.bin"));

    fg.connect_stream(src, "out", noise, "in")?;
    fg.connect_stream(noise, "out", head, "in")?;
    fg.connect_stream(head, "out", file_sink, "in")?;

    let runtime = Runtime::new();
    runtime.run(fg)?;

    Ok(())
}
