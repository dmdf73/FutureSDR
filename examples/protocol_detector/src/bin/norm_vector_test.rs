use num_complex::Complex32;
use std::collections::VecDeque;

struct NormVectorCalculator {
    norm_queue: VecDeque<f32>,
    running_sum: f32,
}

impl NormVectorCalculator {
    fn new() -> Self {
        NormVectorCalculator {
            norm_queue: VecDeque::new(),
            running_sum: 0.0,
        }
    }

    fn update_norm_vector(&mut self, window: &[Complex32], sequence_length: usize) -> Vec<f32> {
        let mut norms = Vec::with_capacity(sequence_length);
        let start_index;
        if self.norm_queue.is_empty() {
            let initial_segment = &window[..sequence_length];
            self.running_sum = 0.0;
            for &value in initial_segment {
                let norm = value.norm_sqr();
                self.norm_queue.push_back(norm);
                self.running_sum += norm;
            }
            norms.push(self.running_sum);
            start_index = 1;
        } else {
            start_index = 0;
        }
        for i in start_index..(window.len() - sequence_length + 1) {
            let new_val = window[i + sequence_length - 1].norm_sqr();
            if let Some(old_val) = self.norm_queue.pop_front() {
                self.running_sum = self.running_sum - old_val + new_val;
            }
            self.norm_queue.push_back(new_val);
            norms.push(self.running_sum);
        }
        norms.iter().map(|&sum| sum.sqrt()).collect()
    }
}

fn main() {
    let test_sequence = vec![
        Complex32::new(0.0, 0.0),
        Complex32::new(0.0, 0.0),
        Complex32::new(1.0, 1.0),
        Complex32::new(1.0, 1.0),
        Complex32::new(0.0, 1.0),
        Complex32::new(0.0, 1.0),
        Complex32::new(0.0, 0.0),
        Complex32::new(0.0, 0.0),
        Complex32::new(-5.0, -5.0),
        Complex32::new(-5.0, -5.0),
    ];

    let mut calculator = NormVectorCalculator::new();
    let sequence_length = 2;
    let result = calculator.update_norm_vector(&test_sequence, sequence_length);

    println!("Eingabesequenz:");
    for (i, &complex) in test_sequence.iter().enumerate() {
        println!("{}: ({}, {})", i, complex.re, complex.im);
    }

    println!("\nErgebnis (Norm-Vektor):");
    for (i, &norm) in result.iter().enumerate() {
        println!("{}: {:.6}", i, norm);
    }
}
