pub mod gguf;
pub mod kernels;
pub mod pipeline;

#[cfg(test)]
mod tests {
    use super::gguf::inspect_gguf;
    use super::kernels::{
        dot_q4_0, dot_q4_0_scalar, dot_q8_0, dot_q8_0_scalar, matvec_q4_0_parallel,
        matvec_q8_0_parallel, quantize_q4_0, quantize_q8_0, rmsnorm, softmax_inplace, swiglu,
    };
    use super::pipeline::{PulseResponseCache, compress_context};
    use std::io::Write;
    use std::time::Duration;

    #[test]
    fn simd_q8_and_q4_match_scalar_reference() {
        let cols = 256usize;
        let w_f32: Vec<f32> = (0..cols).map(|i| ((i as f32) * 0.13).sin()).collect();
        let x: Vec<f32> = (0..cols).map(|i| ((i as f32) * 0.07).cos()).collect();

        let q8 = quantize_q8_0(&w_f32);
        let scalar8 = dot_q8_0_scalar(&q8, &x);
        let simd8 = dot_q8_0(&q8, &x);
        assert!((scalar8 - simd8).abs() < 1e-3, "q8 mismatch: {scalar8} vs {simd8}");

        let q4 = quantize_q4_0(&w_f32);
        let scalar4 = dot_q4_0_scalar(&q4, &x);
        let simd4 = dot_q4_0(&q4, &x);
        assert!((scalar4 - simd4).abs() < 1e-3, "q4 mismatch: {scalar4} vs {simd4}");

        let rows = 64;
        let mut mat8 = Vec::new();
        let mut mat4 = Vec::new();
        for _ in 0..rows {
            mat8.extend_from_slice(&q8);
            mat4.extend_from_slice(&q4);
        }
        let mut out8 = vec![0.0; rows];
        let mut out4 = vec![0.0; rows];
        matvec_q8_0_parallel(&mat8, rows, cols, &x, &mut out8);
        matvec_q4_0_parallel(&mat4, rows, cols, &x, &mut out4);
        assert!((out8[0] - scalar8).abs() < 1e-3);
        assert!((out4[rows - 1] - scalar4).abs() < 1e-3);
    }

    #[test]
    fn transformer_ops_and_context_compression() {
        let x = [1.0, 2.0, 3.0, 4.0];
        let w = [1.0, 1.0, 1.0, 1.0];
        let mut out = [0.0; 4];
        rmsnorm(&x, &w, 1e-5, &mut out);
        assert!(out[3] > out[0]);

        let mut logits = [1.0, 2.0, 3.0];
        softmax_inplace(&mut logits);
        let sum: f32 = logits.iter().sum();
        assert!((sum - 1.0).abs() < 1e-5);

        let mut act = [0.0; 2];
        swiglu(&[1.0, -1.0], &[2.0, 2.0], &mut act);
        assert!(act[0] > 1.0);

        let raw = "Home\nHome\nHome\nCookie preferences\nArthos is a fast browser built in Rust.\nArthos is a fast browser built in Rust.\n";
        let comp = compress_context(raw, 500);
        assert!(comp.reduction_pct > 0);
        assert!(comp.text.contains("Arthos is a fast browser"));

        let mut cache = PulseResponseCache::new(4, Duration::from_secs(60));
        let k = PulseResponseCache::make_key("local", "m", "summarize", "url1");
        cache.insert(k, "Summary result".into());
        assert_eq!(cache.get(k).as_deref(), Some("Summary result"));
    }

    #[test]
    fn parses_gguf_v3_header() {
        let tmp = std::env::temp_dir().join("arthos_test_model.gguf");
        let mut f = std::fs::File::create(&tmp).unwrap();
        f.write_all(b"GGUF").unwrap();
        f.write_all(&3u32.to_le_bytes()).unwrap(); // version 3
        f.write_all(&12u64.to_le_bytes()).unwrap(); // tensor_count
        f.write_all(&1u64.to_le_bytes()).unwrap(); // kv_count
        let k = b"general.architecture";
        f.write_all(&(k.len() as u64).to_le_bytes()).unwrap();
        f.write_all(k).unwrap();
        f.write_all(&8u32.to_le_bytes()).unwrap(); // string type
        let v = b"llama";
        f.write_all(&(v.len() as u64).to_le_bytes()).unwrap();
        f.write_all(v).unwrap();
        drop(f);

        let summary = inspect_gguf(&tmp).unwrap();
        assert_eq!(summary.version, 3);
        assert_eq!(summary.tensor_count, 12);
        assert_eq!(summary.architecture, "llama");
        let _ = std::fs::remove_file(tmp);
    }
}
