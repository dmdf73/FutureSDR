pub mod ZadoffChu;
pub mod detector;
pub mod multi_port_inserter;
pub mod multi_protocol_inserter;
pub mod sequences;
pub mod simple_tag_inserter;
pub mod zc_detect;
pub mod zc_pad;

pub use detector::Protocol;
pub use detector::ProtocolDetector;
pub use detector::Sequence;
use futuresdr::anyhow::{anyhow, Result};
use futuresdr::macros::connect;
use futuresdr::num_complex::Complex32;
use futuresdr::runtime::buffer::circular::Circular;
pub use multi_port_inserter::MultiPortInserter;
pub use multi_protocol_inserter::MultiProtocolInserter;
pub use simple_tag_inserter::SimpleTagInserter;

use std::fs::File;
use std::io::BufRead as _;
use std::io::{BufReader, Read};

use futuresdr::blocks::*;
use futuresdr::runtime::Flowgraph;
use lora::utils::{Bandwidth, CodeRate, SpreadingFactor};
use lora::*;
use wlan::*;

pub fn load_complex32_from_file(filename: &str) -> Result<Vec<Complex32>> {
    let file = File::open(filename)?;
    let mut reader = BufReader::new(file);
    let mut buffer = [0u8; 8];
    let mut vec = Vec::new();

    while reader.read_exact(&mut buffer).is_ok() {
        let re = f32::from_le_bytes([buffer[0], buffer[1], buffer[2], buffer[3]]);
        let im = f32::from_le_bytes([buffer[4], buffer[5], buffer[6], buffer[7]]);
        vec.push(Complex32::new(re, im));
    }

    Ok(vec)
}

const PAD_FRONT: usize = 10000;
const PAD_TAIL: usize = 10000;
const HAS_CRC: bool = true;
const IMPLICIT_HEADER: bool = false;
const LOW_DATA_RATE: bool = false;
const PREAMBLE_LEN: usize = 8;
const PAD: usize = 10000;

pub fn create_lora_transmitter(
    mut fg: Flowgraph,
    code_rate: CodeRate,
    spreading_factor: SpreadingFactor,
    oversampling: usize,
    sync_word: usize,
) -> Result<(Flowgraph, usize, Circular, usize)> {
    let transmitter = Transmitter::new(
        code_rate.into(),
        HAS_CRC,
        spreading_factor.into(),
        LOW_DATA_RATE,
        IMPLICIT_HEADER,
        oversampling,
        vec![sync_word],
        PREAMBLE_LEN,
        PAD,
    );
    let fg_tx_port = transmitter.message_input_name_to_id("msg").ok_or(anyhow!(
        "Message handler `msg` of whitening block not found"
    ))?;
    let circular_buffer =
        Circular::with_size((1 << usize::from(spreading_factor)) * 3 * oversampling);
    let transmitter_id = fg.add_block(transmitter);
    println!("Added block with ID: {}", transmitter_id);
    Ok((fg, transmitter_id, circular_buffer, fg_tx_port))
}

pub fn create_lora_receiver(
    mut fg: Flowgraph,
    input: usize,
    bandwidth: Bandwidth,
    spreading_factor: SpreadingFactor,
    oversampling: usize,
    sync_word: usize,
    soft_decoding: bool,
) -> Result<Flowgraph> {
    let frame_sync = FrameSync::new(
        868_000_000,
        bandwidth.into(),
        spreading_factor.into(),
        IMPLICIT_HEADER,
        vec![vec![sync_word]],
        oversampling,
        None,
        None,
        false,
        None,
    );

    let fft_demod = FftDemod::new(soft_decoding, spreading_factor.into());
    let gray_mapping = GrayMapping::new(soft_decoding);
    let deinterleaver = Deinterleaver::new(soft_decoding);
    let hamming_dec = HammingDec::new(soft_decoding);
    let header_decoder = HeaderDecoder::new(HeaderMode::Explicit, false);
    let decoder = lora::Decoder::new();

    let udp_data = BlobToUdp::new("127.0.0.1:55555");
    let udp_rftap = BlobToUdp::new("127.0.0.1:55556");
    connect!(fg,
        input [Circular::with_size((1 << usize::from(spreading_factor)) * 3 * oversampling)] frame_sync > fft_demod > gray_mapping > deinterleaver > hamming_dec > header_decoder;
        header_decoder.frame_info | frame_sync.frame_info;
        header_decoder | decoder;
        decoder.crc_check | frame_sync.payload_crc_result;
        decoder.out | udp_data;
        decoder.rftap | udp_rftap;
    );

    Ok(fg)
}

