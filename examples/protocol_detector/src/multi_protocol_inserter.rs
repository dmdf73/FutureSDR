use crate::detector::Protocol;
use futuresdr::anyhow::Result;
use futuresdr::macros::async_trait;
use futuresdr::num_complex::Complex32;
use futuresdr::runtime::{
    Block, BlockMeta, BlockMetaBuilder, ItemTag, Kernel, MessageIo, MessageIoBuilder, StreamIo,
    StreamIoBuilder, Tag, WorkIo,
};
pub struct MultiProtocolInserter {
    protocols: Vec<Protocol>,
    pad_front: usize,
    pad_tail: usize,
    current_sequence: Option<Vec<Complex32>>,
    sequence_index: usize,
    padding_index: usize,
    is_inserting: bool,
}

impl MultiProtocolInserter {
    pub fn new(protocols: Vec<Protocol>, pad_front: usize, pad_tail: usize) -> Block {
        Block::new(
            BlockMetaBuilder::new("MultiProtocolInserter").build(),
            StreamIoBuilder::new()
                .add_input::<Complex32>("in")
                .add_output::<Complex32>("out")
                .build(),
            MessageIoBuilder::new().build(),
            MultiProtocolInserter {
                protocols,
                pad_front,
                pad_tail,
                current_sequence: None,
                sequence_index: 0,
                padding_index: 0,
                is_inserting: false,
            },
        )
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

        if input.is_empty() || output.is_empty() {
            return Ok(());
        }

        let tags = sio.input(0).tags();

        if !self.is_inserting {
            if let Some(ItemTag {
                index: 0,
                tag: Tag::NamedUsize(protocol_name, _),
            }) = tags.first()
            {
                if let Some(protocol) = self.protocols.iter().find(|p| &p.name == protocol_name) {
                    self.current_sequence = Some(protocol.sequence.data.clone());
                    self.sequence_index = 0;
                    self.padding_index = 0;
                    self.is_inserting = true;
                }
            }
        }

        if self.is_inserting {
            if self.padding_index < self.pad_front {
                output[0] = Complex32::new(0.0, 0.0);
                self.padding_index += 1;
            } else if let Some(seq) = &self.current_sequence {
                if self.sequence_index < seq.len() {
                    output[0] = seq[self.sequence_index];
                    self.sequence_index += 1;
                } else if self.padding_index < self.pad_front + self.pad_tail {
                    output[0] = Complex32::new(0.0, 0.0);
                    self.padding_index += 1;
                } else {
                    self.is_inserting = false;
                    self.current_sequence = None;
                    output[0] = input[0];
                    sio.input(0).consume(1);
                }
            }
        } else {
            output[0] = input[0];
            sio.input(0).consume(1);
        }

        sio.output(0).produce(1);

        if sio.input(0).finished() {
            io.finished = true;
        }

        Ok(())
    }
}
