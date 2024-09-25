use futuresdr::anyhow::Result;
use futuresdr::blocks::{FileSink, Head, Source};
use futuresdr::num_complex::Complex32;
use futuresdr::runtime::{Flowgraph, Runtime};
use std::fs::File;
use std::io::{BufReader, Read};
use std::sync::Arc;
use protocol_detector::{MultiProtocolInserter, Protocol, Sequence, SimpleTagInserter};

#[cfg(test)]
mod tests {
    use super::*;

    fn load_complex32_from_file(filename: &str) -> Result<Vec<Complex32>> {
        let file = File::open(filename)?;
        let mut reader = BufReader::new(file);
        let mut buffer = [0u8; 8]; // 8 bytes for two f32 values
        let mut vec = Vec::new();

        while reader.read_exact(&mut buffer).is_ok() {
            let re = f32::from_le_bytes([buffer[0], buffer[1], buffer[2], buffer[3]]);
            let im = f32::from_le_bytes([buffer[4], buffer[5], buffer[6], buffer[7]]);
            vec.push(Complex32::new(re, im));
        }

        Ok(vec)
    }

    #[test]
    fn test_multi_protocol_inserter() -> Result<()> {
        let mut fg = Flowgraph::new();

        // Define protocols
        let protocols = vec![
            Protocol {
                name: "zc".to_string(),
                sequence: Sequence::new(vec![Complex32::new(1.0, 0.0); 3], 0.7),
                sequences: vec![],
            },
            Protocol {
                name: "lora".to_string(),
                sequence: Sequence::new(vec![Complex32::new(0.0, 1.0); 3], 0.7),
                sequences: vec![],
            },
        ];

        let pad_front = 2;
        let pad_tail = 2;

        // Create blocks
        let src_block = fg.add_block(Source::new(move || Complex32::new(-1.0, -1.0)));

        // Add SimpleTagInserter with updated interval
        let tag_inserter = SimpleTagInserter::new(20, vec!["zc".to_string(), "lora".to_string()]);
        let tag_inserter_block = fg.add_block(tag_inserter);

        let inserter = MultiProtocolInserter::new(protocols, pad_front, pad_tail);
        let inserter_block = fg.add_block(inserter);

        // Update Head block to process 120 samples
        let head = Head::<Complex32>::new(120);
        let head_block = fg.add_block(head);

        let sink = FileSink::<Complex32>::new("inserter_output.bin");
        let sink_block = fg.add_block(sink);

        // Connect blocks
        fg.connect_stream(src_block, "out", tag_inserter_block, "in")?;
        fg.connect_stream(tag_inserter_block, "out", inserter_block, "in")?;
        fg.connect_stream(inserter_block, "out", head_block, "in")?;
        fg.connect_stream(head_block, "out", sink_block, "in")?;

        // Run the flowgraph
        Runtime::new().run(fg)?;

        // Load the output from the file
        let output = load_complex32_from_file("inserter_output.bin")?;
        println!("{:?}", output);

        let expected_output: Vec<Complex32> = vec![
            // ZC sequence (7 samples)
            vec![Complex32::new(0.0, 0.0); 2],    // Front padding
            vec![Complex32::new(1.0, 0.0); 3],    // ZC sequence
            vec![Complex32::new(0.0, 0.0); 2],    // Tail padding
            vec![Complex32::new(-1.0, -1.0); 20], // 20 samples from source
            // LoRa sequence (7 samples)
            vec![Complex32::new(0.0, 0.0); 2],    // Front padding
            vec![Complex32::new(0.0, 1.0); 3],    // LoRa sequence
            vec![Complex32::new(0.0, 0.0); 2],    // Tail padding
            vec![Complex32::new(-1.0, -1.0); 20], // 20 samples from source
            // ZC sequence (7 samples)
            vec![Complex32::new(0.0, 0.0); 2],    // Front padding
            vec![Complex32::new(1.0, 0.0); 3],    // ZC sequence
            vec![Complex32::new(0.0, 0.0); 2],    // Tail padding
            vec![Complex32::new(-1.0, -1.0); 20], // 20 samples from source
            // LoRa sequence (7 samples)
            vec![Complex32::new(0.0, 0.0); 2],    // Front padding
            vec![Complex32::new(0.0, 1.0); 3],    // LoRa sequence
            vec![Complex32::new(0.0, 0.0); 2],    // Tail padding
            vec![Complex32::new(-1.0, -1.0); 20], // 20 samples from source
            // ZC sequence (7 samples)
            vec![Complex32::new(0.0, 0.0); 2],   // Front padding
            vec![Complex32::new(1.0, 0.0); 3],   // ZC sequence
            vec![Complex32::new(0.0, 0.0); 2],   // Tail padding
            vec![Complex32::new(-1.0, -1.0); 5], // 5 samples from source to reach 120 total
        ]
        .into_iter()
        .flatten()
        .collect();
        assert_eq!(
            output, expected_output,
            "Output does not match expected result"
        );

        // Clean up the output file
        std::fs::remove_file("inserter_output.bin")?;

        Ok(())
    }
}
