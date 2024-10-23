use futuresdr::anyhow::Result;
use futuresdr::blocks::{ConsoleSink, Head, NullSink, Source};
use futuresdr::num_complex::Complex32;
use futuresdr::runtime::{Flowgraph, Runtime};
use protocol_detector::{Protocol, Sequence, SimpleTagInserter};
use rand::thread_rng;
use rand_distr::{Distribution, Normal};

// Importieren der Sequenzen und des neuen Detektors
use protocol_detector::sequences::{GOLD_63, ZC_63};
use protocol_detector::MultiProtocolInserter;
use protocol_detector::ProtocolDetector;

fn main() -> Result<()> {
    let mut fg = Flowgraph::new();

    // Quelle für weißes Rauschen
    let std_dev = 1.;
    let normal = Normal::new(0.0f32, std_dev).unwrap();
    let white_noise_generator = move || {
        let mut rng = thread_rng();
        let re = normal.sample(&mut rng);
        let im = normal.sample(&mut rng);
        Complex32::new(re, im)
    };

    let src_block = fg.add_block(Source::new(white_noise_generator));

    // Definition der Protokolle und Sequenzen
    let protocols = vec![
        Protocol {
            name: "zc".to_string(),
            sequence: Sequence::new(ZC_63.to_vec(), 0.7),
            sequences: vec![Sequence::new(ZC_63.to_vec(), 0.7)],
        },
        Protocol {
            name: "lora".to_string(),
            sequence: Sequence::new(GOLD_63.to_vec(), 0.7),
            sequences: vec![Sequence::new(GOLD_63.to_vec(), 0.7)],
        },
    ];

    // Simple Tag Inserter
    let tag_inserter =
        SimpleTagInserter::new(10, protocols.iter().map(|p| p.name.clone()).collect());
    let tag_inserter_block = fg.add_block(tag_inserter);

    // Multi-Protocol Sequence Inserter
    let inserter = MultiProtocolInserter::new(protocols.clone(), 10, 10);
    let inserter_block = fg.add_block(inserter);

    // Flexible Multi-Protocol Detector
    let detector = ProtocolDetector::new(protocols, None, false, None);
    let detector_block = fg.add_block(detector);

    // Head block to limit the number of samples
    let head = Head::<Complex32>::new(100); // Increased sample size
    let head_block = fg.add_block(head);

    // Sinks für jedes Protokoll und einen für nicht erkannte Daten
    let zc_sink = ConsoleSink::<Complex32>::new("\n");
    let zc_sink_block = fg.add_block(zc_sink);

    let lora_sink = ConsoleSink::<Complex32>::new("\n");
    let lora_sink_block = fg.add_block(lora_sink);

    // Verbindungen im Flowgraph
    fg.connect_stream(src_block, "out", tag_inserter_block, "in")?;
    fg.connect_stream(tag_inserter_block, "out", inserter_block, "in")?;
    fg.connect_stream(inserter_block, "out", head_block, "in")?;
    fg.connect_stream(head_block, "out", detector_block, "in")?;
    fg.connect_stream(detector_block, "zc", zc_sink_block, "in")?;
    fg.connect_stream(detector_block, "lora", lora_sink_block, "in")?;

    // Run the flowgraph
    Runtime::new().run(fg)?;

    Ok(())
}
