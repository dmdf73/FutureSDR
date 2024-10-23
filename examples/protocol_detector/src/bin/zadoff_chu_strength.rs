use num_complex::Complex32;
use protocol_detector::generate_zadoff_chu;

fn calculate_signal_strength(sequence: &[Complex32]) -> f32 {
    sequence.iter().map(|&c| c.norm_sqr()).sum::<f32>() / sequence.len() as f32
}

fn main() {
    let sequence_length = 64;
    let root = 11;

    let sequence = generate_zadoff_chu(root, sequence_length, 0, 0);
    let signal_strength = calculate_signal_strength(&sequence);

    println!("Zadoff-Chu Sequence:");
    for (i, value) in sequence.iter().enumerate() {
        println!("{}: {}", i, value);
    }
    println!(
        "
Signal Strength: {}",
        signal_strength
    );
}
