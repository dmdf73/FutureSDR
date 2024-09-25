use futuresdr::anyhow::Result;
use futuresdr::macros::async_trait;
use futuresdr::num_complex::Complex32;
use futuresdr::runtime::Block;
use futuresdr::runtime::BlockMeta;
use futuresdr::runtime::BlockMetaBuilder;
use futuresdr::runtime::ItemTag;
use futuresdr::runtime::Kernel;
use futuresdr::runtime::MessageIo;
use futuresdr::runtime::MessageIoBuilder;
use futuresdr::runtime::StreamIo;
use futuresdr::runtime::StreamIoBuilder;
use futuresdr::runtime::Tag;
use futuresdr::runtime::WorkIo;
#[allow(dead_code)]
pub struct ZcDetect {}

impl ZcDetect {
    pub fn new() -> Block {
        Block::new(
            BlockMetaBuilder::new("ZcDetect").build(),
            StreamIoBuilder::new()
                .add_input::<Complex32>("in")
                .add_output::<Complex32>("out")
                .build(),
            MessageIoBuilder::new().build(),
            ZcDetect {},
        )
    }
}

fn normalized_dot_product(seq1: &[Complex32], seq2: &[Complex32]) -> f32 {
    // Ensure the slices have the same length
    assert_eq!(
        seq1.len(),
        seq2.len(),
        "sequence 1 and sequence 2 of different size for dot product"
    );

    // Calculate the norms of the sequences
    let norm_seq1 = seq1.iter().map(|x| x.norm()).sum::<f32>().sqrt();
    let norm_seq2 = seq2.iter().map(|x| x.norm()).sum::<f32>().sqrt();

    // Calculate the dot product
    let sum_val: Complex32 = seq1
        .iter()
        .zip(seq2.iter())
        .map(|(a, b)| *a * b.conj())
        .sum();

    // Calculate the normalized dot product
    return (sum_val / (norm_seq1 * norm_seq2)).norm();
}

#[async_trait]
impl Kernel for ZcDetect {
    async fn work(
        &mut self,
        io: &mut WorkIo,
        sio: &mut StreamIo,
        _m: &mut MessageIo<Self>,
        _b: &mut BlockMeta,
    ) -> Result<()> {
        let input = sio.input(0).slice::<Complex32>();
        let output = sio.output(0).slice::<Complex32>();
        let len = input.len();
        // wait till enough room in output buffer + input buffer filled
        if len >= SYNC_LENGTH {
            let mut x = 0;
            while (len - x) >= SYNC_LENGTH {
                // test correlation for LORA or WIFI
                let corr_wifi = normalized_dot_product(&input[x..x + SYNC_LENGTH], &WIFI);
                //dbg!(corr);
                if corr_wifi > 0.5 {
                    println!("WIFI:, corr={}, x={}", corr_wifi, x);
                    sio.output(0)
                        .add_tag(x, Tag::String("ZC Start".to_string()));
                }
                let corr_lora = normalized_dot_product(&input[x..x + SYNC_LENGTH], &LORA);
                //dbg!(corr);
                if corr_lora > 0.5 {
                    println!("LORA:, corr={}, x={}", corr_lora, x);
                    sio.output(0)
                        .add_tag(x, Tag::String("ZC Start".to_string()));
                }
                x = x + 1;
            }
            // finished, giving our results to the flowgraph
            sio.input(0).consume(len);
            output[0..len].copy_from_slice(input);
            sio.output(0).produce(len);

            if sio.input(0).finished() {
                io.finished = true;
            }
        } else if sio.input(0).finished() {
            sio.input(0).consume(len);
            output[0..len].copy_from_slice(input);
            sio.output(0).produce(len);
            io.finished = true;
        }

        Ok(())
    }
}

const SYNC_LENGTH: usize = 53;

// u = 3
const WIFI: [Complex32; 53] = [
    Complex32::new(1.0, 0.0),
    Complex32::new(0.9374196611341209, -0.34820163543439875),
    Complex32::new(0.48279220273074486, -0.8757349421956369),
    Complex32::new(-0.5338233779647907, -0.8455960035018261),
    Complex32::new(-0.9151456172430188, 0.40312342928797157),
    Complex32::new(0.5829794791144711, 0.812486878005682),
    Complex32::new(0.37582758211423845, -0.9266896074318333),
    Complex32::new(-0.861043611767356, 0.5085311186492196),
    Complex32::new(0.9720229140804112, -0.23488604578098216),
    Complex32::new(-0.956400984276523, 0.29205677063697394),
    Complex32::new(0.7575112421616227, -0.6528221181905186),
    Complex32::new(-0.0887958953229353, 0.9960498426152169),
    Complex32::new(-0.8610436117673543, -0.5085311186492224),
    Complex32::new(0.5829794791144766, -0.812486878005678),
    Complex32::new(0.9374196611341209, 0.34820163543439864),
    Complex32::new(0.2635871660690649, 0.9646355819083594),
    Complex32::new(-0.32026985386284473, 0.947326353854189),
    Complex32::new(-0.5338233779647963, 0.8455960035018225),
    Complex32::new(-0.4300652022765272, 0.9027978299657403),
    Complex32::new(0.02963332782254987, 0.9995608365087947),
    Complex32::new(0.757511242161611, 0.6528221181905322),
    Complex32::new(0.8896570909947464, -0.45662923739371464),
    Complex32::new(-0.43006520227651546, -0.9027978299657459),
    Complex32::new(-0.7175072570443461, 0.6965510290629816),
    Complex32::new(0.9929810960135166, 0.11827317092136941),
    Complex32::new(-0.794854441413348, -0.6068001458186002),
    Complex32::new(0.6749830015182045, 0.7378332790417328),
    Complex32::new(-0.7948544414133518, -0.6068001458185952),
    Complex32::new(0.992981096013518, 0.11827317092135675),
    Complex32::new(-0.7175072570443427, 0.696551029062985),
    Complex32::new(-0.4300652022765, -0.9027978299657533),
    Complex32::new(0.8896570909947644, -0.4566292373936798),
    Complex32::new(0.7575112421616174, 0.6528221181905247),
    Complex32::new(0.029633327822551833, 0.9995608365087946),
    Complex32::new(-0.4300652022765646, 0.9027978299657224),
    Complex32::new(-0.5338233779647779, 0.8455960035018342),
    Complex32::new(-0.3202698538628517, 0.9473263538541866),
    Complex32::new(0.26358716606904337, 0.9646355819083653),
    Complex32::new(0.9374196611341055, 0.3482016354344402),
    Complex32::new(0.5829794791145074, -0.8124868780056559),
    Complex32::new(-0.8610436117673292, -0.5085311186492649),
    Complex32::new(-0.08879589532294628, 0.9960498426152159),
    Complex32::new(0.7575112421616442, -0.6528221181904936),
    Complex32::new(-0.9564009842765433, 0.2920567706369076),
    Complex32::new(0.972022914080411, -0.2348860457809831),
    Complex32::new(-0.8610436117673821, 0.5085311186491754),
    Complex32::new(0.3758275821142373, -0.9266896074318338),
    Complex32::new(0.5829794791144167, 0.8124868780057211),
    Complex32::new(-0.9151456172430316, 0.4031234292879426),
    Complex32::new(-0.5338233779647862, -0.845596003501829),
    Complex32::new(0.48279220273078327, -0.8757349421956157),
    Complex32::new(0.937419661134113, -0.34820163543442),
    Complex32::new(1.0, 3.331534478190071e-14),
];

