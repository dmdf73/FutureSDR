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
#[test]
fn test_multi_protocol_detection() -> Result<()> {
    let mut fg = Flowgraph::new();
    let protocols = vec![
        Protocol {
            name: "zc".to_string(),
            sequence: Sequence::new(vec![Complex32::new(1.0, 0.0); 63], 1.0),
            sequences: vec![Sequence::new(vec![Complex32::new(1.0, 0.0); 63], 1.0)],
        },
        Protocol {
            name: "lora".to_string(),
            sequence: Sequence::new(vec![Complex32::new(0.0, 1.0); 63], 1.0),
            sequences: vec![Sequence::new(vec![Complex32::new(0.0, 1.0); 63], 1.0)],
        },
    ];
    let pad_front = 10;
    let pad_tail = 10;
    let src_block = fg.add_block(Source::new(|| Complex32::new(-1.0, -1.0)));
    let tag_inserter = SimpleTagInserter::new(100, vec!["zc".to_string()]);
    let tag_inserter_block = fg.add_block(tag_inserter);
    let inserter = MultiProtocolInserter::new(protocols.clone(), pad_front, pad_tail);
    let inserter_block = fg.add_block(inserter);
    let head = Head::<Complex32>::new(1000);
    let head_block = fg.add_block(head);
    let detector = ProtocolDetector::new(
        protocols,
        None,
        true,
        std::option::Option::Some("prot.log".to_owned()),
    );
    let detector_block = fg.add_block(detector);

    let zc_sink = FileSink::<Complex32>::new("zc_output.bin");
    let zc_sink_block = fg.add_block(zc_sink);
    let lora_sink = FileSink::<Complex32>::new("lora_output.bin");
    let lora_sink_block = fg.add_block(lora_sink);

    fg.connect_stream(src_block, "out", tag_inserter_block, "in")?;
    fg.connect_stream(tag_inserter_block, "out", inserter_block, "in")?;
    fg.connect_stream(inserter_block, "out", head_block, "in")?;
    fg.connect_stream(head_block, "out", detector_block, "in")?;
    fg.connect_stream(detector_block, "zc", zc_sink_block, "in")?;
    fg.connect_stream(detector_block, "lora", lora_sink_block, "in")?;

    Runtime::new().run(fg)?;

    // Laden der Daten aus den Dateien in Vektoren
    let zc_samples = protocol_detector::load_complex32_from_file("zc_output.bin")?;
    let lora_samples = protocol_detector::load_complex32_from_file("lora_output.bin")?;

    assert!(!zc_samples.is_empty(), "ZC sink should have received data");
    assert_eq!(
        zc_samples.len(),
        1000,
        "ZC sink should have received 1000 Complex32 samples"
    );
    assert!(
        lora_samples.is_empty(),
        "LoRa sink should not have received any data"
    );

    let expected_padding = vec![Complex32::new(0.0, 0.0); pad_front];
    assert_eq!(
        &zc_samples[0..pad_front],
        expected_padding,
        "Front padding should be zero"
    );

    let expected_sequence = vec![Complex32::new(1.0, 0.0); 63];
    assert_eq!(
        &zc_samples[pad_front..pad_front + 63],
        expected_sequence,
        "Sequence after padding should match the expected ZC sequence"
    );

    let expected_tail_padding = vec![Complex32::new(0.0, 0.0); pad_tail];
    assert_eq!(
        &zc_samples[pad_front + 63..pad_front + 63 + pad_tail],
        expected_tail_padding,
        "Tail padding should be zero"
    );

    let expected_source = Complex32::new(-1.0, -1.0);
    assert_eq!(
        zc_samples[pad_front + 63 + pad_tail],
        expected_source,
        "First sample after protocol sequence and padding should be from source"
    );

    Ok(())
}
