use serde::{Deserialize, Serialize};
use std::fs::File;
use std::io::{BufReader, Read, Seek, SeekFrom};
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GgufSummary {
    pub path: String,
    pub version: u32,
    pub tensor_count: u64,
    pub kv_count: u64,
    pub architecture: String,
    pub model_name: String,
    pub context_length: Option<u64>,
    pub file_size_mb: f64,
}

fn read_u32(r: &mut impl Read) -> Result<u32, String> {
    let mut b = [0u8; 4];
    r.read_exact(&mut b).map_err(|e| e.to_string())?;
    Ok(u32::from_le_bytes(b))
}

fn read_u64(r: &mut impl Read) -> Result<u64, String> {
    let mut b = [0u8; 8];
    r.read_exact(&mut b).map_err(|e| e.to_string())?;
    Ok(u64::from_le_bytes(b))
}

fn read_string(r: &mut impl Read) -> Result<String, String> {
    let len = read_u64(r)? as usize;
    if len > 4 * 1024 * 1024 {
        return Err("GGUF string exceeds safety bound".into());
    }
    let mut buf = vec![0u8; len];
    r.read_exact(&mut buf).map_err(|e| e.to_string())?;
    Ok(String::from_utf8_lossy(&buf).into_owned())
}

fn skip_value(r: &mut (impl Read + Seek), vtype: u32) -> Result<Option<String>, String> {
    match vtype {
        0 | 1 | 7 => {
            r.seek(SeekFrom::Current(1)).map_err(|e| e.to_string())?;
            Ok(None)
        }
        2 | 3 => {
            r.seek(SeekFrom::Current(2)).map_err(|e| e.to_string())?;
            Ok(None)
        }
        4 | 5 | 6 => {
            let v = read_u32(r)?;
            Ok(Some(v.to_string()))
        }
        8 => Ok(Some(read_string(r)?)),
        9 => {
            let elem_type = read_u32(r)?;
            let count = read_u64(r)?;
            for _ in 0..count.min(200_000) {
                skip_value(r, elem_type)?;
            }
            Ok(None)
        }
        10 | 11 | 12 => {
            let v = read_u64(r)?;
            Ok(Some(v.to_string()))
        }
        _ => Err(format!("Unknown GGUF metadata type {vtype}")),
    }
}

/// Inspects a local `.gguf` file header and metadata in under 1ms without loading weights.
pub fn inspect_gguf(path: &Path) -> Result<GgufSummary, String> {
    let file = File::open(path).map_err(|e| format!("Failed to open GGUF: {e}"))?;
    let file_size_mb = file.metadata().map(|m| m.len() as f64 / (1024.0 * 1024.0)).unwrap_or(0.0);
    let mut r = BufReader::new(file);

    let mut magic = [0u8; 4];
    r.read_exact(&mut magic).map_err(|e| e.to_string())?;
    if &magic != b"GGUF" {
        return Err("Not a valid GGUF file (invalid magic bytes)".into());
    }

    let version = read_u32(&mut r)?;
    let tensor_count = read_u64(&mut r)?;
    let kv_count = read_u64(&mut r)?;

    let mut architecture = String::from("unknown");
    let mut model_name = path
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_default();
    let mut context_length = None;

    for _ in 0..kv_count.min(4096) {
        let key = read_string(&mut r)?;
        let vtype = read_u32(&mut r)?;
        let val = skip_value(&mut r, vtype)?;
        if key == "general.architecture" {
            if let Some(a) = val {
                architecture = a;
            }
        } else if key == "general.name" {
            if let Some(n) = val {
                model_name = n;
            }
        } else if key.ends_with(".context_length") {
            if let Some(c) = val.and_then(|s| s.parse::<u64>().ok()) {
                context_length = Some(c);
            }
        }
    }

    Ok(GgufSummary {
        path: path.to_string_lossy().to_string(),
        version,
        tensor_count,
        kv_count,
        architecture,
        model_name,
        context_length,
        file_size_mb: (file_size_mb * 100.0).round() / 100.0,
    })
}
