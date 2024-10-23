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
        mut protocols: Vec<Protocol>,
        sync_sequence: Option<Sequence>,
        debug_enabled: bool,
        debug_log_file: Option<String>,
    ) -> Block {
        Self::validate_protocols(&protocols, &sync_sequence).expect("Invalid protocols");
        
        if let Some(sync) = sync_sequence {
            for protocol in &mut protocols {
                protocol.sequences.insert(0, sync.clone());
            }
        }
        
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

    fn validate_protocols(protocols: &[Protocol], sync_sequence: &Option<Sequence>) -> Result<()> {
        if protocols.is_empty() {
            return Err(anyhow!("No protocols provided"));
        }

        let reference_sequence_length = if let Some(sync) = sync_sequence {
            sync.data.len()
        } else {
            protocols[0].sequences[0].data.len()
        };

        if let Some(sync) = sync_sequence {
            if sync.data.len() != reference_sequence_length {
                return Err(anyhow!("Sync sequence length does not match the reference length"));
            }
        }

        for (i, protocol) in protocols.iter().enumerate() {
            if protocol.sequences.len() != 1 {
                // return Err(anyhow!("Protocol {} must have exactly one sequence", i));
            }

            if protocol.sequences[0].data.len() != reference_sequence_length {
                return Err(anyhow!("Sequence length in protocol {} does not match the reference length", i));
            }
        }

        Ok(())
    }

    fn match_protocol(
        &self,
        input: &[Complex32],
        input_norms: &[f32],
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

            let correlation = self.normalized_dot_product(
                &input[start_index + current_offset..start_index + current_offset + sequence_length],
                &input_norms[start_index + current_offset],
                sequence,
            );

            correlations.push(correlation);

            if correlation < sequence.threshold {
                return (false, correlations);
            }

            current_offset += sequence_length;
        }

        (true, correlations)
    }

    fn normalized_dot_product(
        &self,
        seq1: &[Complex32],
        seq1_norm: &f32,
        seq2: &Sequence,
    ) -> f32 {
        assert_eq!(
            seq1.len(),
            seq2.data.len(),
            "Sequences must have the same length"
        );

        if *seq1_norm == 0.0 || seq2.norm() == 0.0 {
            return if *seq1_norm == seq2.norm() { 1.0 } else { 0.0 };
        }

        let sum_val: Complex32 = seq1
            .iter()
            .zip(seq2.data.iter())
            .map(|(a, b)| a * b.conj())
            .sum();

        let normalized = sum_val / (*seq1_norm * seq2.norm());
        normalized.re
    }

    fn debug_echo(&self, message: &str) {
        if self.debug_enabled {
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
fn calculate_norm_vector(input: &[Complex32], window_length: usize) -> Vec<f32> {
    let input_len = input.len();
    if input_len < window_length {
        return Vec::with_capacity(0);
    }
    let mut norm_vector = Vec::with_capacity(input_len - window_length + 1);
    
    let mut window_sum = input[0..window_length]
        .iter()
        .map(|x| x.norm_sqr())
        .sum::<f32>();
    
    norm_vector.push(window_sum.sqrt());
    
    for i in 1..=(input_len - window_length) {
        window_sum -= input[i-1].norm_sqr();
        window_sum += input[i+window_length-1].norm_sqr();
        norm_vector.push(window_sum.sqrt());
    }
    
    norm_vector
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
        
        let window_length = self.protocols[0].sequences[0].data.len();
        let input_norms = calculate_norm_vector(input, window_length);
        
        let mut output_slices: Vec<&mut [Complex32]> = Vec::new();
        for i in 0..self.protocols.len() {
            output_slices.push(sio.output(i).slice::<Complex32>());
        }

        let min_output_len = output_slices.iter().map(|slice| slice.len()).min().unwrap_or(0);

        let max_process = std::cmp::min(
            input.len().saturating_sub(self.total_protocol_length - 1),
            min_output_len
        ).saturating_sub(1);
        let  mut current_protocol = self.current_protocol.unwrap_or(0);
        if max_process == 0 && sio.input(0).finished() {
            
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


        self.debug_echo(&format!(
            "DEBUG: Processing samples from index {} to {}",
            self.current_index,
            self.current_index + max_process
        ));

        let mut matches = Vec::new();

        matches.push((current_protocol, 0));

        for i in 0..max_process {
            let mut protocol_matched = false;
            for (protocol_index, protocol) in self.protocols.iter().enumerate() {
                let (matched, correlations) = self.match_protocol(input, &input_norms, i, protocol);
                if matched {
                    if protocol_index != current_protocol {
                        matches.push((protocol_index, i));
                        self.debug_echo(&format!(
                            "Switching from {} to {} protocol at index {}",
                            self.protocols[current_protocol].name,
                            protocol.name,
                            self.current_index                         ));
                        current_protocol = protocol_index;
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
            matches.len() - 2,
            self.protocols[self.current_protocol.unwrap_or(0)].name
        ));

        Ok(())
    }
}