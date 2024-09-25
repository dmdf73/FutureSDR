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
use protocol_detector::create_zigbee_receiver;
use protocol_detector::create_zigbee_transmitter;

const PAD_FRONT: usize = 100;
const PAD_TAIL: usize = 100;

#[derive(Parser, Debug)]
#[clap(version)]
struct Args {
    #[clap(long, default_value_t = 0.1)]
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
    let zigbee_sequence = generate_zadoff_chu(25, 64, 0);

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

    let zigbee_protocol = Protocol {
        name: "zigbee".to_string(),
        sequence: Sequence::new([sync_sequence.clone(), zigbee_sequence.clone()].concat(), 0.7),
        sequences: vec![Sequence::new(zigbee_sequence.clone(), 0.7)],
    };

    let (wifi_tx, wifi_tx_output, mut fg) = create_wifi_transmitter(fg)?;
    let (zigbee_tx, zigbee_tx_output, mut fg) = create_zigbee_transmitter(fg)?;
    let (mut fg, lora_tx, lora_circular_buffer, lora_tx_port) = create_lora_transmitter(
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
    let wifi_combined = [sync_sequence.clone(), wifi_sequence.clone()].concat();
    let lora_combined = [sync_sequence.clone(), lora_sequence.clone()].concat();
    let zigbee_combined = [sync_sequence.clone(), zigbee_sequence.clone()].concat();
    let inserter = MultiPortInserter::new(ports, vec![wifi_combined, lora_combined, zigbee_combined], 30, 30);
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
        vec![wifi_protocol, lora_protocol, zigbee_protocol],
        true,
        Some("matches.log".to_owned()),
    );
    let detector_block = fg.add_block(detector);
    println!("Added block with ID: {}", detector_block);
    fg.connect_stream(noise, "out", detector_block, "in")?;
    let pass = fg.add_block(Apply::new(|i: &Complex32| i * 1.));
    println!("Added block pass with ID: {}", pass);
    fg.connect_stream(detector_block, "lora", pass, "in")?;

    let (wifi_rx, mut fg) = create_wifi_receiver(fg, detector_block)?;
    let (zigbee_rx, mut fg) = create_zigbee_receiver(fg, detector_block)?;
    let (lora_rx, mut fg) = create_lora_receiver(
        fg,
        pass,
        args.bandwidth,
        args.spreading_factor,
        args.oversampling,
        args.sync_word,
        args.soft_decoding,
    )?;

    let rt = Runtime::new();
    let (wifi_tx_frame, wifi_rx_frame) = mpsc::channel::<Pmt>(100);
    let message_pipe = fg.add_block(MessagePipe::new(wifi_tx_frame));
    fg.connect_message(wifi_rx, "rx_frames", message_pipe, "in")?;

    let (lora_tx_frame, lora_rx_frame) = mpsc::channel::<Pmt>(100);
    let message_pipe = fg.add_block(MessagePipe::new(lora_tx_frame));
    fg.connect_message(lora_rx, "out", message_pipe, "in")?;

    let (zigbee_tx_frame, zigbee_rx_frame) = mpsc::channel::<Pmt>(100);
    let message_pipe = fg.add_block(MessagePipe::new(zigbee_tx_frame));
    fg.connect_message(zigbee_rx, "out", message_pipe, "in")?;


    let (_fg, mut handle) = rt.start_sync(fg);
    rt.spawn_background(async move {
        let mut counter: usize = 0;
        loop {
            let mut wifi_payload = format!("Wifi");
            if counter % 3 == 0 {
                handle
                    .call(
                        wifi_tx,
                        0,
                        Pmt::Any(Box::new((wifi_payload.as_bytes().to_vec(), Mcs::Qam16_1_2))),
                    )
                    .await
                    .unwrap();
            } else if counter % 3 == 1 {
                handle
                    .call(lora_tx, 0, Pmt::String(wifi_payload))
                    .await
                    .unwrap();
            } else {
                handle
                    .call(zigbee_tx, "tx", Pmt::Blob(wifi_payload.as_bytes().to_vec()))
                    .await
                    .unwrap();
            }

            counter += 1;
            counter %= 100;
            Timer::after(Duration::from_secs_f32(args.tx_interval)).await;
        }
    });

    rt.spawn_background(async move {
        let mut wifi_rx_frame = wifi_rx_frame;
        while let Some(x) = wifi_rx_frame.next().await {
            match x {
                Pmt::Blob(data) => {
                    println!("Received wifi frame length: {:?} bytes", data.len());
                }
                _ => panic!("wrong pmt"),
            }
        }
    });

    rt.spawn_background(async move {
        let mut lora_rx_frame = lora_rx_frame;
        while let Some(x) = lora_rx_frame.next().await {
            match x {
                Pmt::Blob(data) => {
                    println!("Received lora frame length: {:?} bytes", data.len());
                }
                _ => panic!("wrong pmt"),
            }
        }
    });

    rt.block_on(async move {
        let mut zigbee_rx_frame = zigbee_rx_frame;
        while let Some(x) = zigbee_rx_frame.next().await {
            match x {
                Pmt::Blob(data) => {
                    println!("Received zigbee frame length: {:?} bytes", data.len());
                }
                _ => panic!("wrong pmt"),
            }
        }
    });


    Ok(())
}
