use serde::{Deserialize, Serialize};
use std::time::Instant;

pub const QK8_0: usize = 32;
pub const QK4_0: usize = 32;

/// 32-weight 8-bit quantized block (matches GGUF Q8_0 layout).
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct BlockQ8_0 {
    pub d: u16, // IEEE 754 binary16 scale
    pub qs: [i8; QK8_0],
}

/// 32-weight 4-bit quantized block (matches GGUF Q4_0 layout).
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct BlockQ4_0 {
    pub d: u16, // IEEE 754 binary16 scale
    pub qs: [u8; QK4_0 / 2],
}

#[inline]
pub fn f16_to_f32(bits: u16) -> f32 {
    let sign = ((bits & 0x8000) as u32) << 16;
    let exp = (bits & 0x7C00) >> 10;
    let mant = (bits & 0x03FF) as u32;

    let f_bits = if exp == 0 {
        if mant == 0 {
            sign
        } else {
            let mut m = mant;
            let mut e = 127i32 - 14;
            while (m & 0x0400) == 0 {
                m <<= 1;
                e -= 1;
            }
            m &= 0x03FF;
            sign | ((e as u32) << 23) | (m << 13)
        }
    } else if exp == 0x1F {
        sign | 0x7F80_0000 | (mant << 13)
    } else {
        let e = (exp as u32) + (127 - 15);
        sign | (e << 23) | (mant << 13)
    };
    f32::from_bits(f_bits)
}

#[inline]
pub fn f32_to_f16(val: f32) -> u16 {
    let bits = val.to_bits();
    let sign = ((bits >> 16) & 0x8000) as u16;
    let exp = ((bits >> 23) & 0xFF) as i32;
    let mant = bits & 0x007F_FFFF;

    if exp == 0xFF {
        return sign | 0x7C00 | if mant != 0 { 0x0200 } else { 0 };
    }
    let new_exp = exp - 127 + 15;
    if new_exp >= 31 {
        return sign | 0x7C00;
    }
    if new_exp <= 0 {
        if new_exp < -10 {
            return sign;
        }
        let m = (mant | 0x0080_0000) >> (1 - new_exp + 13);
        return sign | (m as u16);
    }
    sign | ((new_exp as u16) << 10) | ((mant >> 13) as u16)
}

/// Quantize a slice of f32 values into Q8_0 blocks. Length must be a multiple of 32.
pub fn quantize_q8_0(input: &[f32]) -> Vec<BlockQ8_0> {
    assert_eq!(input.len() % QK8_0, 0, "input length must be multiple of 32");
    input
        .chunks_exact(QK8_0)
        .map(|chunk| {
            let max_abs = chunk.iter().fold(0.0f32, |m, &v| m.max(v.abs()));
            let d = max_abs / 127.0;
            let inv_d = if d > 0.0 { 1.0 / d } else { 0.0 };
            let mut qs = [0i8; QK8_0];
            for (dst, &src) in qs.iter_mut().zip(chunk.iter()) {
                *dst = (src * inv_d).round().clamp(-127.0, 127.0) as i8;
            }
            BlockQ8_0 {
                d: f32_to_f16(d),
                qs,
            }
        })
        .collect()
}

/// Quantize a slice of f32 values into Q4_0 blocks. Length must be a multiple of 32.
pub fn quantize_q4_0(input: &[f32]) -> Vec<BlockQ4_0> {
    assert_eq!(input.len() % QK4_0, 0, "input length must be multiple of 32");
    input
        .chunks_exact(QK4_0)
        .map(|chunk| {
            let max_abs = chunk.iter().fold(0.0f32, |m, &v| m.max(v.abs()));
            let d = max_abs / 7.0;
            let inv_d = if d > 0.0 { 1.0 / d } else { 0.0 };
            let mut qs = [0u8; QK4_0 / 2];
            for i in 0..16 {
                let q0 = ((chunk[i] * inv_d).round().clamp(-8.0, 7.0) as i32 + 8) as u8;
                let q1 = ((chunk[i + 16] * inv_d).round().clamp(-8.0, 7.0) as i32 + 8) as u8;
                qs[i] = (q0 & 0x0F) | ((q1 & 0x0F) << 4);
            }
            BlockQ4_0 {
                d: f32_to_f16(d),
                qs,
            }
        })
        .collect()
}