pub fn create_wifi_transmitter(mut fg: Flowgraph) -> Result<(usize, usize, Flowgraph)> {
    let mac = fg.add_block(Mac::new([0x42; 6], [0x23; 6], [0xff; 6]));
    println!("Added block with ID: {}", mac);
    let encoder = fg.add_block(wlan::Encoder::new(Mcs::Qpsk_1_2));
    println!("Added block with ID: {}", encoder);
    let mapper = fg.add_block(Mapper::new());
    println!("Added block with ID: {}", mapper);
    let mut fft = Fft::with_options(
        64,
        FftDirection::Inverse,
        true,
        Some((1.0f32 / 52.0).sqrt()),
    );
    fft.set_tag_propagation(Box::new(fft_tag_propagation));
    let fft = fg.add_block(fft);
    println!("Added block with ID: {}", fft);
    let prefix = fg.add_block(Prefix::new(PAD_FRONT, PAD_TAIL));
    println!("Added block with ID: {}", prefix);

    fg.connect_message(mac, "tx", encoder, "tx")?;
    fg.connect_stream(encoder, "out", mapper, "in")?;
    fg.connect_stream(mapper, "out", fft, "in")?;
    fg.connect_stream_with_type(fft, "out", prefix, "in", Circular::with_size(4096))?;

    Ok((mac, prefix, fg))
}

pub fn create_wifi_receiver(mut fg: Flowgraph, src: usize) -> Result<(usize, Flowgraph)> {
    let delay = fg.add_block(Delay::<Complex32>::new(16));
    println!("Added block with ID: {}", delay);
    let complex_to_mag_2 = fg.add_block(Apply::new(|i: &Complex32| i.norm_sqr()));
    println!("Added block with ID: {}", complex_to_mag_2);
    let float_avg = fg.add_block(MovingAverage::<f32>::new(64));
    println!("Added block with ID: {}", float_avg);
    let mult_conj = fg.add_block(Combine::new(|a: &Complex32, b: &Complex32| a * b.conj()));
    println!("Added block with ID: {}", mult_conj);
    let complex_avg = fg.add_block(MovingAverage::<Complex32>::new(48));
    println!("Added block with ID: {}", complex_avg);
    let divide_mag = fg.add_block(Combine::new(|a: &Complex32, b: &f32| a.norm() / b));
    println!("Added block with ID: {}", divide_mag);
    let sync_short = fg.add_block(SyncShort::new());
    println!("Added block with ID: {}", sync_short);
    let sync_long = fg.add_block(SyncLong::new());
    println!("Added block with ID: {}", sync_long);
    let mut fft = Fft::new(64);
    fft.set_tag_propagation(Box::new(fft_tag_propagation));
    let fft = fg.add_block(fft);
    println!("Added block with ID: {}", fft);
    let frame_equalizer = fg.add_block(FrameEqualizer::new());
    println!("Added block fra with ID: {}", frame_equalizer);
    let decoder = fg.add_block(wlan::Decoder::new());
    println!("Added block dec with ID: {}", decoder);

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

mod protocol_detector_fft;
pub use protocol_detector_fft::ProtocolDetectorFFT;

fn normalized_dot_product(seq1: &[Complex32], seq2: &Sequence) -> f32 {
    assert_eq!(
        seq1.len(),
        seq2.data.len(),
        "Sequences must have the same length"
    );

    let norm_seq1 = seq1.iter().map(|x| x.norm_sqr()).sum::<f32>().sqrt();
    let norm_seq2 = seq2.norm();

    if norm_seq1 == 0.0 || norm_seq2 == 0.0 {
        return if norm_seq1 == norm_seq2 { 0.0 } else { 0.0 };
    }

    let sum_val: Complex32 = seq1
        .iter()
        .zip(seq2.data.iter())
        .map(|(a, b)| a * b.conj())
        .sum();

    let normalized = sum_val / (norm_seq1 * norm_seq2);
    normalized.re
}
pub fn load_log_file(filename: &str) -> Result<Vec<String>> {
    let file = File::open(filename)?;
    let reader = BufReader::new(file);
    let lines: Result<Vec<String>, _> = reader.lines().collect();
    Ok(lines?)
}
const PI: f32 = std::f32::consts::PI;

pub fn generate_zadoff_chu(u: u32, n: u32, q: u32) -> Vec<Complex32> {
    if u == 0 || u >= n {
        panic!("u must be in the range 1 <= u < n");
    }
    if gcd(u, n) != 1 {
        panic!("u and n must be coprime");
    }

    let cf = n % 2;
    (0..n)
        .map(|k| {
            let k = k as f32;
            let n = n as f32;
            let u = u as f32;
            let q = q as f32;
            let cf = cf as f32;
            let exponent = -PI * u * k * (k + cf + 2.0 * q) / n;
            Complex32::new(0.0, exponent).exp()
        })
        .collect()
}

fn gcd(a: u32, b: u32) -> u32 {
    if b == 0 {
        a
    } else {
        gcd(b, a % b)
    }
}
