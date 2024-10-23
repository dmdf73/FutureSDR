use futuresdr::anyhow::Result;
use futuresdr::macros::async_trait;
use futuresdr::num_complex::Complex32;
use futuresdr::runtime::MessageIoBuilder;
use futuresdr::runtime::StreamIoBuilder;
use futuresdr::runtime::{Block, BlockMeta, BlockMetaBuilder, Kernel, MessageIo, StreamIo, WorkIo};
use futuresdr::runtime::{ItemTag, Tag};

const DEBUG: bool = false;

fn debug_print(message: &str) {
    if DEBUG {
        println!("{}", message);
    }
}

pub struct MultiPortInserter {
    ports: Vec<(String, String)>,
    sequences: Vec<Vec<Complex32>>,
    current_port: Option<usize>,
    current_sequence: Option<usize>,
    inserting_sequence: bool,
    sequence_index: usize,
    packet_length: usize,
    consumed_input: usize,
    insertion_index: usize,
    samples_after_sequence: usize,
    port_order: Vec<usize>,
}

impl MultiPortInserter {
    pub fn new(
        ports: Vec<(String, String)>,
        sequences: Vec<Vec<Complex32>>,
        pad_front: usize,
        pad_back: usize,
    ) -> Block {
        debug_print("=== Creating MultiPortInserter ===");
        debug_print(&format!("Number of ports: {}", ports.len()));
        debug_print(&format!("Pad front: {}, Pad back: {}", pad_front, pad_back));

        assert_eq!(
            ports.len(),
            sequences.len(),
            "Each port must have a corresponding sequence"
        );

        let padded_sequences: Vec<Vec<Complex32>> = sequences
            .into_iter()
            .enumerate()
            .map(|(i, seq)| {
                let mut padded_seq = vec![Complex32::new(0.0, 0.0); pad_front];
                padded_seq.extend(seq);
                padded_seq.extend(vec![Complex32::new(0.0, 0.0); pad_back]);
                padded_seq
            })
            .collect();
        let ports_length = ports.len();
        Block::new(
            BlockMetaBuilder::new("MultiPortInserter").build(),
            {
                let mut sio = StreamIoBuilder::new();
                for (port_name, _) in &ports {
                    sio = sio.add_input::<Complex32>(port_name);
                }
                sio.add_output::<Complex32>("out").build()
            },
            MessageIoBuilder::new().build(),
            MultiPortInserter {
                ports,
                sequences: padded_sequences,
                current_port: None,
                current_sequence: None,
                inserting_sequence: false,
                sequence_index: 0,
                packet_length: 0,
                consumed_input: 0,
                insertion_index: 0,
                samples_after_sequence: 0,
                port_order: (0..ports_length).collect(),
            },
        )
    }
}