#[inline]
pub fn dot_q8_0_scalar(weights: &[BlockQ8_0], x: &[f32]) -> f32 {
    debug_assert_eq!(weights.len() * QK8_0, x.len());
    let mut total = 0.0f32;
    for (blk, x_chunk) in weights.iter().zip(x.chunks_exact(QK8_0)) {
        let d = f16_to_f32(blk.d);
        let mut sum = 0.0f32;
        for i in 0..QK8_0 {
            sum += (blk.qs[i] as f32) * x_chunk[i];
        }
        total += sum * d;
    }
    total
}

#[inline]
pub fn dot_q4_0_scalar(weights: &[BlockQ4_0], x: &[f32]) -> f32 {
    debug_assert_eq!(weights.len() * QK4_0, x.len());
    let mut total = 0.0f32;
    for (blk, x_chunk) in weights.iter().zip(x.chunks_exact(QK4_0)) {
        let d = f16_to_f32(blk.d);
        let mut sum = 0.0f32;
        for i in 0..16 {
            let byte = blk.qs[i];
            let q0 = ((byte & 0x0F) as i32 - 8) as f32;
            let q1 = (((byte >> 4) & 0x0F) as i32 - 8) as f32;
            sum += q0 * x_chunk[i] + q1 * x_chunk[i + 16];
        }
        total += sum * d;
    }
    total
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2", enable = "fma")]
unsafe fn dot_q8_0_avx2(weights: &[BlockQ8_0], x: &[f32]) -> f32 {
    use std::arch::x86_64::*;

    let mut acc = _mm256_setzero_ps();
    let x_ptr = x.as_ptr();

    for (b_idx, blk) in weights.iter().enumerate() {
        let d = f16_to_f32(blk.d);
        let d_vec = _mm256_set1_ps(d);
        let q_ptr = blk.qs.as_ptr();
        let xp = unsafe { x_ptr.add(b_idx * QK8_0) };

        let mut blk_acc = _mm256_setzero_ps();
        for lane in 0..4 {
            let q8 = unsafe { _mm_loadl_epi64(q_ptr.add(lane * 8) as *const __m128i) };
            let q32 = _mm256_cvtepi8_epi32(q8);
            let qf = _mm256_cvtepi32_ps(q32);
            let xv = unsafe { _mm256_loadu_ps(xp.add(lane * 8)) };
            blk_acc = _mm256_fmadd_ps(qf, xv, blk_acc);
        }
        acc = _mm256_fmadd_ps(blk_acc, d_vec, acc);
    }

    // Horizontal sum of 8 floats
    let hi = _mm256_extractf128_ps(acc, 1);
    let lo = _mm256_castps256_ps128(acc);
    let sum128 = _mm_add_ps(lo, hi);
    let shuf = _mm_movehdup_ps(sum128);
    let sums = _mm_add_ps(sum128, shuf);
    let shuf2 = _mm_movehl_ps(sums, sums);
    _mm_cvtss_f32(_mm_add_ss(sums, shuf2))
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2", enable = "fma")]
unsafe fn dot_q4_0_avx2(weights: &[BlockQ4_0], x: &[f32]) -> f32 {
    use std::arch::x86_64::*;

    let mut acc = _mm256_setzero_ps();
    let x_ptr = x.as_ptr();
    let mask_low = _mm_set1_epi8(0x0F);
    let offset_8 = _mm_set1_epi8(8);

    for (b_idx, blk) in weights.iter().enumerate() {
        let d = f16_to_f32(blk.d);
        let d_vec = _mm256_set1_ps(d);
        let raw = unsafe { _mm_loadu_si128(blk.qs.as_ptr() as *const __m128i) };

        let lo_nibbles = _mm_sub_epi8(_mm_and_si128(raw, mask_low), offset_8);
        let hi_nibbles = _mm_sub_epi8(
            _mm_and_si128(_mm_srli_epi16(raw, 4), mask_low),
            offset_8,
        );

        let xp = unsafe { x_ptr.add(b_idx * QK4_0) };
        let mut blk_acc = _mm256_setzero_ps();

        // First 16 weights (low nibbles)
        let lo0_32 = _mm256_cvtepi8_epi32(lo_nibbles);
        let lo1_32 = _mm256_cvtepi8_epi32(_mm_srli_si128(lo_nibbles, 8));
        blk_acc = _mm256_fmadd_ps(_mm256_cvtepi32_ps(lo0_32), unsafe { _mm256_loadu_ps(xp) }, blk_acc);
        blk_acc = _mm256_fmadd_ps(_mm256_cvtepi32_ps(lo1_32), unsafe { _mm256_loadu_ps(xp.add(8)) }, blk_acc);

        // Next 16 weights (high nibbles)
        let hi0_32 = _mm256_cvtepi8_epi32(hi_nibbles);
        let hi1_32 = _mm256_cvtepi8_epi32(_mm_srli_si128(hi_nibbles, 8));
        blk_acc = _mm256_fmadd_ps(_mm256_cvtepi32_ps(hi0_32), unsafe { _mm256_loadu_ps(xp.add(16)) }, blk_acc);
        blk_acc = _mm256_fmadd_ps(_mm256_cvtepi32_ps(hi1_32), unsafe { _mm256_loadu_ps(xp.add(24)) }, blk_acc);

        acc = _mm256_fmadd_ps(blk_acc, d_vec, acc);
    }

    let hi = _mm256_extractf128_ps(acc, 1);
    let lo = _mm256_castps256_ps128(acc);
    let sum128 = _mm_add_ps(lo, hi);
    let shuf = _mm_movehdup_ps(sum128);
    let sums = _mm_add_ps(sum128, shuf);
    let shuf2 = _mm_movehl_ps(sums, sums);
    _mm_cvtss_f32(_mm_add_ss(sums, shuf2))
}

#[inline]
pub fn dot_q8_0(weights: &[BlockQ8_0], x: &[f32]) -> f32 {
    #[cfg(target_arch = "x86_64")]
    {
        if is_x86_feature_detected!("avx2") && is_x86_feature_detected!("fma") {
            return unsafe { dot_q8_0_avx2(weights, x) };
        }
    }
    dot_q8_0_scalar(weights, x)
}

#[inline]
pub fn dot_q4_0(weights: &[BlockQ4_0], x: &[f32]) -> f32 {
    #[cfg(target_arch = "x86_64")]
    {
        if is_x86_feature_detected!("avx2") && is_x86_feature_detected!("fma") {
            return unsafe { dot_q4_0_avx2(weights, x) };
        }
    }
    dot_q4_0_scalar(weights, x)
}

/// Multi-core SIMD matrix-vector product for Q8_0 weights: out[r] = dot(W[r], x).
pub fn matvec_q8_0_parallel(
    weights: &[BlockQ8_0],
    rows: usize,
    cols: usize,
    x: &[f32],
    out: &mut [f32],
) {
    assert_eq!(cols % QK8_0, 0);
    let blocks_per_row = cols / QK8_0;
    assert_eq!(weights.len(), rows * blocks_per_row);
    assert_eq!(x.len(), cols);
    assert_eq!(out.len(), rows);

    let threads = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4)
        .min(rows.max(1));

    if threads <= 1 || rows < 32 {
        for (r, dst) in out.iter_mut().enumerate() {
            let row_slice = &weights[r * blocks_per_row..(r + 1) * blocks_per_row];
            *dst = dot_q8_0(row_slice, x);
        }
        return;
    }

    let chunk_rows = rows.div_ceil(threads);
    std::thread::scope(|scope| {
        for (t_idx, out_chunk) in out.chunks_mut(chunk_rows).enumerate() {
            let start_row = t_idx * chunk_rows;
            let end_row = start_row + out_chunk.len();
            let w_chunk = &weights[start_row * blocks_per_row..end_row * blocks_per_row];
            scope.spawn(move || {
                for (local_r, dst) in out_chunk.iter_mut().enumerate() {
                    let row_slice =
                        &w_chunk[local_r * blocks_per_row..(local_r + 1) * blocks_per_row];
                    *dst = dot_q8_0(row_slice, x);
                }
            });
        }
    });
}

/// Multi-core SIMD matrix-vector product for Q4_0 weights: out[r] = dot(W[r], x).
pub fn matvec_q4_0_parallel(
    weights: &[BlockQ4_0],
    rows: usize,
    cols: usize,
    x: &[f32],
    out: &mut [f32],
) {
    assert_eq!(cols % QK4_0, 0);
    let blocks_per_row = cols / QK4_0;
    assert_eq!(weights.len(), rows * blocks_per_row);
    assert_eq!(x.len(), cols);
    assert_eq!(out.len(), rows);

    let threads = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4)
        .min(rows.max(1));

    if threads <= 1 || rows < 32 {
        for (r, dst) in out.iter_mut().enumerate() {
            let row_slice = &weights[r * blocks_per_row..(r + 1) * blocks_per_row];
            *dst = dot_q4_0(row_slice, x);
        }
        return;
    }

    let chunk_rows = rows.div_ceil(threads);
    std::thread::scope(|scope| {
        for (t_idx, out_chunk) in out.chunks_mut(chunk_rows).enumerate() {
            let start_row = t_idx * chunk_rows;
            let end_row = start_row + out_chunk.len();
            let w_chunk = &weights[start_row * blocks_per_row..end_row * blocks_per_row];
            scope.spawn(move || {
                for (local_r, dst) in out_chunk.iter_mut().enumerate() {
                    let row_slice =
                        &w_chunk[local_r * blocks_per_row..(local_r + 1) * blocks_per_row];
                    *dst = dot_q4_0(row_slice, x);
                }
            });
        }
    });
}

