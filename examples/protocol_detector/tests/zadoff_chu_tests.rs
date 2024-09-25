use num_complex::Complex;
use std::f64::consts::PI;

pub fn generate_zadoff_chu(u: u32, n: u32, q: u32) -> Vec<Complex<f64>> {
    if u == 0 || u >= n {
        panic!("u must be in the range 1 <= u < n");
    }
    if gcd(u, n) != 1 {
        panic!("u and n must be coprime");
    }

    let cf = n % 2;
    (0..n)
        .map(|k| {
            let k = k as f64;
            let n = n as f64;
            let u = u as f64;
            let q = q as f64;
            let cf = cf as f64;
            let exponent = -PI * u * k * (k + cf + 2.0 * q) / n;
            Complex::new(0.0, exponent).exp()
        })
        .collect()
}

fn gcd(a: u32, b: u32) -> u32 {
    if b == 0 {
        a
    } else {
        gcd(b, a % b)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use num_complex::Complex;

    #[test]
    fn test_generate_zadoff_chu() {
        let u = 25;
        let n = 139;
        let q = 0;

        // Erwartete Sequenz, basierend auf der Python-Ausgabe
        let expected_sequence = vec![
            Complex::new(1.0, 0.0),
            Complex::new(0.426597, -0.904442),
            Complex::new(-0.969254, 0.246062),
            Complex::new(0.878907, -0.476993),
            Complex::new(0.300406, 0.953811),
        ];

        // Generierte Sequenz mit der Rust-Funktion
        let generated_sequence = generate_zadoff_chu(u, n, q);

        // Überprüfen Sie, ob die generierte Sequenz die erwartete Länge hat
        assert_eq!(
            139,
            generated_sequence.len(),
            "Generated sequence length does not match expected length of 139"
        );

        // Überprüfen, dass die generierte Sequenz länger ist als die erwartete
        assert!(
            generated_sequence.len() > expected_sequence.len(),
            "Generated sequence should be longer than the expected test sequence"
        );

        // Überprüfen der Sequenzen auf die ersten fünf Elemente
        for (i, (expected, generated)) in expected_sequence
            .iter()
            .zip(generated_sequence.iter())
            .enumerate()
        {
            let real_diff = (expected.re - generated.re).abs();
            let imag_diff = (expected.im - generated.im).abs();
            assert!(
                real_diff < 0.001 && imag_diff < 0.001,
                "Mismatch at index {}: expected {:?}, got {:?} (real diff: {}, imag diff: {})",
                i,
                expected,
                generated,
                real_diff,
                imag_diff
            );
        }
    }
}
