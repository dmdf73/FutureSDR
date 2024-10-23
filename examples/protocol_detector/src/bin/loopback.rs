use futuresdr::anyhow::Result;
use futuresdr::async_io::Timer;
use futuresdr::blocks::*;
use futuresdr::futures::channel::mpsc;
use futuresdr::futures::StreamExt;
use futuresdr::num_complex::Complex32;
use futuresdr::runtime::buffer::circular::Circular;
use futuresdr::runtime::Flowgraph;
use futuresdr::runtime::Pmt;
use futuresdr::runtime::Runtime;
use protocol_detector::sequences::sequences;
use protocol_detector::{MultiPortInserter, Protocol, ProtocolDetector, Sequence};
use rand_distr::{Distribution, Normal};
use std::time::Duration;
use wlan::*;

const PAD_FRONT: usize = 10000;
const PAD_TAIL: usize = 10000;
const WIFI_PREAMBLE: [Complex32; 63] = protocol_detector::sequences::ZC_63;

fn create_wifi_transmitter(mut fg: Flowgraph) -> Result<(usize, usize, Flowgraph)> {
    let mac = fg.add_block(Mac::new([0x42; 6], [0x23; 6], [0xff; 6]));
    let encoder = fg.add_block(Encoder::new(Mcs::Qpsk_1_2));
    let mapper = fg.add_block(Mapper::new());
    let mut fft = Fft::with_options(
        64,
        FftDirection::Inverse,
        true,
        Some((1.0f32 / 52.0).sqrt()),
    );
    fft.set_tag_propagation(Box::new(fft_tag_propagation));
    let fft = fg.add_block(fft);
    let prefix = fg.add_block(Prefix::new(PAD_FRONT, PAD_TAIL));

    fg.connect_message(mac, "tx", encoder, "tx")?;
    fg.connect_stream(encoder, "out", mapper, "in")?;
    fg.connect_stream(mapper, "out", fft, "in")?;
    fg.connect_stream_with_type(fft, "out", prefix, "in", Circular::with_size(4096))?;

    Ok((mac, prefix, fg))
}

fn create_wifi_receiver(mut fg: Flowgraph, src: usize) -> Result<(usize, Flowgraph)> {
    let delay = fg.add_block(Delay::<Complex32>::new(16));
    let complex_to_mag_2 = fg.add_block(Apply::new(|i: &Complex32| i.norm_sqr()));
    let float_avg = fg.add_block(MovingAverage::<f32>::new(64));
    let mult_conj = fg.add_block(Combine::new(|a: &Complex32, b: &Complex32| a * b.conj()));
    let complex_avg = fg.add_block(MovingAverage::<Complex32>::new(48));
    let divide_mag = fg.add_block(Combine::new(|a: &Complex32, b: &f32| a.norm() / b));
    let sync_short = fg.add_block(SyncShort::new());
    let sync_long = fg.add_block(SyncLong::new());
    let mut fft = Fft::new(64);
    fft.set_tag_propagation(Box::new(fft_tag_propagation));
    let fft = fg.add_block(fft);
    let frame_equalizer = fg.add_block(FrameEqualizer::new());
    let decoder = fg.add_block(Decoder::new());

    fg.connect_stream(src, "wifi", delay, "in")?;
    fg.connect_stream(src, "wifi", complex_to_mag_2, "in")?;
    fg.connect_stream(src, "wifi", mult_conj, "in0")?;
    fg.connect_stream(complex_to_mag_2, "out", float_avg, "in")?;
    fg.connect_stream(delay, "out", mult_conj, "in1")?;
    fg.connect_stream(mult_conj, "out", complex_avg, "in")?;
    fg.connect_stream(complex_avg, "out", divide_mag, "in0")?;
    fg.connect_stream(float_avg, "out", divide_mag, "in1")?;
    fg.connect_stream(delay, "out", sync_short, "in_sig")?;
    fg.connect_stream(complex_avg, "out", sync_short, "in_abs")?;
    fg.connect_stream(divide_mag, "out", sync_short, "in_cor")?;
    fg.connect_stream(sync_short, "out", sync_long, "in")?;
    fg.connect_stream(sync_long, "out", fft, "in")?;
    fg.connect_stream(fft, "out", frame_equalizer, "in")?;
    fg.connect_stream(frame_equalizer, "out", decoder, "in")?;

    Ok((decoder, fg))
}

fn main() -> Result<()> {
    let mut fg = Flowgraph::new();

    let wifi_protocol = Protocol {
        name: "wifi".to_string(),
        sequence: Sequence::new(WIFI_PREAMBLE.to_vec(), 0.75),
        sequences: vec![Sequence::new(WIFI_PREAMBLE.to_vec(), 0.75)],
    };

    let (tx_mac, tx_output, mut fg) = create_wifi_transmitter(fg)?;

    // Berechnung der Puffergröße
    let mut size = 4096;
    let prefix_out_size = loop {
        if size / 8 >= PAD_FRONT + std::cmp::max(PAD_TAIL, 1) + 320 + MAX_SYM * 80 {
            break size;
        }
        size += 4096
    };

    // 1. MultiPortInserter
    let ports = vec![("wifi".to_string(), "burst_start".to_string())];
    let inserter = MultiPortInserter::new(ports, vec![WIFI_PREAMBLE.to_vec()], 30, 30);
    let inserter_block = fg.add_block(inserter);
    fg.connect_stream_with_type(
        tx_output,
        "out",
        inserter_block,
        "wifi",
        Circular::with_size(prefix_out_size),
    )?;

    // 2. Noise
    let normal = Normal::new(0.0f32, 0.001).unwrap();
    let noise = fg.add_block(Apply::new(move |i: &Complex32| -> Complex32 {
        let re = normal.sample(&mut rand::thread_rng());
        let imag = normal.sample(&mut rand::thread_rng());
        i + Complex32::new(re, imag)
    }));
    fg.connect_stream(inserter_block, "out", noise, "in")?;

    // 3. ProtocolDetector
    let detector = ProtocolDetector::new(
        vec![wifi_protocol],
        None,
        true,
        Some("matches.log".to_owned()),
    );
    let detector_block = fg.add_block(detector);
    fg.connect_stream(noise, "out", detector_block, "in")?;

    let (rx_decoder, mut fg) = create_wifi_receiver(fg, detector_block)?;

    let (tx_frame, rx_frame) = mpsc::channel::<Pmt>(100);
    let message_pipe = fg.add_block(MessagePipe::new(tx_frame));
    fg.connect_message(rx_decoder, "rx_frames", message_pipe, "in")?;

    let rt = Runtime::new();
    let (fg, mut handle) = rt.start_sync(fg);

    rt.spawn_background(async move {
        let mut seq = 0u64;
        loop {
            Timer::after(Duration::from_secs_f32(0.8)).await;
            handle
                .call(
                    tx_mac,
                    0,
                    Pmt::Any(Box::new((
                        format!("FutureSDR {seq}").as_bytes().to_vec(),
                        Mcs::Qam16_1_2,
                    ))),
                )
                .await
                .unwrap();
            seq += 1;
        }
    });

    rt.block_on(async move {
        let mut rx_frame = rx_frame;
        while let Some(x) = rx_frame.next().await {
            match x {
                Pmt::Blob(data) => {
                    println!("Received frame length: {} bytes", data.len());
                }
                _ => break,
            }
        }
    });

    Ok(())
}