/// Vectorized RMSNorm: out[i] = (x[i] / rms) * weight[i].
pub fn rmsnorm(x: &[f32], weight: &[f32], eps: f32, out: &mut [f32]) {
    assert_eq!(x.len(), weight.len());
    assert_eq!(x.len(), out.len());
    let ss: f32 = x.iter().map(|&v| v * v).sum();
    let inv_rms = 1.0 / ((ss / x.len() as f32) + eps).sqrt();
    for ((&xv, &wv), dst) in x.iter().zip(weight.iter()).zip(out.iter_mut()) {
        *dst = xv * inv_rms * wv;
    }
}

/// Fused SwiGLU activation: out[i] = (gate[i] * sigmoid(gate[i])) * up[i].
pub fn swiglu(gate: &[f32], up: &[f32], out: &mut [f32]) {
    assert_eq!(gate.len(), up.len());
    assert_eq!(gate.len(), out.len());
    for ((&g, &u), dst) in gate.iter().zip(up.iter()).zip(out.iter_mut()) {
        let silu = g / (1.0 + (-g).exp());
        *dst = silu * u;
    }
}

/// Numerically stable in-place softmax.
pub fn softmax_inplace(logits: &mut [f32]) {
    if logits.is_empty() {
        return;
    }
    let max_val = logits.iter().fold(f32::NEG_INFINITY, |a, &b| a.max(b));
    let mut sum = 0.0f32;
    for v in logits.iter_mut() {
        *v = (*v - max_val).exp();
        sum += *v;
    }
    if sum > 0.0 {
        let inv = 1.0 / sum;
        for v in logits.iter_mut() {
            *v *= inv;
        }
    }
}

