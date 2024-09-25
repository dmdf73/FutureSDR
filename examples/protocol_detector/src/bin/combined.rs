use clap::Parser;
use lora::utilities::CodeRate;
use std::time::Duration;

use futuresdr::anyhow::Result;
use futuresdr::async_io::Timer;
use futuresdr::num_complex::Complex32;
use futuresdr::runtime::buffer::circular::Circular;
use futuresdr::runtime::Flowgraph;
use futuresdr::runtime::Pmt;
use futuresdr::runtime::Runtime;
use lora::utilities::Bandwidth;
use lora::utilities::SpreadingFactor;

use futuresdr::blocks::*;
use futuresdr::futures::channel::mpsc;
use futuresdr::futures::StreamExt;
use protocol_detector::{
    generate_zadoff_chu, MultiPortInserter, Protocol, ProtocolDetectorFFT, Sequence,
    SimpleTagInserter,
};
use rand_distr::{Distribution, Normal};
use wlan::*;

use protocol_detector::create_lora_receiver;
use protocol_detector::create_lora_transmitter;
use protocol_detector::create_wifi_receiver;
use protocol_detector::create_wifi_transmitter;

const PAD_FRONT: usize = 100;
const PAD_TAIL: usize = 100;

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
}

fn main() -> Result<()> {
    let args = Args::parse();

    let fg = Flowgraph::new();

    let sync_sequence = generate_zadoff_chu(11, 64, 0);
    let wifi_sequence = generate_zadoff_chu(17, 64, 0);
    let lora_sequence = generate_zadoff_chu(23, 64, 0);

    let wifi_protocol = Protocol {
        name: "wifi".to_string(),
        sequence: Sequence::new(
            [sync_sequence.clone(), wifi_sequence.clone()].concat(),
            0.75,
        ),
        sequences: vec![Sequence::new(wifi_sequence.clone(), 0.75)],
    };

    let lora_protocol = Protocol {
        name: "lora".to_string(),
        sequence: Sequence::new([sync_sequence.clone(), lora_sequence.clone()].concat(), 0.7),
        sequences: vec![Sequence::new(lora_sequence.clone(), 0.7)],
    };

    let (wifi_tx_mac, wifi_tx_output, mut fg) = create_wifi_transmitter(fg)?;
    let (mut fg, lora_tx, lora_circular_buffer, lora_tx_port) = create_lora_transmitter(
        fg,
        args.code_rate,
        args.spreading_factor,
        args.oversampling,
        args.sync_word,
    )?;

    let tag_inserter = SimpleTagInserter::new(20000, vec!["burst_start".to_string()]);
    let tag_inserter_block = fg.add_block(tag_inserter);

    fg.connect_stream(lora_tx, "out", tag_inserter_block, "in")?;

    let ports = vec![
        ("wifi".to_string(), "burst_start".to_string()),
        ("lora".to_string(), "burst_start".to_string()),
    ];
    let wifi_combined = [sync_sequence.clone(), wifi_sequence.clone()].concat();
    let lora_combined = [sync_sequence.clone(), lora_sequence.clone()].concat();
    let inserter = MultiPortInserter::new(ports, vec![wifi_combined, lora_combined], 30, 30);
    let inserter_block = fg.add_block(inserter);
    println!("Added block with ID: {}", inserter_block);
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
        tag_inserter_block,
        "out",
        inserter_block,
        "lora",
        Circular::with_size(prefix_out_size),
    )?;

    let normal = Normal::new(0.0f32, 0.001).unwrap();
    let noise = fg.add_block(Apply::new(move |i: &Complex32| -> Complex32 {
        let re = normal.sample(&mut rand::thread_rng());
        let imag = normal.sample(&mut rand::thread_rng());
        i + Complex32::new(re, imag)
    }));
    println!("Added block with ID: {}", noise);
    fg.connect_stream(inserter_block, "out", noise, "in")?;

    let detector = ProtocolDetectorFFT::new(
        Sequence::new(sync_sequence.clone(), 0.75),
        vec![wifi_protocol, lora_protocol],
        true,
        Some("matches.log".to_owned()),
    );
    let detector_block = fg.add_block(detector);
    println!("Added block with ID: {}", detector_block);
    fg.connect_stream(noise, "out", detector_block, "in")?;
    let pass = fg.add_block(Apply::new(|i: &Complex32| i * 1.));
    println!("Added block pass with ID: {}", pass);
    fg.connect_stream(detector_block, "lora", pass, "in")?;

    let (wifi_rx_decoder, mut fg) = create_wifi_receiver(fg, detector_block)?;
    let mut fg = create_lora_receiver(
        fg,
        pass,
        args.bandwidth,
        args.spreading_factor,
        args.oversampling,
        args.sync_word,
        args.soft_decoding,
    )?;

    let rt = Runtime::new();
    let (tx_frame, rx_frame) = mpsc::channel::<Pmt>(100);
    let message_pipe = fg.add_block(MessagePipe::new(tx_frame));
    fg.connect_message(wifi_rx_decoder, "rx_frames", message_pipe, "in")?;

    let (_fg, mut handle) = rt.start_sync(fg);
    rt.spawn_background(async move {
        let mut counter: usize = 0;
        loop {
            let mut wifi_payload = format!("Wifi");
            if counter % 2 == 0 {
                handle
                    .call(
                        wifi_tx_mac,
                        0,
                        Pmt::Any(Box::new((wifi_payload.as_bytes().to_vec(), Mcs::Qam16_1_2))),
                    )
                    .await
                    .unwrap();
            } else {
                handle
                    .call(lora_tx, 0, Pmt::String(wifi_payload))
                    .await
                    .unwrap();
            }

            counter += 1;
            counter %= 100;
            Timer::after(Duration::from_secs_f32(args.tx_interval)).await;
        }
    });

    rt.block_on(async move {
        let mut rx_frame = rx_frame;
        while let Some(x) = rx_frame.next().await {
            match x {
                Pmt::Blob(data) => {
                    println!("Received frame length: {:?} bytes", data.len());
                }
                _ => break,
            }
        }
    });

    Ok(())
}