// u = 7
const LORA: [Complex32; 53] = [
    Complex32::new(1.0, 0.0),
    Complex32::new(0.6749830015182106, -0.7378332790417272),
    Complex32::new(-0.7948544414133532, -0.6068001458185935),
    Complex32::new(0.2635871660690671, 0.9646355819083589),
    Complex32::new(-0.43006520227651995, -0.9027978299657438),
    Complex32::new(0.9929810960135169, 0.11827317092136576),
    Complex32::new(0.14764656400247989, 0.9890401873221641),
    Complex32::new(-0.32026985386284046, 0.9473263538541904),
    Complex32::new(0.029633327822558442, 0.9995608365087943),
    Complex32::new(0.9374196611341209, 0.34820163543439864),
    Complex32::new(-0.08879589532292555, -0.9960498426152178),
    Complex32::new(-0.20597861874109955, 0.9785564922995038),
    Complex32::new(-0.32026985386284057, -0.9473263538541904),
    Complex32::new(0.9929810960135182, -0.1182731709213553),
    Complex32::new(0.6749830015182056, 0.7378332790417318),
    Complex32::new(0.5829794791144725, 0.8124868780056811),
    Complex32::new(0.9720229140804066, 0.23488604578100072),
    Complex32::new(0.26358716606907123, -0.9646355819083576),
    Complex32::new(-0.8610436117673487, 0.5085311186492321),
    Complex32::new(0.8294056854501988, -0.5586467658036569),
    Complex32::new(-0.08879589532294824, 0.9960498426152157),
    Complex32::new(-0.9982437317643206, 0.05924062789372784),
    Complex32::new(-0.8610436117673562, -0.5085311186492193),
    Complex32::new(-0.9564009842765161, -0.29205677063699664),
    Complex32::new(-0.7175072570443304, 0.6965510290629977),
    Complex32::new(0.8896570909947272, 0.4566292373937521),
    Complex32::new(-0.6300878435816778, -0.7765238627180694),
    Complex32::new(0.8896570909947621, 0.4566292373936841),
    Complex32::new(-0.7175072570444021, 0.6965510290629239),
    Complex32::new(-0.9564009842765083, -0.29205677063702196),
    Complex32::new(-0.8610436117673382, -0.5085311186492497),
    Complex32::new(-0.9982437317643249, 0.059240627893655445),
    Complex32::new(-0.08879589532294434, 0.9960498426152161),
    Complex32::new(0.8294056854502015, -0.5586467658036529),
    Complex32::new(-0.8610436117673991, 0.5085311186491468),
    Complex32::new(0.26358716606905186, -0.964635581908363),
    Complex32::new(0.9720229140804026, 0.2348860457810174),
    Complex32::new(0.5829794791144629, 0.8124868780056879),
    Complex32::new(0.6749830015181904, 0.7378332790417457),
    Complex32::new(0.9929810960135267, -0.11827317092128377),
    Complex32::new(-0.32026985386277745, -0.9473263538542117),
    Complex32::new(-0.20597861874118034, 0.9785564922994867),
    Complex32::new(-0.0887958953229053, -0.9960498426152196),
    Complex32::new(0.9374196611340762, 0.3482016354345192),
    Complex32::new(0.029633327822520494, 0.9995608365087955),
    Complex32::new(-0.3202698538628276, 0.9473263538541948),
    Complex32::new(0.14764656400238976, 0.9890401873221777),
    Complex32::new(0.992981096013516, 0.11827317092137328),
    Complex32::new(-0.43006520227655487, -0.9027978299657271),
    Complex32::new(0.26358716606900084, 0.964635581908377),
    Complex32::new(-0.7948544414132223, -0.606800145818765),
    Complex32::new(0.6749830015182554, -0.7378332790416862),
    Complex32::new(1.0, 5.878799820416566e-14),
];
