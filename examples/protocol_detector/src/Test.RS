use crate::detector::{Protocol, Sequence};
use futuresdr::anyhow::Result;
use futuresdr::macros::async_trait;
use futuresdr::num_complex::Complex32;
use futuresdr::runtime::{
    Block, BlockMeta, BlockMetaBuilder, ItemTag, Kernel, MessageIo, MessageIoBuilder, StreamIo,
    StreamIoBuilder, Tag, WorkIo,
};

pub struct MultiProtocolInserter {
    protocols: Vec<Protocol>,
    current_sequence: Option<Vec<Complex32>>,
    sequence_index: usize,
    is_inserting: bool,
    pad_front: usize,
    pad_back: usize,
}

impl MultiProtocolInserter {
    pub fn new(protocols: Vec<Protocol>, pad_front: usize, pad_back: usize) -> Block {
        Block::new(
            BlockMetaBuilder::new("MultiProtocolInserter").build(),
            StreamIoBuilder::new()
                .add_input::<Complex32>("in")
                .add_output::<Complex32>("out")
                .build(),
            MessageIoBuilder::new().build(),
            MultiProtocolInserter {
                protocols,
                current_sequence: None,
                sequence_index: 0,
                is_inserting: false,
                pad_front,
                pad_back,
            },
        )
    }

    fn copy_sequence(
        &mut self,
        output: &mut [Complex32],
        output_index: usize,
        max_index: usize,
    ) -> usize {
        if let Some(sequence) = &self.current_sequence {
            let remaining_sequence = &sequence[self.sequence_index..];
            let available_output = max_index - output_index;
            let copy_len = remaining_sequence.len().min(available_output);

            output[output_index..output_index + copy_len]
                .copy_from_slice(&remaining_sequence[..copy_len]);

            self.sequence_index += copy_len;
            self.is_inserting = self.sequence_index < sequence.len();

            output_index + copy_len
        } else {
            output_index
        }
    }

    fn copy_input_to_output(
        &self,
        input: &[Complex32],
        output: &mut [Complex32],
        input_start: usize,
        input_end: usize,
        output_index: usize,
        max_index: usize,
    ) -> usize {
        let copy_len = input_end
            .saturating_sub(input_start)
            .min(max_index - output_index);
        output[output_index..output_index + copy_len]
            .copy_from_slice(&input[input_start..input_start + copy_len]);
        output_index + copy_len
    }
}

#[async_trait]
impl Kernel for MultiProtocolInserter {
    async fn work(
        &mut self,
        io: &mut WorkIo,
        sio: &mut StreamIo,
        _m: &mut MessageIo<Self>,
        _b: &mut BlockMeta,
    ) -> Result<()> {
        let input = sio.input(0).slice::<Complex32>();
        let output = sio.output(0).slice::<Complex32>();
        let tags = sio.input(0).tags();

        let max_index = output.len().saturating_sub(1);
        let mut output_index = 0;
        let mut input_index = 0;
        let mut tag_index = 0;

        if self.is_inserting {
            output_index = self.copy_sequence(output, output_index, max_index);
        }

        while output_index <= max_index && tag_index < tags.len() {
            let current_tag = &tags[tag_index];
            let next_tag_index = if tag_index + 1 < tags.len() {
                tags[tag_index + 1].index
            } else {
                input.len()
            };

            if output_index <= max_index {
                output_index = self.copy_input_to_output(
                    input,
                    output,
                    input_index,
                    current_tag.index,
                    output_index,
                    max_index,
                );
            }

            if let Tag::NamedUsize(protocol_name, _) = &current_tag.tag {
                if let Some(protocol) = self.protocols.iter().find(|p| &p.name == protocol_name) {
                    let mut padded_sequence = Vec::with_capacity(
                        self.pad_front + protocol.sequence.data.len() + self.pad_back,
                    );
                    padded_sequence
                        .extend(std::iter::repeat(Complex32::new(0.0, 0.0)).take(self.pad_front));
                    padded_sequence.extend_from_slice(&protocol.sequence.data);
                    padded_sequence
                        .extend(std::iter::repeat(Complex32::new(0.0, 0.0)).take(self.pad_back));

                    self.current_sequence = Some(padded_sequence);
                    self.sequence_index = 0;
                    self.is_inserting = true;
                    output_index = self.copy_sequence(output, output_index, max_index);
                }
            }

            input_index = current_tag.index;
            tag_index += 1;
        }

        if output_index <= max_index {
            output_index = self.copy_input_to_output(
                input,
                output,
                input_index,
                input.len(),
                output_index,
                max_index,
            );
            input_index = input.len();
        }

        let input_finished = sio.input(0).finished();

        if input_finished && input_index >= input.len() {
            if output_index == 0 && input_index == input.len() {
                io.finished = true;
            } else if output_index < max_index && input_index < input.len() {
                // Copy one more sample
                output[output_index] = input[input_index];
                output_index += 1;
                input_index += 1;
            }
        }

        sio.input(0).consume(input_index);
        sio.output(0).produce(output_index);

        Ok(())
    }
}
