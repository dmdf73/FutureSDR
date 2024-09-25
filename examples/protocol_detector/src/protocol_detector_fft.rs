use crate::{normalized_dot_product, Protocol, Sequence};
use futuresdr::anyhow::{anyhow, Result};
use futuresdr::macros::async_trait;
use futuresdr::num_complex::Complex32;
use futuresdr::runtime::{
    Block, BlockMeta, BlockMetaBuilder, Kernel, MessageIo, MessageIoBuilder, StreamIo,
    StreamIoBuilder, Tag, WorkIo,
};
use rustfft::Fft;
use rustfft::{num_complex::Complex, FftPlanner};
use std::collections::VecDeque;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

struct Sequence_fft {
    sequence: Sequence,
    fft: Vec<Complex<f32>>,
}

impl Sequence_fft {
    fn new(sequence: Sequence) -> Self {
        let n = sequence.data.len() * 2;
        let mut planner = FftPlanner::new();
        let fft = planner.plan_fft_forward(n);

        let mut fft_data: Vec<Complex<f32>> = sequence
            .data
            .iter()
            .map(|&c| Complex::new(c.re, c.im))
            .chain(std::iter::repeat(Complex::new(0.0, 0.0)).take(n - sequence.data.len()))
            .collect();

        fft.process(&mut fft_data);

        Sequence_fft {
            sequence,
            fft: fft_data,
        }
    }
}

pub struct ProtocolDetectorFFT {
    sync_sequence: Sequence_fft,
    protocols: Vec<Protocol>,
    sequence_lengths: Vec<usize>,
    total_protocol_length: usize,
    current_protocol: Option<usize>,
    absolute_index: usize,
    match_log_file: Option<std::fs::File>,
    debug_enabled: bool,
    norm_queue: VecDeque<f32>,
    running_sum: f32,
    total_processing_time: Duration,
    time_log_file: PathBuf,
    fft_planner: FftPlanner<f32>,
    fft_plan: Arc<dyn Fft<f32>>,
    ifft_plan: Arc<dyn Fft<f32>>,
}

impl ProtocolDetectorFFT {
    pub fn new(
        sync_sequence: Sequence,
        protocols: Vec<Protocol>,
        debug_enabled: bool,
        log_file: Option<String>,
    ) -> Block {
        Self::validate_protocols(&protocols).expect("Invalid protocols");
        let sequence_lengths: Vec<usize> = protocols[0]
            .sequences
            .iter()
            .map(|seq| seq.data.len())
            .collect();
        let total_protocol_length: usize = sequence_lengths.iter().sum();

        let mut sio = StreamIoBuilder::new().add_input::<Complex32>("in");

        for protocol in &protocols {
            sio = sio.add_output::<Complex32>(&protocol.name);
        }

        let match_log_file =
            log_file.map(|f| std::fs::File::create(f).expect("Failed to create log file"));

        let time_log_file = PathBuf::from("time.txt");

        let mut fft_planner = FftPlanner::new();
        let n = sync_sequence.data.len() * 2;
        let fft_plan = fft_planner.plan_fft_forward(n);
        let ifft_plan = fft_planner.plan_fft_inverse(n);

        let detector = ProtocolDetectorFFT {
            sync_sequence: Sequence_fft::new(sync_sequence),
            protocols,
            sequence_lengths,
            total_protocol_length,
            current_protocol: Some(0),
            absolute_index: 0,
            match_log_file,
            debug_enabled,
            norm_queue: VecDeque::new(),
            running_sum: 0.0,
            total_processing_time: Duration::new(0, 0),
            time_log_file,
            fft_planner,
            fft_plan,
            ifft_plan,
        };

        Block::new(
            BlockMetaBuilder::new("ProtocolDetectorFFT").build(),
            sio.build(),
            MessageIoBuilder::new().build(),
            detector,
        )
    }

