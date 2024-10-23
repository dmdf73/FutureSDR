use clap::Parser;
use futuresdr::anyhow::Result;
use futuresdr::blocks::*;
use futuresdr::num_complex::Complex32;
use futuresdr::runtime::buffer::circular::Circular;
use futuresdr::runtime::Flowgraph;
use futuresdr::runtime::Runtime;
use protocol_detector::ProtocolDetector;
use protocol_detector::{generate_zadoff_chu, Protocol, ProtocolDetectorFFT, Sequence};

#[derive(Parser, Debug)]
#[clap(version)]
struct Args {
    #[clap(long, default_value = "output_after_noise.bin")]
    input_file: String,
    #[clap(long, default_value_t = 64)]
    sequence_length: u32,
    #[clap(long, default_value = "11")]
    sync_root: u32,
    #[clap(long, default_value = "17")]
    wifi_root: u32,
    #[clap(long, default_value = "23")]
    lora_root: u32,
    #[clap(long, default_value = "25")]
    zigbee_root: u32,
    #[clap(long, default_value_t = false)]
    use_fft: bool,
    #[clap(long, default_value_t = 0.7)]
    threshold: f32,
}

fn main() -> Result<()> {
    let args = Args::parse();

    let mut fg = Flowgraph::new();

    // FileSource zum Lesen der Daten
    let source = fg.add_block(FileSource::<Complex32>::new(&args.input_file, false));
    let threshold = args.threshold;
    // Protokollerkennung vorbereiten
    // let sync_sequence = generate_zadoff_chu(23, args.sequence_length, 0, 0);
    // let wifi_sequence = generate_zadoff_chu(13, args.sequence_length, 0, 100);
    // let lora_sequence = generate_zadoff_chu(17, args.sequence_length, 0, 200);
    // let zigbee_sequence = generate_zadoff_chu(19, args.sequence_length, 0, 300);
    let sync_sequence = generate_zadoff_chu(args.sync_root, args.sequence_length, 0, 0);
    let wifi_sequence = generate_zadoff_chu(args.wifi_root, args.sequence_length, 0, 0);
    let lora_sequence = generate_zadoff_chu(args.lora_root, args.sequence_length, 0, 0);
    let zigbee_sequence = generate_zadoff_chu(args.zigbee_root, args.sequence_length, 0, 0);

    let wifi_protocol = Protocol {
        name: "wifi".to_string(),
        sequence: Sequence::new(
            [sync_sequence.clone(), wifi_sequence.clone()].concat(),
            threshold,
        ),
        sequences: vec![Sequence::new(wifi_sequence, threshold)],
    };

    let lora_protocol = Protocol {
        name: "lora".to_string(),
        sequence: Sequence::new(
            [sync_sequence.clone(), lora_sequence.clone()].concat(),
            threshold,
        ),
        sequences: vec![Sequence::new(lora_sequence, threshold)],
    };

    let zigbee_protocol = Protocol {
        name: "zigbee".to_string(),
        sequence: Sequence::new(
            [sync_sequence.clone(), zigbee_sequence.clone()].concat(),
            threshold,
        ),
        sequences: vec![Sequence::new(zigbee_sequence, threshold)],
    };
    let protocols_for_detector = vec![wifi_protocol, lora_protocol, zigbee_protocol];

    let detector = if args.use_fft {
        ProtocolDetectorFFT::new(
            Sequence::new(sync_sequence, threshold),
            protocols_for_detector,
            true,
            Some("matches.log".to_owned()),
        )
    } else {
        ProtocolDetector::new(
            protocols_for_detector,
            Some(Sequence::new(sync_sequence, threshold)),
            true,
            std::option::Option::Some("matches.log".to_owned()),
        )
    };
    let detector_block = fg.add_block(detector);
    fg.connect_stream_with_type(
        source,
        "out",
        detector_block,
        "in",
        Circular::with_size(20000000),
    )?;
    let wifi_sink = fg.add_block(NullSink::<Complex32>::new());
    let lora_sink = fg.add_block(NullSink::<Complex32>::new());
    let zigbee_sink = fg.add_block(NullSink::<Complex32>::new());

    fg.connect_stream_with_type(
        detector_block,
        "wifi",
        wifi_sink,
        "in",
        Circular::with_size(20000000),
    )?;
    fg.connect_stream_with_type(
        detector_block,
        "lora",
        lora_sink,
        "in",
        Circular::with_size(20000000),
    )?;
    fg.connect_stream_with_type(
        detector_block,
        "zigbee",
        zigbee_sink,
        "in",
        Circular::with_size(20000000),
    )?;
    // Flowgraph ausführen
    let rt = Runtime::new();
    rt.run(fg)?;

    Ok(())
}
