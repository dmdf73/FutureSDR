use futuresdr::anyhow::{anyhow, Result};
use futuresdr::macros::async_trait;
use futuresdr::num_complex::Complex32;
use futuresdr::runtime::BlockMeta;
use futuresdr::runtime::BlockMetaBuilder;
use futuresdr::runtime::Kernel;
use futuresdr::runtime::MessageIo;
use futuresdr::runtime::MessageIoBuilder;
use futuresdr::runtime::StreamIo;
use futuresdr::runtime::Tag;
use futuresdr::runtime::WorkIo;
use futuresdr::runtime::{Block, StreamIoBuilder};
use std::fs::OpenOptions;
use std::io::Write;
use std::process::exit;

#[derive(Clone)]
pub struct Sequence {
    pub data: Vec<Complex32>,
    pub threshold: f32,
    norm: f32,
}

impl Sequence {
    pub fn new(data: Vec<Complex32>, threshold: f32) -> Self {
        let norm = data.iter().map(|x| x.norm_sqr()).sum::<f32>().sqrt();
        Sequence {
            data,
            threshold,
            norm,
        }
    }

    pub fn norm(&self) -> f32 {
        self.norm
    }
}

#[derive(Clone)]
pub struct Protocol {
    pub name: String,
    pub sequences: Vec<Sequence>,
    pub sequence: Sequence,
}

pub struct ProtocolDetector {
    protocols: Vec<Protocol>,
    sequence_lengths: Vec<usize>,
    total_protocol_length: usize,
    current_protocol: Option<usize>,
    current_index: usize,
    debug_enabled: bool,
    debug_log_file: Option<String>,
}

impl ProtocolDetector {
    pub fn new(
        protocols: Vec<Protocol>,
        debug_enabled: bool,
        debug_log_file: Option<String>,
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

        let detector = ProtocolDetector {
            protocols,
            sequence_lengths,
            total_protocol_length,
            current_protocol: Some(0),
            current_index: 0,
            debug_enabled,
            debug_log_file,
        };

        if let Err(e) = detector.initialize_log_file() {
            eprintln!("Failed to initialize log file: {}", e);
        }

        Block::new(
            BlockMetaBuilder::new("ProtocolDetector").build(),
            sio.build(),
            MessageIoBuilder::new().build(),
            detector,
        )
    }

    fn validate_protocols(protocols: &[Protocol]) -> Result<()> {
        if protocols.is_empty() {
            return Err(anyhow!("No protocols provided"));
        }
        if protocols.len() == 1 {
            return Ok(());
        }

        let reference_protocol = &protocols[0];
        let reference_sequence_count = reference_protocol.sequences.len();
        let reference_sequence_lengths: Vec<usize> = reference_protocol
            .sequences
            .iter()
            .map(|seq| seq.data.len())
            .collect();

        for protocol in &protocols[1..] {
            if protocol.sequences.len() != reference_sequence_count {
                return Err(anyhow!("Protocols have different numbers of sequences"));
            }

            for (i, sequence) in protocol.sequences.iter().enumerate() {
                if sequence.data.len() != reference_sequence_lengths[i] {
                    return Err(anyhow!("Sequence lengths do not match across protocols"));
                }
            }
        }

        Ok(())
    }

    fn match_protocol(
        &self,
        input: &[Complex32],
        start_index: usize,
        protocol: &Protocol,
    ) -> (bool, Vec<f32>) {
        let mut current_offset = 0;
        let mut correlations = Vec::new();

        for (seq_index, sequence) in protocol.sequences.iter().enumerate() {
            let sequence_length = self.sequence_lengths[seq_index];
            if start_index + current_offset + sequence_length > input.len() {
                return (false, correlations);
            }

            let correlation = normalized_dot_product(
                &input
                    [start_index + current_offset..start_index + current_offset + sequence_length],
                sequence,
            );

            correlations.push(correlation);
            self.debug_echo(&format!(
                "DEBUG: Protocol: {}, Sequence {}, Correlation: {:.4}, Threshold: {:.4}",
                protocol.name, seq_index, correlation, sequence.threshold
            ));

            if correlation < sequence.threshold {
                return (false, correlations);
            }

            current_offset += sequence_length;
        }

        (true, correlations)
    }

    fn debug_echo(&self, message: &str) {
        if self.debug_enabled {
            //println!("{}", message);
        }
    }

    fn initialize_log_file(&self) -> Result<()> {
        if self.debug_enabled {
            if let Some(log_file) = &self.debug_log_file {
                if std::fs::metadata(log_file).is_ok() {
                    std::fs::remove_file(log_file)?;
                }
                let file = OpenOptions::new().create(true).write(true).open(log_file)?;
                file.set_len(0)?;
            }
        }
        Ok(())
    }

    fn log_sequence_match(&self, protocol_name: &str) -> Result<()> {
        if self.debug_enabled {
            if let Some(log_file) = &self.debug_log_file {
                let mut file = OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(log_file)?;
                writeln!(file, "{},{}", self.current_index, protocol_name)?;
            }
        }
        Ok(())
    }
}

