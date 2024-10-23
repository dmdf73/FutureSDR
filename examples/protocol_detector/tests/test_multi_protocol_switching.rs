use futuresdr::anyhow::Result;
use futuresdr::blocks::{FileSink, Head, Source};
use futuresdr::num_complex::Complex32;
use futuresdr::runtime::Flowgraph;
use futuresdr::runtime::Runtime;
use protocol_detector::{
    MultiProtocolInserter, Protocol, ProtocolDetector, Sequence, SimpleTagInserter,
};
use std::fs::File;
use std::io::{BufWriter, Write};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

fn write_complex32_to_file(filename: &str, data: &[Complex32]) -> std::io::Result<()> {
    let file = File::create(filename)?;
    let mut writer = BufWriter::new(file);
    for (i, &c) in data.iter().enumerate() {
        writeln!(writer, "{}: {:.2}{:+.2}j", i, c.re, c.im)?;
    }
    Ok(())
}

#[test]
fn test_multi_protocol_switching() -> Result<()> {
    let mut fg = Flowgraph::new();
    let protocols = vec![
        Protocol {
            name: "zc".to_string(),
            sequence: Sequence::new(vec![Complex32::new(1.0, 0.0); 2], 1.0),
            sequences: vec![Sequence::new(vec![Complex32::new(1.0, 0.0); 2], 1.0)],
        },
        Protocol {
            name: "lora".to_string(),
            sequence: Sequence::new(vec![Complex32::new(0.0, 1.0); 2], 1.0),
            sequences: vec![Sequence::new(vec![Complex32::new(0.0, 1.0); 2], 1.0)],
        },
    ];
    let pad_front = 3;
    let pad_tail = 2;

    let sample_count = Arc::new(AtomicUsize::new(0));
    let sample_count_clone = Arc::clone(&sample_count);

    let src_block = fg.add_block(Source::new(move || {
        let count = sample_count_clone.fetch_add(1, Ordering::SeqCst);
        if (count / 10) % 2 == 0 {
            Complex32::new(-1.0, -1.0) // ZC
        } else {
            Complex32::new(-5.0, -5.0) // LoRa
        }
    }));

    let tag_inserter = SimpleTagInserter::new(10, vec!["zc".to_string(), "lora".to_string()]);
    let tag_inserter_block = fg.add_block(tag_inserter);
    let inserter = MultiProtocolInserter::new(protocols.clone(), pad_front, pad_tail);
    let inserter_block = fg.add_block(inserter);
    let head = Head::<Complex32>::new(120);
    let head_block = fg.add_block(head);

    let head_output_sink = FileSink::<Complex32>::new("head_output.bin");
    let head_output_sink_block = fg.add_block(head_output_sink);

    let detector = ProtocolDetector::new(protocols, None, true, None);
    let detector_block = fg.add_block(detector);

    let zc_sink = FileSink::<Complex32>::new("zc_output_multi.bin");
    let zc_sink_block = fg.add_block(zc_sink);
    let lora_sink = FileSink::<Complex32>::new("lora_output_multi.bin");
    let lora_sink_block = fg.add_block(lora_sink);

    fg.connect_stream(src_block, "out", tag_inserter_block, "in")?;
    fg.connect_stream(tag_inserter_block, "out", inserter_block, "in")?;
    fg.connect_stream(inserter_block, "out", head_block, "in")?;
    fg.connect_stream(head_block, "out", head_output_sink_block, "in")?;
    fg.connect_stream(head_block, "out", detector_block, "in")?;
    fg.connect_stream(detector_block, "zc", zc_sink_block, "in")?;
    fg.connect_stream(detector_block, "lora", lora_sink_block, "in")?;

    Runtime::new().run(fg)?;

    let head_output_samples = protocol_detector::load_complex32_from_file("head_output.bin")?;
    write_complex32_to_file("head_output_readable.txt", &head_output_samples)?;

    let zc_samples = protocol_detector::load_complex32_from_file("zc_output_multi.bin")?;
    write_complex32_to_file("zc_output_readable.txt", &zc_samples)?;

    let lora_samples = protocol_detector::load_complex32_from_file("lora_output_multi.bin")?;
    write_complex32_to_file("lora_output_readable.txt", &lora_samples)?;

    assert!(!zc_samples.is_empty(), "ZC sink should have received data");
    assert!(
        !lora_samples.is_empty(),
        "LoRa sink should have received data"
    );

    let sequence_length = 2;
    let total_insertion_length = pad_front + sequence_length + pad_tail;
    let zc_sequence = vec![Complex32::new(1.0, 0.0); sequence_length];
    let lora_sequence = vec![Complex32::new(0.0, 1.0); sequence_length];

    // ZC (Wi-Fi escape Tasse das widerrufen das widerrufen) Überprüfung
    let mut zc_count = 0;
    let mut expected_zc_start = 0;
    while expected_zc_start < zc_samples.len() {
        // Überprüfe ZC-Sequenz
        if expected_zc_start + pad_front + sequence_length <= zc_samples.len() {
            assert_eq!(
                &zc_samples[expected_zc_start + pad_front
                    ..expected_zc_start + pad_front + sequence_length],
                zc_sequence,
                "ZC sequence should be present at index {}",
                expected_zc_start + pad_front
            );
            zc_count += 1;
        }

        // Überprüfe Quelldaten nach der Sequenz und dem End-Padding
        let source_data_start = expected_zc_start + total_insertion_length;
        for i in source_data_start..source_data_start + 10 {
            if i < zc_samples.len() {
                assert_eq!(
                    zc_samples[i],
                    Complex32::new(-1.0, -1.0),
                    "ZC source data should be -1-1j at index {}",
                    i
                );
            }
        }

        expected_zc_start += total_insertion_length + 10;
    }

    // LoRa Überprüfung
    let mut lora_count = 0;
    let mut expected_lora_start = 0; // LoRa startet nach den ersten 10 ZC Samples
    let mut minus_off = 0;
    while expected_lora_start < lora_samples.len() {
        if expected_lora_start == 0 {
            minus_off = pad_front;
        } else {
            minus_off = 0;
        }
        // Überprüfe LoRa-Sequenz
        if expected_lora_start + pad_front + sequence_length - minus_off <= lora_samples.len() {
            assert_eq!(
                &lora_samples[expected_lora_start + pad_front - minus_off
                    ..expected_lora_start + pad_front + sequence_length - minus_off],
                lora_sequence,
                "LoRa sequence should be present at index {}",
                expected_lora_start + pad_front
            );
            lora_count += 1;
        }

        // Überprüfe Quelldaten nach der Sequenz und dem End-Padding
        let source_data_start = expected_lora_start + total_insertion_length - minus_off;
        for i in source_data_start..source_data_start + 10 {
            if i < lora_samples.len() {
                assert_eq!(
                    lora_samples[i],
                    Complex32::new(-5.0, -5.0),
                    "LoRa source data should be -5-5j at index {}",
                    i
                );
            }
        }

        expected_lora_start += total_insertion_length + 10 - minus_off;
    }

    assert_eq!(zc_count, 4, "Incorrect number of ZC sequences detected");
    assert_eq!(lora_count, 3, "Incorrect number of LoRa sequences detected");

    Ok(())
}