/// Pre-allocated contiguous KV Cache arena for zero-allocation autoregressive decoding.
pub struct PagedKvCache {
    pub num_layers: usize,
    pub max_seq: usize,
    pub kv_dim: usize,
    pub keys: Vec<f32>,
    pub values: Vec<f32>,
}

impl PagedKvCache {
    pub fn new(num_layers: usize, max_seq: usize, kv_dim: usize) -> Self {
        let cap = num_layers * max_seq * kv_dim;
        Self {
            num_layers,
            max_seq,
            kv_dim,
            keys: vec![0.0; cap],
            values: vec![0.0; cap],
        }
    }

    pub fn write(&mut self, layer: usize, pos: usize, k: &[f32], v: &[f32]) {
        debug_assert!(layer < self.num_layers && pos < self.max_seq);
        let offset = (layer * self.max_seq + pos) * self.kv_dim;
        self.keys[offset..offset + self.kv_dim].copy_from_slice(k);
        self.values[offset..offset + self.kv_dim].copy_from_slice(v);
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PulseBenchmarkReport {
    pub cpu_threads: usize,
    pub avx2_fma_enabled: bool,
    pub matrix_shape: String,
    pub scalar_ms: f64,
    pub pulse_q4_ms: f64,
    pub pulse_q8_ms: f64,
    pub q4_speedup: f64,
    pub q8_speedup: f64,
    pub q4_bandwidth_gbps: f64,
    pub q4_gflops: f64,
}

pub fn avx2_fma_supported() -> bool {
    #[cfg(target_arch = "x86_64")]
    {
        return is_x86_feature_detected!("avx2") && is_x86_feature_detected!("fma");
    }
    #[allow(unreachable_code)]
    false
}

/// Runs a real CPU benchmark comparing scalar Q4_0 MatVec against Arthos Pulse
/// AVX2+FMA multi-core Q4_0 and Q8_0 MatVec on a 2048x2048 transformer projection.
pub fn run_pulse_benchmark() -> PulseBenchmarkReport {
    let rows = 2048usize;
    let cols = 2048usize;
    let threads = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1);

    let x: Vec<f32> = (0..cols).map(|i| ((i % 97) as f32 - 48.0) * 0.01).collect();
    let row_f32: Vec<f32> = (0..cols).map(|i| ((i % 53) as f32 - 26.0) * 0.02).collect();
    let q4_row = quantize_q4_0(&row_f32);
    let q8_row = quantize_q8_0(&row_f32);

    let mut w_q4 = Vec::with_capacity(rows * q4_row.len());
    let mut w_q8 = Vec::with_capacity(rows * q8_row.len());
    for _ in 0..rows {
        w_q4.extend_from_slice(&q4_row);
        w_q8.extend_from_slice(&q8_row);
    }
    let blocks_per_row = cols / QK4_0;
    let mut out = vec![0.0f32; rows];

    // Warmup
    matvec_q4_0_parallel(&w_q4, rows, cols, &x, &mut out);

    // Scalar Q4_0 baseline (single thread)
    let t0 = Instant::now();
    for (r, dst) in out.iter_mut().enumerate() {
        *dst = dot_q4_0_scalar(&w_q4[r * blocks_per_row..(r + 1) * blocks_per_row], &x);
    }
    let scalar_ms = t0.elapsed().as_secs_f64() * 1000.0;

    // Pulse Q4_0 AVX2 + multi-core (best of 3 passes)
    let mut pulse_q4_ms = f64::MAX;
    for _ in 0..3 {
        let t1 = Instant::now();
        matvec_q4_0_parallel(&w_q4, rows, cols, &x, &mut out);
        pulse_q4_ms = pulse_q4_ms.min(t1.elapsed().as_secs_f64() * 1000.0);
    }

    // Pulse Q8_0 AVX2 + multi-core (best of 3 passes)
    let mut pulse_q8_ms = f64::MAX;
    for _ in 0..3 {
        let t2 = Instant::now();
        matvec_q8_0_parallel(&w_q8, rows, cols, &x, &mut out);
        pulse_q8_ms = pulse_q8_ms.min(t2.elapsed().as_secs_f64() * 1000.0);
    }

    let q4_bytes = (w_q4.len() * std::mem::size_of::<BlockQ4_0>()) as f64;
    let q4_secs = (pulse_q4_ms / 1000.0).max(1e-9);
    let q4_bandwidth_gbps = (q4_bytes / 1e9) / q4_secs;
    let ops = (2 * rows * cols) as f64;
    let q4_gflops = (ops / 1e9) / q4_secs;

    PulseBenchmarkReport {
        cpu_threads: threads,
        avx2_fma_enabled: avx2_fma_supported(),
        matrix_shape: format!("{rows}x{cols}"),
        scalar_ms: (scalar_ms * 100.0).round() / 100.0,
        pulse_q4_ms: (pulse_q4_ms * 100.0).round() / 100.0,
        pulse_q8_ms: (pulse_q8_ms * 100.0).round() / 100.0,
        q4_speedup: ((scalar_ms / pulse_q4_ms.max(0.001)) * 10.0).round() / 10.0,
        q8_speedup: ((scalar_ms / pulse_q8_ms.max(0.001)) * 10.0).round() / 10.0,
        q4_bandwidth_gbps: (q4_bandwidth_gbps * 100.0).round() / 100.0,
        q4_gflops: (q4_gflops * 100.0).round() / 100.0,
    }
}