fn normalized_dot_product(seq1: &[Complex32], seq2: &Sequence) -> f32 {
    assert_eq!(
        seq1.len(),
        seq2.data.len(),
        "Sequences must have the same length"
    );

    let norm_seq1 = seq1.iter().map(|x| x.norm_sqr()).sum::<f32>().sqrt();
    let norm_seq2 = seq2.norm();

    if norm_seq1 == 0.0 || norm_seq2 == 0.0 {
        return if norm_seq1 == norm_seq2 { 1.0 } else { 0.0 };
    }

    let sum_val: Complex32 = seq1
        .iter()
        .zip(seq2.data.iter())
        .map(|(a, b)| a * b.conj())
        .sum();

    let normalized = sum_val / (norm_seq1 * norm_seq2);
    normalized.re
}

#[async_trait]
impl Kernel for ProtocolDetector {
    async fn work(
        &mut self,
        io: &mut WorkIo,
        sio: &mut StreamIo,
        _m: &mut MessageIo<Self>,
        _b: &mut BlockMeta,
    ) -> Result<()> {
        let input = sio.input(0).slice::<Complex32>();
        
        // Berechne die Output-Slices am Anfang
        let mut output_slices: Vec<&mut [Complex32]> = Vec::new();
        for i in 0..self.protocols.len() {
            output_slices.push(sio.output(i).slice::<Complex32>());
        }

        let min_output_len = output_slices.iter().map(|slice| slice.len()).min().unwrap_or(0);

        let max_process = std::cmp::min(
            input.len().saturating_sub(self.total_protocol_length - 1),
            min_output_len
        ).saturating_sub(1);

        self.debug_echo(&format!(
            "DEBUG: Processing samples from index {} to {}",
            self.current_index,
            self.current_index + max_process
        ));

        let mut matches = Vec::new();
        let current_protocol = self.current_protocol.unwrap_or(0);

        // Füge das aktuelle Protokoll am Anfang hinzu
        matches.push((current_protocol, 0));

        for i in 0..max_process {
            let mut protocol_matched = false;
            for (protocol_index, protocol) in self.protocols.iter().enumerate() {
                let (matched, correlations) = self.match_protocol(&input, i, protocol);
                self.debug_echo(&format!(
                    "DEBUG: Index {}, Protocol: {}, Matched: {}",
                    self.current_index, protocol.name, matched
                ));
                for (seq_index, corr) in correlations.iter().enumerate() {
                    self.debug_echo(&format!(
                        "DEBUG:   Sequence {}: Correlation = {:.4}, Threshold = {:.4}",
                        seq_index, corr, protocol.sequences[seq_index].threshold
                    ));
                }
                if matched {
                    if protocol_index != current_protocol {
                        matches.push((protocol_index, i));
                        self.debug_echo(&format!(
                            "Switching from {} to {} protocol at index {}",
                            self.protocols[current_protocol].name,
                            protocol.name,
                            self.current_index + i
                        ));
                        self.current_protocol = Some(protocol_index);
                        sio.output(protocol_index).add_tag(
                            i,
                            Tag::String(format!("{} Start", protocol.name)),
                        );
                    }
                    self.log_sequence_match(&protocol.name)?;
                    protocol_matched = true;
                    break;
                }
            }
           self.current_index+=1; 
        }

        // Füge das letzte Match hinzu (entweder das aktuelle Protokoll bis zum Ende)
        matches.push((self.current_protocol.unwrap_or(current_protocol), max_process));

        let mut output_status = vec![0; self.protocols.len()];
        let mut consumed = 0;

        for window in matches.windows(2) {
            let (current_protocol, current_index) = window[0];
            let next_index = window[1].1;
            let output_len = next_index - current_index;

            let output = &mut output_slices[current_protocol];
            let start_idx = output_status[current_protocol];
            output[start_idx..start_idx + output_len].copy_from_slice(&input[current_index..next_index]);
            output_status[current_protocol] += output_len;

            consumed += output_len;
        }
        if(consumed != max_process){
            exit(1);
        }
        sio.input(0).consume(consumed);
        for (protocol_index, &produced) in output_status.iter().enumerate() {
            sio.output(protocol_index).produce(produced);
            self.debug_echo(&format!(
                "DEBUG: Protocol {} produced {} samples",
                self.protocols[protocol_index].name,
                produced
            ));
        }

        self.debug_echo(&format!(
            "DEBUG: Processed {} samples. Current index: {}. Matches found: {}. Current protocol: {}",
            consumed, 
            self.current_index, 
            matches.len() - 2,  // Subtracting 2 because of the initial and final entries
            self.protocols[self.current_protocol.unwrap_or(0)].name
        ));

        if sio.input(0).finished() {
            
            let  out= sio.output(current_protocol).slice::<Complex32>();
            let mut inp = sio.input(0).slice::<Complex32>();
            let mut len = inp.len();
            if len > 0 {
                if out.len() > 0{
                    out[0] = inp[0];
                    sio.input(0).consume(1);
                    sio.output(current_protocol).produce(1); 
                }

            }else{
                self.debug_echo(&format!(
                    "DEBUG: Input stream finished at index {}",
                    self.current_index
                ));
                io.finished = true;
            }

        }

        Ok(())
    }
}