    fn validate_protocols(protocols: &[Protocol]) -> Result<()> {
        if protocols.is_empty() {
            return Err(anyhow!("No protocols provided"));
        }
        let first_protocol = &protocols[0];
        if first_protocol.sequences[0].data.len() % 2 != 0 {
            return Err(anyhow!("Invalid protocol structure"));
        }
        let reference_length = first_protocol.sequences[0].data.len();
        for protocol in protocols.iter().skip(1) {
            if protocol.sequences[0].data.len() != reference_length {
                return Err(anyhow!(
                    "Protocols must have sequences of equal, even length"
                ));
            }
        }
        Ok(())
    }

    fn update_norm_vector(&mut self, window: &[Complex32], sequence_length: usize) -> Vec<f32> {
        let mut norms = Vec::with_capacity(sequence_length);
        let mut start_index = 1;

        if self.norm_queue.is_empty() {
            let initial_segment = &window[..sequence_length];
            self.running_sum = 0.0;
            for &value in initial_segment {
                let norm = value.norm_sqr();
                self.norm_queue.push_back(norm);
                self.running_sum += norm;
            }
            norms.push(self.running_sum);
        } else {
            start_index = 0;
        }

        for i in start_index..(window.len() - sequence_length) {
            let new_val = window[i + sequence_length - 1].norm_sqr();
            if let Some(old_val) = self.norm_queue.pop_front() {
                self.running_sum = self.running_sum - old_val + new_val;
            }
            self.norm_queue.push_back(new_val);
            norms.push(self.running_sum.sqrt());
        }

        norms
        // .iter().map(|&sum| sum.sqrt()).collect()
    }

    fn fft_corr_normalized(&mut self, window: &[Complex32], norm_vector: &[f32]) -> Option<usize> {
        let n = window.len();
        let mut window_fft: Vec<Complex<f32>> =
            window.iter().map(|&c| Complex::new(c.re, c.im)).collect();
        self.fft_plan.process(&mut window_fft);
        for (w, s) in window_fft.iter_mut().zip(self.sync_sequence.fft.iter()) {
            *w *= s.conj();
        }
        let start_time = Instant::now();
        self.ifft_plan.process(&mut window_fft);
        self.total_processing_time += start_time.elapsed();
        let sequence_norm = self.sync_sequence.sequence.norm();
        let norm_factor = n as f32; // Skalierungsfaktor als float speichern

        for (i, &x) in window_fft
            .iter()
            .take(self.sync_sequence.sequence.data.len())
            .enumerate()
        {
            let normalized_corr = if norm_vector[i] == 0.0 {
                0.0
            } else {
                (x.re / (norm_vector[i] * sequence_norm * norm_factor))
            };
            if normalized_corr >= self.sync_sequence.sequence.threshold {
                // self.total_processing_time += start_time.elapsed();
                return Some(i);
            }
        }
        // self.total_processing_time += start_time.elapsed();
        None
    }

    fn match_protocols(
        &self,
        input: &[Complex32],
        start_index: usize,
        protocols: &[Protocol],
    ) -> Option<usize> {
        let sequence_length = protocols[0].sequences[0].data.len();
        if start_index + sequence_length > input.len() {
            panic!("Start index plus sequence length exceeds the length of the input");
        }
        for (protocol_index, protocol) in protocols.iter().enumerate() {
            let sequence = &protocol.sequences[0];
            let correlation = normalized_dot_product(
                &input[start_index..start_index + sequence_length],
                sequence,
            );
            self.debug_echo(&format!(
                "DEBUG: Protocol: {}, Correlation: {:.4}, Threshold: {:.4}",
                protocol.name, correlation, sequence.threshold
            ));

            if correlation >= sequence.threshold {
                self.debug_echo(&format!(
                    "DEBUG: Protocol: {}, Correlation: {:.4}, Threshold: {:.4}",
                    protocol.name, correlation, sequence.threshold
                ));

                return Some(protocol_index);
            }
        }

        None
    }

    fn debug_echo(&self, message: &str) {
        if self.debug_enabled {
            // println!("{}", message);
        }
    }

    fn log_match(&mut self, index: usize, protocol_name: &str) -> Result<()> {
        if let Some(file) = &mut self.match_log_file {
            writeln!(file, "{},{}", self.absolute_index + index, protocol_name)?;
        }
        Ok(())
    }

