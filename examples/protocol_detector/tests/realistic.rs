use futuresdr::anyhow::Result;
use futuresdr::blocks::{FileSink, Head, Source};
use futuresdr::num_complex::Complex32;
use futuresdr::runtime::{Flowgraph, Runtime};
use protocol_detector::sequences::{GOLD_63, ZC_63};
use protocol_detector::{Protocol, ProtocolDetector, Sequence};
use rand::thread_rng;
use rand_distr::{Distribution, Normal};
use std::fs::File;
use std::io::{BufReader, Read};
use std::time::Instant;

fn generate_protocols(include_pad: bool) -> Vec<Protocol> {
    let pad = vec![Complex32::new(0.0, 0.0); 30]; // 30 Nullen für das Padding

    vec![
        Protocol {
            name: "zc".to_string(),
            sequence: if include_pad {
                Sequence::new(
                    [pad.clone(), ZC_63.to_vec(), ZC_63.to_vec(), pad.clone()].concat(),
                    0.65,
                )
            } else {
                Sequence::new([ZC_63.to_vec(), ZC_63.to_vec()].concat(), 0.65)
            },
            sequences: vec![
                Sequence::new(ZC_63.to_vec(), 0.65),
                Sequence::new(ZC_63.to_vec(), 0.65),
            ],
        },
        Protocol {
            name: "gold".to_string(),
            sequence: if include_pad {
                Sequence::new(
                    [pad.clone(), GOLD_63.to_vec(), GOLD_63.to_vec(), pad.clone()].concat(),
                    0.65,
                )
            } else {
                Sequence::new([GOLD_63.to_vec(), GOLD_63.to_vec()].concat(), 0.65)
            },
            sequences: vec![
                Sequence::new(GOLD_63.to_vec(), 0.65),
                Sequence::new(GOLD_63.to_vec(), 0.65),
            ],
        },
    ]
}

#[cfg(test)]
mod tests {
    use std::fs::File;
    use std::io::{BufRead, BufReader, Read};

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

        let detector = ProtocolDetector::new(
            protocols_for_detector,
            true,
            std::option::Option::Some("matches.log".to_owned()),
        );
        let detector_block = fg.add_block(detector);

        let head = Head::<Complex32>::new(100000);
        let head_block = fg.add_block(head);

        let zc_sink = FileSink::<Complex32>::new("zc_output.bin");
        let zc_sink_block = fg.add_block(zc_sink);

        let gold_sink = FileSink::<Complex32>::new("gold_output.bin");
        let gold_sink_block = fg.add_block(gold_sink);

        fg.connect_stream(src_block, "out", head_block, "in")?;
        fg.connect_stream(head_block, "out", detector_block, "in")?;
        fg.connect_stream(detector_block, "zc", zc_sink_block, "in")?;
        fg.connect_stream(detector_block, "gold", gold_sink_block, "in")?;

        let start_time = Instant::now();
        Runtime::new().run(fg)?;
        let duration = start_time.elapsed();

        println!("Ausführungszeit des Graphen: {:?}", duration);

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

        for window in all_positions.windows(2) {
            let index1: usize = window[0].split(',').next().unwrap().parse().unwrap();
            let index2: usize = window[1].split(',').next().unwrap().parse().unwrap();
            assert_eq!(
                index2 - index1,
                1186,
                "Distance between detections should be 1186"
            );
        }

        std::fs::remove_file("zc_output.bin")?;
        std::fs::remove_file("gold_output.bin")?;

        Ok(())
    }
}