#[async_trait]
impl Kernel for MultiPortInserter {
    async fn work(
        &mut self,
        io: &mut WorkIo,
        sio: &mut StreamIo,
        _mio: &mut MessageIo<Self>,
        _meta: &mut BlockMeta,
    ) -> Result<()> {
        debug_print("\n=== Starting work cycle ===");

        // Check if all input buffers are empty and finished
        let all_inputs_empty_and_finished = self.ports.iter().enumerate().all(|(i, _)| {
            let input = sio.input(i).slice::<Complex32>();
            input.is_empty() && sio.input(i).finished()
        });

        if all_inputs_empty_and_finished {
            debug_print("All input buffers are empty and finished. Marking block as finished.");
            io.finished = true;
            debug_print("=== End of work cycle (finished) ===\n");
            return Ok(());
        }

        if self.current_port.is_none() {
            debug_print("Searching for a matching tag...");
            for &port_index in &self.port_order {
                let (port_name, search_string) = &self.ports[port_index];
                let input = sio.input(port_index).slice::<Complex32>();
                let tags = sio.input(port_index).tags();
                debug_print(&format!(
                    "Checking port {} ({}): {} tags found",
                    port_index,
                    port_name,
                    tags.len()
                ));

                if let Some((index, len)) = tags.iter().find_map(|x| match x {
                    ItemTag {
                        index,
                        tag: Tag::NamedUsize(n, len),
                    } => {
                        if n == search_string {
                            debug_print(&format!(
                                "Found matching tag: {} at index {} with length {}",
                                n, index, len
                            ));
                            Some((*index, *len))
                        } else {
                            None
                        }
                    }
                    _ => None,
                }) {
                    debug_print(&format!(
                        "Switching to port {} ({}) with sequence length {}",
                        port_index, port_name, len
                    ));
                    self.current_port = Some(port_index);
                    self.current_sequence = Some(port_index);
                    self.inserting_sequence = true;
                    self.sequence_index = 0;
                    self.packet_length = len;
                    self.consumed_input = 0;
                    self.insertion_index = index;
                    self.samples_after_sequence = 0;

                    // Update port order
                    self.port_order.retain(|&p| p != port_index);
                    self.port_order.push(port_index);

                    break;
                }
            }
        }

        if let Some(port_index) = self.current_port {
            let input = sio.input(port_index).slice::<Complex32>();
            let mut output = sio.output(0).slice::<Complex32>();
            let mut input_consumed = 0;
            let mut output_produced = 0;

            debug_print(&format!("Processing port {}", port_index));
            debug_print(&format!("Input buffer size: {}", input.len()));
            debug_print(&format!("Output buffer size: {}", output.len()));

            // Copy input data before insertion point
            if self.consumed_input < self.insertion_index {
                let pre_insertion = (self.insertion_index - self.consumed_input)
                    .min(input.len())
                    .min(output.len());
                output[0..pre_insertion].copy_from_slice(&input[0..pre_insertion]);
                input_consumed += pre_insertion;
                output_produced += pre_insertion;
                self.consumed_input += pre_insertion;
                debug_print(&format!(
                    "Copied {} samples before insertion point",
                    pre_insertion
                ));
            }

            if self.inserting_sequence {
                // Insert padded sequence
                let sequence = &self.sequences[self.current_sequence.unwrap()];
                let to_insert = sequence.len() - self.sequence_index;
                let data_inserted = to_insert.min(output.len() - output_produced);
                output[output_produced..output_produced + data_inserted].copy_from_slice(
                    &sequence[self.sequence_index..self.sequence_index + data_inserted],
                );
                output_produced += data_inserted;
                self.sequence_index += data_inserted;
                self.inserting_sequence = self.sequence_index < sequence.len();

                debug_print(&format!(
                    "Inserting sequence: {} samples inserted, {} remaining",
                    data_inserted,
                    sequence.len() - self.sequence_index
                ));

                if !self.inserting_sequence {
                    debug_print("Sequence insertion complete");
                    self.sequence_index = 0;
                }
            }

            // Copy remaining input data, but only up to packet_length after sequence
            if !self.inserting_sequence {
                let remaining_to_copy = self.packet_length - self.samples_after_sequence;
                let data_to_copy = input
                    .len()
                    .min(remaining_to_copy)
                    .min(output.len() - output_produced);

                output[output_produced..output_produced + data_to_copy]
                    .copy_from_slice(&input[input_consumed..input_consumed + data_to_copy]);
                input_consumed += data_to_copy;
                output_produced += data_to_copy;
                self.samples_after_sequence += data_to_copy;

                debug_print(&format!(
                    "Copied {} samples after sequence insertion",
                    data_to_copy
                ));
                debug_print(&format!(
                    "Total samples after sequence: {}/{}",
                    self.samples_after_sequence, self.packet_length
                ));

                if self.samples_after_sequence >= self.packet_length {
                    debug_print("Packet processing complete. Resetting state.");
                    self.current_port = None;
                    self.current_sequence = None;
                    self.inserting_sequence = false;
                    self.sequence_index = 0;
                    self.packet_length = 0;
                    self.consumed_input = 0;
                    self.insertion_index = 0;
                    self.samples_after_sequence = 0;
                }
            }

            sio.input(port_index).consume(input_consumed);
            sio.output(0).produce(output_produced);
            debug_print("Work cycle complete:");
            debug_print(&format!("  - Samples consumed: {}", input_consumed));
            debug_print(&format!("  - Samples produced: {}", output_produced));
            debug_print(&format!(
                "  - Remaining input: {}",
                input.len() - input_consumed
            ));
            debug_print(&format!(
                "  - Remaining output capacity: {}",
                output.len() - output_produced
            ));
        } else {
            debug_print("No active port. Waiting for matching tag.");
        }

        debug_print("=== End of work cycle ===\n");
        Ok(())
    }
}