    fn log_total_processing_time(&self) -> Result<()> {
        let mut file = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .open(&self.time_log_file)?;
        writeln!(
            file,
            "Gesamtverarbeitungszeit: {:?}",
            self.total_processing_time
        )?;
        Ok(())
    }
}

#[async_trait]
impl Kernel for ProtocolDetectorFFT {
    async fn work(
        &mut self,
        io: &mut WorkIo,
        sio: &mut StreamIo,
        _m: &mut MessageIo<Self>,
        _b: &mut BlockMeta,
    ) -> Result<()> {
        // let start_time = Instant::now();
        let input = sio.input(0).slice::<Complex32>();
        let mut output_slices: Vec<&mut [Complex32]> = Vec::new();
        for i in 0..self.protocols.len() {
            output_slices.push(sio.output(i).slice::<Complex32>());
        }
        let min_output_len = output_slices
            .iter()
            .map(|slice| slice.len())
            .min()
            .unwrap_or(0);
        let window_length = 2 * self.sequence_lengths[0];
        let max_process = std::cmp::min(
            input
                .len()
                .saturating_sub((1.5 * window_length as f32).ceil() as usize - 1),
            min_output_len.saturating_sub(window_length - 1),
        )
        .saturating_sub(1);
        let mut sync_hits = Vec::new();
        let mut sequence_hits = Vec::new();
        let step_size = window_length / 2;
        let mut i = 0;
        let mut call_counter = 0;
        let mut current_protocol = self.current_protocol.unwrap_or(0);
        while i < max_process {
            call_counter += 1;

            let window = &input[i..i + window_length];
            let norm_vector: Vec<f32> =
                self.update_norm_vector(window, self.sync_sequence.sequence.data.len());
            if let Some(hit) = self.fft_corr_normalized(window, &norm_vector) {
                sync_hits.push(i + hit);
                let seq_start = i + hit + self.sequence_lengths[0];
                if let Some(detected_seq) =
                    self.match_protocols(&input[seq_start..], 0, &self.protocols)
                {
                    current_protocol = detected_seq;
                    sequence_hits.push((i + hit, current_protocol));
                    self.log_match(i + hit, &self.protocols[detected_seq].name.to_string())?;
                }
            }
            i += step_size;
        }
        sequence_hits.insert(0, (0, self.current_protocol.unwrap_or(0)));
        self.current_protocol = Some(current_protocol);
        let mut calculated_value = if i > 0 { i } else { 0 };
        sequence_hits.push((calculated_value, current_protocol));
        let mut output_status = vec![0; self.protocols.len()];
        let mut consumed = 0;
        if calculated_value > 0 {
            for (idx, window) in sequence_hits.windows(2).enumerate() {
                let (current_hit, current_protocol) = window[0];
                let (next_hit, _) = window[1];
                let output_len = next_hit - current_hit;
                let output: &mut &mut [Complex<f32>] = &mut output_slices[current_protocol];
                let start_idx = output_status[current_protocol];
                output[start_idx..start_idx + output_len]
                    .copy_from_slice(&input[current_hit..next_hit]);
                output_status[current_protocol] += output_len;
                consumed += output_len;
            }

            sio.input(0).consume(consumed);
            for (protocol_index, &produced) in output_status.iter().enumerate() {
                sio.output(protocol_index).produce(produced);
            }
        }
        if sio.input(0).finished() {
            let input = sio.input(0).slice::<Complex32>();
            if input.len() == 0 {
                io.finished = true;
                if self.debug_enabled {
                    if let Err(e) = self.log_total_processing_time() {
                        eprintln!("Fehler beim Loggen der Gesamtverarbeitungszeit: {}", e);
                    }
                }
            } else {
                let current_protocol = self.current_protocol.unwrap_or(0);
                let mut current_output = sio.output(current_protocol).slice::<Complex32>();
                if current_output.len() >= 1 {
                    current_output[0] = input[0];
                    sio.input(0).consume(1);
                    sio.output(current_protocol).produce(1);
                }
            }
        }
        self.current_protocol = Some(current_protocol);

        self.absolute_index += consumed;

        // self.total_processing_time += start_time.elapsed();
        Ok(())
    }
}
