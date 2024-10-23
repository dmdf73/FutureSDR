use futuresdr::anyhow::Result;
use futuresdr::blocks::{FileSink, Head, Source};
use futuresdr::num_complex::Complex32;
use futuresdr::runtime::{Flowgraph, Runtime};

use rand::thread_rng;
use rand_distr::{Distribution, Normal};
use std::time::Instant;

use protocol_detector::{
    generate_zadoff_chu, load_complex32_from_file, load_log_file, Protocol, ProtocolDetectorFFT,
    Sequence,
};

fn generate_protocols(include_pad: bool) -> Vec<Protocol> {
    // Adjust the length parameter to 120
    let sync_sequence = generate_zadoff_chu(11, 64, 0, 0);
    let zc_sequence = generate_zadoff_chu(17, 64, 0, 0);
    let gold_sequence = generate_zadoff_chu(31, 64, 0, 0);
    let pad = vec![Complex32::new(0.0, 0.0); 30]; // 30 zeros for padding

    vec![
        Protocol {
            name: "zc".to_string(),
            sequence: if include_pad {
                Sequence::new(
                    [
                        pad.clone(),
                        sync_sequence.clone(),
                        zc_sequence.clone(),
                        pad.clone(),
                    ]
                    .concat(),
                    0.65,
                )
            } else {
                Sequence::new([sync_sequence.clone(), zc_sequence.clone()].concat(), 0.65)
            },
            sequences: vec![Sequence::new(zc_sequence.clone(), 0.65)],
        },
        Protocol {
            name: "gold".to_string(),
            sequence: if include_pad {
                Sequence::new(
                    [
                        pad.clone(),
                        sync_sequence.clone(),
                        gold_sequence.clone(),
                        pad.clone(),
                    ]
                    .concat(),
                    0.65,
                )
            } else {
                Sequence::new(
                    [sync_sequence.clone(), gold_sequence.clone()].concat(),
                    0.65,
                )
            },
            sequences: vec![Sequence::new(gold_sequence.clone(), 0.65)],
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_large_protocol_detection_fft() -> Result<()> {
        let mut fg = Flowgraph::new();
        let std_dev = 0.05;
        let normal = Normal::new(0.0f32, std_dev).unwrap();

        let protocols_for_source = generate_protocols(true); // Mit PAD
        let protocols_for_detector = generate_protocols(false); // Ohne PAD

        let mut sample_counter = 0;
        let mut is_inserting = false;
        let mut current_protocol_index = 0;
        let mut sequence_position = 0;

        let source_with_protocols = move || {
            if !is_inserting && sample_counter % 1000 == 0 {
                is_inserting = true;
                current_protocol_index = (sample_counter / 1000) % 2;
                sequence_position = 0;
            }

            let result = if is_inserting {
                let protocol = &protocols_for_source[current_protocol_index];
                let sequence = &protocol.sequence.data;

                if sequence_position < sequence.len() {
                    let sample = sequence[sequence_position];
                    sequence_position += 1;
                    sample
                } else {
                    is_inserting = false;
                    sample_counter += 1;
                    Complex32::new(
                        normal.sample(&mut thread_rng()),
                        normal.sample(&mut thread_rng()),
                    )
                }
            } else {
                sample_counter += 1;
                Complex32::new(
                    normal.sample(&mut thread_rng()),
                    normal.sample(&mut thread_rng()),
                )
            };

            result
        };

        let src_block = fg.add_block(Source::new(source_with_protocols));

        let detector = ProtocolDetectorFFT::new(
            Sequence::new(generate_zadoff_chu(11, 64, 0, 0), 0.65),
            protocols_for_detector,
            true,
            Some("matches.log".to_owned()),
        );
        let detector_block = fg.add_block(detector);
        let head = Head::<Complex32>::new(1000000);
        let head_block = fg.add_block(head);
        let zc_sink = FileSink::<Complex32>::new("zc_output.bin");
        let zc_sink_block = fg.add_block(zc_sink);
        let gold_sink = FileSink::<Complex32>::new("gold_output.bin");
        let gold_sink_block = fg.add_block(gold_sink);

        fg.connect_stream(src_block, "out", head_block, "in")?;
        fg.connect_stream(head_block, "out", detector_block, "in")?;
        fg.connect_stream(detector_block, "zc", zc_sink_block, "in")?;
        fg.connect_stream(detector_block, "gold", gold_sink_block, "in")?;

        let runtime = Runtime::new();
        let start_time = Instant::now();
        runtime.run(fg)?;
        let duration = start_time.elapsed();
        println!("Graph execution time: {:?}", duration);

        let zc_output = load_complex32_from_file("zc_output.bin")?;
        let gold_output = load_complex32_from_file("gold_output.bin")?;
        assert!(!zc_output.is_empty(), "ZC output should not be empty");
        assert!(!gold_output.is_empty(), "Gold output should not be empty");

        let log_lines = load_log_file("matches.log")?;
        let all_positions: Vec<String> = log_lines
            .into_iter()
            .filter(|line| line.contains(','))
            .collect();
        assert!(!all_positions.is_empty(), "No protocol switches detected");
        assert_eq!(
            all_positions[0], "30,zc",
            "Should start with zc at index 30"
        );
        for (i, window) in all_positions.windows(2).enumerate() {
            let index1: usize = window[0].split(',').next().unwrap().parse().unwrap();
            let index2: usize = window[1].split(',').next().unwrap().parse().unwrap();
            let distance = index2 - index1;
            assert_eq!(
                true,
                distance == 1188,
                "Incorrect distance at pair {}: expected 1188, got {} (indices: {} and {})",
                i,
                distance,
                index1,
                index2
            );
        }

        std::fs::remove_file("zc_output.bin")?;
        std::fs::remove_file("gold_output.bin")?;

        Ok(())
    }
}
