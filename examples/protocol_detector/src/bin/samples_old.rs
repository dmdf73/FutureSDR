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
use protocol_detector::{generate_zadoff_chu, MultiPortInserter, Protocol, Sequence};
use rand_distr::{Distribution, Normal};
use wlan::*;

use protocol_detector::create_lora_transmitter;
use protocol_detector::create_wifi_transmitter;
use protocol_detector::create_zigbee_transmitter;

const PAD_FRONT: usize = 1000;
const PAD_TAIL: usize = 1000;

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

    let (wifi_tx, wifi_tx_output, fg) = create_wifi_transmitter(fg)?;
    let (zigbee_tx, zigbee_tx_output, fg) = create_zigbee_transmitter(fg)?;
    let (mut fg, lora_tx, _lora_circular_buffer, _lora_tx_port) = create_lora_transmitter(
        fg,
        args.code_rate,
        args.spreading_factor,
        args.oversampling,
        args.sync_word,
    )?;

    let ports = vec![
        ("wifi".to_string(), "burst_start".to_string()),
        ("lora".to_string(), "burst_start".to_string()),
        ("zigbee".to_string(), "burst_start".to_string()),
    ];
    let wifi_combined = [sync_sequence.clone(), wifi_sequence].concat();
    let lora_combined = [sync_sequence.clone(), lora_sequence].concat();
    let zigbee_combined = [sync_sequence.clone(), zigbee_sequence].concat();
    let inserter = MultiPortInserter::new(
        ports,
        vec![wifi_combined.clone(), lora_combined, zigbee_combined],
        30,
        30,
    );
    let inserter_block = fg.add_block(inserter);
    println!("Added inserter block with ID: {}", inserter_block);

    let mut size = 4096;
    let prefix_out_size = loop {
        if size / 8 >= PAD_FRONT + std::cmp::max(PAD_TAIL, 1) + 320 + MAX_SYM * 80 {
            break size;
        }
        size += 4096
    };

    fg.connect_stream_with_type(
        wifi_tx_output,
        "out",
        inserter_block,
        "wifi",
        Circular::with_size(prefix_out_size),
    )?;
    fg.connect_stream_with_type(
        lora_tx,
        "out",
        inserter_block,
        "lora",
        Circular::with_size(prefix_out_size),
    )?;
    fg.connect_stream_with_type(
        zigbee_tx_output,
        "out",
        inserter_block,
        "zigbee",
        Circular::with_size(prefix_out_size),
    )?;

    let signal_power: f32 =
        wifi_combined.iter().map(|&x| x.norm_sqr()).sum::<f32>() / wifi_combined.len() as f32;

    let snr_linear = 10.0_f32.powf(args.snr_db / 10.0);
    let noise_power = signal_power / snr_linear;
    let noise_std_dev = noise_power.sqrt() / 2.0_f32.sqrt();

    let normal = Normal::new(0.0f32, noise_std_dev).unwrap();
    let noise = fg.add_block(Apply::new(move |i: &Complex32| -> Complex32 {
        let re = normal.sample(&mut rand::thread_rng());
        let imag = normal.sample(&mut rand::thread_rng());
        i + Complex32::new(re, imag)
    }));
    println!("Added noise block with ID: {}", noise);
    fg.connect_stream(inserter_block, "out", noise, "in")?;

    let head = fg.add_block(Head::<Complex32>::new(1000000));
    println!("Added head block with ID: {}", head);
    fg.connect_stream(noise, "out", head, "in")?;

    let file_sink = fg.add_block(FileSink::<Complex32>::new("output_after_noise.bin"));
    println!("Added file sink block with ID: {}", file_sink);
    fg.connect_stream(head, "out", file_sink, "in")?;

    let rt = Runtime::new();

    let (fg, mut handle) = rt.start_sync(fg);
    rt.block_on(async move {
        let mut counter: usize = 0;
        loop {
            let wifi_payload = format!("Wifi");
            let result = if counter % 3 == 0 {
                handle
                    .call(
                        wifi_tx,
                        0,
                        Pmt::Any(Box::new((wifi_payload.as_bytes().to_vec(), Mcs::Qam16_1_2))),
                    )
                    .await
            } else if counter % 3 == 1 {
                handle.call(lora_tx, 0, Pmt::String(wifi_payload)).await
            } else {
                handle
                    .call(zigbee_tx, "tx", Pmt::Blob(wifi_payload.as_bytes().to_vec()))
                    .await
            };

            if let Err(_) = result {
                break;
            }

            counter += 1;
            counter %= 100;
            Timer::after(Duration::from_secs_f32(args.tx_interval)).await;
        }
    });
    Ok(())
}
