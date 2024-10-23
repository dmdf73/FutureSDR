use futuresdr::anyhow::Result;
use futuresdr::blocks::{FileSink, Head, Source};
use futuresdr::num_complex::Complex32;
use futuresdr::runtime::buffer::circular::Circular;
use futuresdr::runtime::Flowgraph;
use futuresdr::runtime::Runtime;
use protocol_detector::{
    MultiPortInserter, Protocol, ProtocolDetector, Sequence, SimpleTagInserter,
};
use std::fs::File;
use std::io::{BufWriter, Write};
use std::process::exit;

fn write_complex32_to_file(filename: &str, data: &[Complex32]) -> Result<()> {
    let file = File::create(filename)?;
    let mut writer = BufWriter::new(file);
    for &complex in data {
        writeln!(writer, "{},{}", complex.re, complex.im)?;
    }
    writer.flush()?;
    Ok(())
}

#[test]
fn test_multi_protocol_detection_2() -> Result<()> {
    let mut fg = Flowgraph::new();

    // Protokolle definieren
    let protocols = vec![
        Protocol {
            name: "zc".to_string(),
            sequence: Sequence::new(vec![Complex32::new(1.0, 0.0); 63], 1.0),
            sequences: vec![Sequence::new(vec![Complex32::new(1.0, 0.0); 63], 1.)],
        },
        Protocol {
            name: "lora".to_string(),
            sequence: Sequence::new(vec![Complex32::new(0.0, 1.0); 63], 1.0),
            sequences: vec![Sequence::new(vec![Complex32::new(0.0, 1.0); 63], 1.)],
        },
    ];

    // Quellen und Head-Blöcke erstellen
    let zc_source = fg.add_block(Source::new(|| Complex32::new(-1.0, -1.0)));
    let lora_source = fg.add_block(Source::new(|| Complex32::new(1.0, 1.0)));
    let zc_head = fg.add_block(Head::<Complex32>::new(1000));
    let lora_head = fg.add_block(Head::<Complex32>::new(1000));

    // Tag-Einfüge-Blöcke erstellen
    let zc_tag_inserter = SimpleTagInserter::new(100, vec!["zc_tag".to_string()]);
    let lora_tag_inserter = SimpleTagInserter::new(100, vec!["lora_tag".to_string()]);
    let zc_tag_inserter_block = fg.add_block(zc_tag_inserter);
    let lora_tag_inserter_block = fg.add_block(lora_tag_inserter);

    // MultiPortInserter erstellen
    let ports = vec![
        ("zc".to_string(), "zc_tag".to_string()),
        ("lora".to_string(), "lora_tag".to_string()),
    ];
    let sequences = vec![
        vec![Complex32::new(1.0, 0.0); 63], // ZC Sequenz
        vec![Complex32::new(0.0, 1.0); 63], // LoRa Sequenz
    ];
    let pad_front = 2;
    let pad_back = 2;
    let multi_port_inserter = MultiPortInserter::new(ports, sequences, pad_front, pad_back);
    let multi_port_inserter_block = fg.add_block(multi_port_inserter);

    // ProtocolDetector erstellen
    let detector = ProtocolDetector::new(protocols, None, true, Some("multi_PROT.log".to_owned()));
    let detector_block = fg.add_block(detector);

    // Sink-Blöcke erstellen
    let zc_sink = FileSink::<Complex32>::new("zc_output.bin");
    let zc_sink_block = fg.add_block(zc_sink);
    let lora_sink = FileSink::<Complex32>::new("lora_output.bin");
    let lora_sink_block = fg.add_block(lora_sink);

    // Neuer FileSink für die Ausgabe des MultiPortInserter
    let raw_sink = FileSink::<Complex32>::new("raw_output.bin");
    let raw_sink_block = fg.add_block(raw_sink);

    // Verbindungen herstellen
    fg.connect_stream(zc_source, "out", zc_head, "in")?;
    fg.connect_stream(lora_source, "out", lora_head, "in")?;
    fg.connect_stream(zc_head, "out", zc_tag_inserter_block, "in")?;
    fg.connect_stream(lora_head, "out", lora_tag_inserter_block, "in")?;
    fg.connect_stream(
        zc_tag_inserter_block,
        "out",
        multi_port_inserter_block,
        "zc",
    )?;
    fg.connect_stream(
        lora_tag_inserter_block,
        "out",
        multi_port_inserter_block,
        "lora",
    )?;
    fg.connect_stream(multi_port_inserter_block, "out", raw_sink_block, "in")?;
    // fg.connect_stream_with_type(
    //     multi_port_inserter_block,
    //     "out",
    //     detector_block,
    //     "in",
    //     Circular::with_size(200000),
    // )?;
    fg.connect_stream(multi_port_inserter_block, "out", detector_block, "in")?;
    fg.connect_stream(detector_block, "zc", zc_sink_block, "in")?;
    fg.connect_stream(detector_block, "lora", lora_sink_block, "in")?;

    Runtime::new().run(fg)?;

    // Daten aus Dateien in Vektoren laden
    let zc_samples: Vec<Complex32> = protocol_detector::load_complex32_from_file("zc_output.bin")?
        .into_iter()
        .filter(|&c| c != Complex32::new(0.0, 0.0))
        .collect();
    let lora_samples: Vec<Complex32> =
        protocol_detector::load_complex32_from_file("lora_output.bin")?
            .into_iter()
            .filter(|&c| c != Complex32::new(0.0, 0.0))
            .collect();

    // Rohe Ausgabe des MultiPortInserter lesen und in menschenlesbares Format konvertieren
    let raw_samples = protocol_detector::load_complex32_from_file("raw_output.bin")?;
    write_complex32_to_file("raw_output.txt", &raw_samples)?;

    // Erwartete Ausgaben definieren
    let sequence_length = 63;
    let source_samples = 100;
    let repeat_count = 10;

    let zc_expected: Vec<Complex32> = (0..repeat_count)
        .flat_map(|_| {
            vec![
                vec![Complex32::new(1.0, 0.0); sequence_length],
                vec![Complex32::new(-1.0, -1.0); source_samples],
            ]
            .into_iter()
            .flatten()
        })
        .collect();

    let lora_expected: Vec<Complex32> = (0..repeat_count)
        .flat_map(|_| {
            vec![
                vec![Complex32::new(0.0, 1.0); sequence_length],
                vec![Complex32::new(1.0, 1.0); source_samples],
            ]
            .into_iter()
            .flatten()
        })
        .collect();

    write_complex32_to_file("ZC_expected.txt", &zc_expected)?;
    write_complex32_to_file("zc_vector.txt", &zc_samples)?;
    write_complex32_to_file("lora_vector.txt", &lora_samples)?;
    write_complex32_to_file("lora_expected.txt", &lora_expected)?;
    // exit(1);

    // Auskommentierte Assertions
    // assert_eq!(
    //     zc_samples, zc_expected,
    //     "ZC Ausgabe stimmt nicht mit der erwarteten Ausgabe überein"
    // );
    assert_eq!(
        lora_samples, lora_expected,
        "LoRa Ausgabe stimmt nicht mit der erwarteten Ausgabe überein"
    );

    Ok(())
}
