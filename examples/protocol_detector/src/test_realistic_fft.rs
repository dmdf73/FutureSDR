use futuresdr::anyhow::Result;
use futuresdr::blocks::{FileSink, Head, Source};
use futuresdr::num_complex::Complex32;
use futuresdr::runtime::{Flowgraph, Runtime};
use protocol_detector::sequences::{GOLD_64, ZC_120, ZC_64};
use protocol_detector::{
    MultiProtocolInserter, Protocol, ProtocolDetectorFFT, Sequence, SimpleTagInserter,
};
use rand::thread_rng;
use rand_distr::{Distribution, Normal};
use std::fs::File;
use std::io::{BufReader, Read};
use std::time::Instant;

#[cfg(test)]
mod tests {
    use std::fs::{File, OpenOptions};
    use std::io::{BufRead, BufReader, Read};

    use protocol_detector::sequences::{GOLD_31, ZC_31, ZC_32};

    use super::*;

    fn load_complex32_from_file(filename: &str) -> Result<Vec<Complex32>> {
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

    fn load_log_file(filename: &str) -> Result<Vec<String>> {
        let file = File::open(filename)?;
        let reader = BufReader::new(file);
        let lines: Result<Vec<String>, _> = reader.lines().collect();
        Ok(lines?)
    }

    #[test]
    fn test_realistic_multi_protocol_system() -> Result<()> {
        let mut fg = Flowgraph::new();

        // White noise source with adjusted standard deviation
        let std_dev = 0.05;
        let normal = Normal::new(0.0f32, std_dev).unwrap();
        let white_noise_generator = move || {
            let mut rng = thread_rng();
            let re = normal.sample(&mut rng);
            let im = normal.sample(&mut rng);
            Complex32::new(re, im)
        };

        let src_block = fg.add_block(Source::new(white_noise_generator));

        // Define sync sequence using ZC_120
        let sync_sequence = Sequence::new(ZC_120.to_vec(), 0.65);

        let protocols = vec![
            Protocol {
                name: "zc".to_string(),
                sequence: Sequence::new(ZC_64.to_vec(), 0.65),
                sequences: vec![Sequence::new(ZC_64.to_vec(), 0.65)],
            },
            Protocol {
                name: "gold".to_string(),
                sequence: Sequence::new(GOLD_64.to_vec(), 0.65),
                sequences: vec![Sequence::new(GOLD_64.to_vec(), 0.65)],
            },
        ];

        // SimpleTagInserter with increased interval
        let tag_inserter = SimpleTagInserter::new(1000, vec!["zc".to_string(), "gold".to_string()]);
        let tag_inserter_block = fg.add_block(tag_inserter);

        // MultiProtocolInserter with increased padding
        let inserter = MultiProtocolInserter::new(protocols.clone(), 30, 30);
        let inserter_block = fg.add_block(inserter);

        // ProtocolDetectorFFT
        let detector = ProtocolDetectorFFT::new(
            sync_sequence,
            protocols,
            true,
            Some("matches.log".to_owned()),
        );
        let detector_block = fg.add_block(detector);

        // Head block with increased sample count
        let head = Head::<Complex32>::new(100000);
        let head_block = fg.add_block(head);

        // FileSinks for each protocol
        let zc_sink = FileSink::<Complex32>::new("zc_output.bin");
        let zc_sink_block = fg.add_block(zc_sink);

        let gold_sink = FileSink::<Complex32>::new("gold_output.bin");
        let gold_sink_block = fg.add_block(gold_sink);

        // Connect blocks
        fg.connect_stream(src_block, "out", tag_inserter_block, "in")?;
        fg.connect_stream(tag_inserter_block, "out", inserter_block, "in")?;
        fg.connect_stream(inserter_block, "out", head_block, "in")?;
        fg.connect_stream(head_block, "out", detector_block, "in")?;
        fg.connect_stream(detector_block, "zc", zc_sink_block, "in")?;
        fg.connect_stream(detector_block, "gold", gold_sink_block, "in")?;

        // Run the flowgraph and measure execution time
        let start_time = Instant::now();
        Runtime::new().run(fg)?;
        let execution_time = start_time.elapsed();

        println!("Flowgraph execution time: {:?}", execution_time);

        // Load and analyze the output files
        let zc_output = load_complex32_from_file("zc_output.bin")?;
        let gold_output = load_complex32_from_file("gold_output.bin")?;

        // Perform assertions or analysis on the outputs
        assert!(!zc_output.is_empty(), "ZC output should not be empty");
        assert!(!gold_output.is_empty(), "Gold output should not be empty");

        // Load and check the log file for detected protocols
        let log_lines = load_log_file("matches.log")?;

        let all_positions: Vec<String> = log_lines
            .into_iter()
            .filter(|line| line.contains(','))
            .collect();

        // Assertions
        assert!(!all_positions.is_empty(), "No protocol switches detected");
        assert_eq!(
            all_positions[0], "30,zc",
            "Should start with zc at index 30"
        );

        for window in all_positions.windows(2) {
            let index1: usize = window[0].split(',').next().unwrap().parse().unwrap();
            let index2: usize = window[1].split(',').next().unwrap().parse().unwrap();
            assert_eq!(
                index2 - index1,
                1186,
                "Distance between detections should be 1186"
            );
        }

        // Clean up output files
        std::fs::remove_file("zc_output.bin")?;
        std::fs::remove_file("gold_output.bin")?;

        Ok(())
    }
}
