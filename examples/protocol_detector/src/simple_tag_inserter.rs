use futuresdr::anyhow::Result;
use futuresdr::macros::async_trait;
use futuresdr::num_complex::Complex32;
use futuresdr::runtime::{
    Block, BlockMeta, BlockMetaBuilder, Kernel, MessageIo, MessageIoBuilder, StreamIo,
    StreamIoBuilder, Tag, WorkIo,
};

pub struct SimpleTagInserter {
    counter: usize,
    interval: usize,
    protocols: Vec<String>,
}

impl SimpleTagInserter {
    pub fn new(interval: usize, protocols: Vec<String>) -> Block {
        Block::new(
            BlockMetaBuilder::new("SimpleTagInserter").build(),
            StreamIoBuilder::new()
                .add_input::<Complex32>("in")
                .add_output::<Complex32>("out")
                .build(),
            MessageIoBuilder::new().build(),
            SimpleTagInserter {
                counter: 0,
                interval,
                protocols,
            },
        )
    }
}

#[async_trait]
impl Kernel for SimpleTagInserter {
    async fn work(
        &mut self,
        io: &mut WorkIo,
        sio: &mut StreamIo,
        _m: &mut MessageIo<Self>,
        _b: &mut BlockMeta,
    ) -> Result<()> {
        let input = sio.input(0).slice::<Complex32>();
        let output = sio.output(0).slice::<Complex32>();

        let n = input.len().min(output.len());
        output[..n].copy_from_slice(&input[..n]);
        if output.len() >= n {
            for i in 0..n {
                if self.counter % self.interval == 0 {
                    let protocol_index = (self.counter / self.interval) % self.protocols.len();
                    let protocol = &self.protocols[protocol_index];
                    sio.output(0)
                        .add_tag(i, Tag::NamedUsize(protocol.clone(), self.interval));
                }
                self.counter += 1;
            }

            sio.input(0).consume(n);
            sio.output(0).produce(n);
        }

        if sio.input(0).finished() {
            io.finished = true;
        }

        Ok(())
    }
}
