use rustfft::num_complex::Complex;
use rustfft::FftPlanner;

fn fft_corr(window: &[Complex<f32>], sequence: &[f32]) -> Vec<Complex<f32>> {
    let n = window.len();
    let mut planner = FftPlanner::new();
    let fft = planner.plan_fft_forward(n);
    let ifft = planner.plan_fft_inverse(n);
    let mut window_fft: Vec<Complex<f32>> =
        window.iter().map(|&c| Complex::new(c.re, c.im)).collect();
    fft.process(&mut window_fft);
    let mut sequence_fft: Vec<Complex<f32>> =
        sequence.iter().map(|&x| Complex::new(x, 0.0)).collect();
    fft.process(&mut sequence_fft);
    for (mut w, s) in window_fft.iter_mut().zip(sequence_fft.iter()) {
        *w *= s.conj();
    }
    ifft.process(&mut window_fft);

    // Scaling
    for val in window_fft.iter_mut() {
        *val /= n as f32;
    }

    let sequence_norm: f32 = sequence.iter().map(|x| x.powi(2)).sum::<f32>().sqrt();
    let window_norm: f32 = window.iter().map(|x| x.norm_sqr()).sum::<f32>().sqrt();
    println!("Window norm: {}", window_norm);
    println!("Sequence norm: {}", sequence_norm);
    println!("Product of norms: {}", window_norm * sequence_norm);
    let mut result: Vec<Complex<f32>> = Vec::new();
    for (i, &x) in window_fft.iter().take(sequence.len()).enumerate() {
        let unnormalized_corr = x;
        let normalized_corr = unnormalized_corr / (window_norm * sequence_norm);
        println!("Index: {}", i);
        println!("  Unnormalized correlation: {}", unnormalized_corr);
        println!("  Normalized correlation: {}", normalized_corr);
        result.push(unnormalized_corr);
    }
    result
}

fn main() {
    let window = vec![
        Complex::new(1.0, 0.0),
        Complex::new(2.0, 0.0),
        Complex::new(3.0, 0.0),
        Complex::new(4.0, 0.0),
    ];
    let sequence = vec![1.0, 1.0, 0.0, 0.0];
    let result = fft_corr(&window, &sequence);
    println!(
        "Correlation values (unnormalized, but scaled): {:?}",
        result
    );
}
