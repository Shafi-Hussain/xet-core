use std::borrow::Cow;
use std::fmt::Display;
use std::io::{copy, Cursor, Read, Write};
use std::time::Instant;

use anyhow::anyhow;
use lz4_flex::frame::{FrameDecoder, FrameEncoder};

use crate::byte_grouping::bg2::{bg2_regroup, bg2_split};
use crate::byte_grouping::bg4::{bg4_regroup, bg4_split};
use crate::error::{CasObjectError, Result};

pub static mut BG4_SPLIT_RUNTIME: f64 = 0.;
pub static mut BG4_REGROUP_RUNTIME: f64 = 0.;
pub static mut BG4_LZ4_COMPRESS_RUNTIME: f64 = 0.;
pub static mut BG4_LZ4_DECOMPRESS_RUNTIME: f64 = 0.;

/// Dis-allow the value of ascii capital letters as valid CompressionScheme, 65-90
#[repr(u8)]
#[derive(Debug, PartialEq, Eq, Clone, Copy, Default)]
pub enum CompressionScheme {
    #[default]
    None = 0,
    LZ4 = 1,
    ByteGrouping4LZ4 = 2, // 4 byte groups
    ByteGrouping2LZ4 = 3, // 2 byte groups
}

impl Display for CompressionScheme {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", Into::<&str>::into(self))
    }
}
impl From<&CompressionScheme> for &'static str {
    fn from(value: &CompressionScheme) -> Self {
        match value {
            CompressionScheme::None => "none",
            CompressionScheme::LZ4 => "lz4",
            CompressionScheme::ByteGrouping4LZ4 => "bg4-lz4",
            CompressionScheme::ByteGrouping2LZ4 => "bg2-lz4",
        }
    }
}

impl From<CompressionScheme> for &'static str {
    fn from(value: CompressionScheme) -> Self {
        From::from(&value)
    }
}

impl TryFrom<u8> for CompressionScheme {
    type Error = CasObjectError;

    fn try_from(value: u8) -> Result<Self> {
        match value {
            0 => Ok(CompressionScheme::None),
            1 => Ok(CompressionScheme::LZ4),
            2 => Ok(CompressionScheme::ByteGrouping4LZ4),
            3 => Ok(CompressionScheme::ByteGrouping2LZ4),
            _ => Err(CasObjectError::FormatError(anyhow!("cannot convert value {value} to CompressionScheme"))),
        }
    }
}

impl CompressionScheme {
    pub fn compress_from_slice<'a>(&self, data: &'a [u8]) -> Result<Cow<'a, [u8]>> {
        Ok(match self {
            CompressionScheme::None => data.into(),
            CompressionScheme::LZ4 => lz4_compress_from_slice(data).map(Cow::from)?,
            CompressionScheme::ByteGrouping4LZ4 => bg4_lz4_compress_from_slice(data).map(Cow::from)?,
            CompressionScheme::ByteGrouping2LZ4 => bg2_lz4_compress_from_slice(data).map(Cow::from)?,
        })
    }

    pub fn decompress_from_slice<'a>(&self, data: &'a [u8]) -> Result<Cow<'a, [u8]>> {
        Ok(match self {
            CompressionScheme::None => data.into(),
            CompressionScheme::LZ4 => lz4_decompress_from_slice(data).map(Cow::from)?,
            CompressionScheme::ByteGrouping4LZ4 => bg4_lz4_decompress_from_slice(data).map(Cow::from)?,
            CompressionScheme::ByteGrouping2LZ4 => bg2_lz4_decompress_from_slice(data).map(Cow::from)?,
        })
    }

    pub fn decompress_from_reader<R: Read, W: Write>(&self, reader: &mut R, writer: &mut W) -> Result<u64> {
        Ok(match self {
            CompressionScheme::None => copy(reader, writer)?,
            CompressionScheme::LZ4 => lz4_decompress_from_reader(reader, writer)?,
            CompressionScheme::ByteGrouping4LZ4 => bg4_lz4_decompress_from_reader(reader, writer)?,
            CompressionScheme::ByteGrouping2LZ4 => bg2_lz4_decompress_from_reader(reader, writer)?,
        })
    }
}

pub fn lz4_compress_from_slice(data: &[u8]) -> Result<Vec<u8>> {
    let mut enc = FrameEncoder::new(Vec::new());
    enc.write_all(data)?;
    Ok(enc.finish()?)
}

pub fn lz4_decompress_from_slice(data: &[u8]) -> Result<Vec<u8>> {
    let mut dest = vec![];
    lz4_decompress_from_reader(&mut Cursor::new(data), &mut dest)?;
    Ok(dest)
}

fn lz4_decompress_from_reader<R: Read, W: Write>(reader: &mut R, writer: &mut W) -> Result<u64> {
    let mut dec = FrameDecoder::new(reader);
    Ok(copy(&mut dec, writer)?)
}

pub fn bg4_lz4_compress_from_slice(data: &[u8]) -> Result<Vec<u8>> {
    let s = Instant::now();
    let groups = bg4_split(data);
    unsafe {
        BG4_SPLIT_RUNTIME += s.elapsed().as_secs_f64();
    }

    let s = Instant::now();
    let mut dest = vec![];
    let mut enc = FrameEncoder::new(&mut dest);
    enc.write_all(&groups)?;
    enc.finish()?;
    unsafe {
        BG4_LZ4_COMPRESS_RUNTIME += s.elapsed().as_secs_f64();
    }

    Ok(dest)
}

pub fn bg4_lz4_decompress_from_slice(data: &[u8]) -> Result<Vec<u8>> {
    let mut dest = vec![];
    bg4_lz4_decompress_from_reader(&mut Cursor::new(data), &mut dest)?;
    Ok(dest)
}

fn bg4_lz4_decompress_from_reader<R: Read, W: Write>(reader: &mut R, writer: &mut W) -> Result<u64> {
    let s = Instant::now();
    let mut g = vec![];
    FrameDecoder::new(reader).read_to_end(&mut g)?;
    unsafe {
        BG4_LZ4_DECOMPRESS_RUNTIME += s.elapsed().as_secs_f64();
    }

    let s = Instant::now();
    let regrouped = bg4_regroup(&g);
    unsafe {
        BG4_REGROUP_RUNTIME += s.elapsed().as_secs_f64();
    }

    writer.write_all(&regrouped)?;

    Ok(regrouped.len() as u64)
}

pub fn bg2_lz4_compress_from_slice(data: &[u8]) -> Result<Vec<u8>> {
    let groups = bg2_split(data);
    let mut dest = vec![];
    let mut enc = FrameEncoder::new(&mut dest);
    enc.write_all(&groups)?;
    enc.finish()?;

    Ok(dest)
}

pub fn bg2_lz4_decompress_from_slice(data: &[u8]) -> Result<Vec<u8>> {
    let mut dest = vec![];
    bg2_lz4_decompress_from_reader(&mut Cursor::new(data), &mut dest)?;
    Ok(dest)
}

fn bg2_lz4_decompress_from_reader<R: Read, W: Write>(reader: &mut R, writer: &mut W) -> Result<u64> {
    let mut g = vec![];
    FrameDecoder::new(reader).read_to_end(&mut g)?;

    let regrouped = bg2_regroup(&g);

    writer.write_all(&regrouped)?;

    Ok(regrouped.len() as u64)
}

#[cfg(test)]
mod tests {
    use std::mem::size_of;

    use half::prelude::*;
    use rand::Rng;

    use crate::byte_grouping::bg4::bg4_split_separate;

    use super::*;

    #[test]
    fn test_to_str() {
        assert_eq!(Into::<&str>::into(CompressionScheme::None), "none");
        assert_eq!(Into::<&str>::into(CompressionScheme::LZ4), "lz4");
        assert_eq!(Into::<&str>::into(CompressionScheme::ByteGrouping4LZ4), "bg4-lz4");
    }

    #[test]
    fn test_from_u8() {
        assert_eq!(CompressionScheme::try_from(0u8), Ok(CompressionScheme::None));
        assert_eq!(CompressionScheme::try_from(1u8), Ok(CompressionScheme::LZ4));
        assert_eq!(CompressionScheme::try_from(2u8), Ok(CompressionScheme::ByteGrouping4LZ4));
        assert!(CompressionScheme::try_from(3u8).is_err());
    }

    #[test]
    fn test_bg4_lz4() {
        let mut rng = rand::thread_rng();

        for i in 0..1 {
            let n = 64 * 1024 + i * 23;
            let all_zeros = vec![0u8; n];
            let all_ones = vec![1u8; n];
            let all_0xff = vec![0xFF; n];
            let random_u8s: Vec<_> = (0..n).map(|_| rng.gen_range(0..255)).collect();
            let random_f32s_ng1_1: Vec<_> = (0..n / size_of::<f32>())
                .map(|_| rng.gen_range(-1.0f32..=1.0))
                .map(|f| f.to_le_bytes())
                .flatten()
                .collect();
            let random_f32s_0_2: Vec<_> = (0..n / size_of::<f32>())
                .map(|_| rng.gen_range(0f32..=2.0))
                .map(|f| f.to_le_bytes())
                .flatten()
                .collect();
            let random_f64s_ng1_1: Vec<_> = (0..n / size_of::<f64>())
                .map(|_| rng.gen_range(-1.0f64..=1.0))
                .map(|f| f.to_le_bytes())
                .flatten()
                .collect();
            let random_f64s_0_2: Vec<_> = (0..n / size_of::<f64>())
                .map(|_| rng.gen_range(0f64..=2.0))
                .map(|f| f.to_le_bytes())
                .flatten()
                .collect();

            // f16, a.k.a binary16 format: sign (1 bit), exponent (5 bit), mantissa (10 bit)
            let random_f16s_ng1_1: Vec<_> = (0..n / size_of::<f16>())
                .map(|_| f16::from_f32(rng.gen_range(-1.0f32..=1.0)))
                .map(|f| f.to_le_bytes())
                .flatten()
                .collect();
            let random_f16s_0_2: Vec<_> = (0..n / size_of::<f16>())
                .map(|_| f16::from_f32(rng.gen_range(0f32..=2.0)))
                .map(|f| f.to_le_bytes())
                .flatten()
                .collect();

            // bf16 format: sign (1 bit), exponent (8 bit), mantissa (7 bit)
            let random_bf16s_ng1_1: Vec<_> = (0..n / size_of::<bf16>())
                .map(|_| bf16::from_f32(rng.gen_range(-1.0f32..=1.0)))
                .map(|f| f.to_le_bytes())
                .flatten()
                .collect();
            let random_bf16s_0_2: Vec<_> = (0..n / size_of::<bf16>())
                .map(|_| bf16::from_f32(rng.gen_range(0f32..=2.0)))
                .map(|f| f.to_le_bytes())
                .flatten()
                .collect();

            let dataset = [
                all_zeros,          // 231.58, 231.58, 231.58
                all_ones,           // 231.58, 231.58, 231.58
                all_0xff,           // 231.58, 231.58, 231.58
                random_u8s,         // 1.00, 1.00, 1.00
                random_f32s_ng1_1,  // 1.08, 1.00, 1.00
                random_f32s_0_2,    // 1.15, 1.00, 1.00
                random_f64s_ng1_1,  // 1.00, 1.00, 1.00
                random_f64s_0_2,    // 1.00, 1.00, 1.00
                random_f16s_ng1_1,  // 1.00, 1.00, 1.00
                random_f16s_0_2,    // 1.00, 1.00, 1.00
                random_bf16s_ng1_1, // 1.18, 1.00, 1.18
                random_bf16s_0_2,   // 1.37, 1.00, 1.35
            ];

            for data in dataset {
                let bg4_groups = bg4_split_separate(&data);
                let bg4_groups_hist = [
                    byte_bit_count_distribution(&bg4_groups[0]),
                    byte_bit_count_distribution(&bg4_groups[1]),
                    byte_bit_count_distribution(&bg4_groups[2]),
                    byte_bit_count_distribution(&bg4_groups[3]),
                ];
                let bg4_total_entropy: f64 = bg4_groups_hist
                    .iter()
                    .map(|hist| distribution_entropy(&as_distribution(hist)))
                    .sum();
                let bg2_total_entropy: f64 =
                    distribution_entropy(&as_distribution(&combine_hist(&bg4_groups_hist[0], &bg4_groups_hist[2])))
                        + distribution_entropy(&as_distribution(&combine_hist(
                            &bg4_groups_hist[1],
                            &bg4_groups_hist[3],
                        )));

                let bg4_lz4_compressed = bg4_lz4_compress_from_slice(&data).unwrap();
                let bg4_lz4_uncompressed = bg4_lz4_decompress_from_slice(&bg4_lz4_compressed).unwrap();
                assert_eq!(data.len(), bg4_lz4_uncompressed.len());
                assert_eq!(data, bg4_lz4_uncompressed);
                let lz4_compressed = lz4_compress_from_slice(&data).unwrap();
                let lz4_uncompressed = lz4_decompress_from_slice(&lz4_compressed).unwrap();
                assert_eq!(data, lz4_uncompressed);
                let bg2_lz4_compressed = bg2_lz4_compress_from_slice(&data).unwrap();
                let bg2_lz4_uncompressed = bg2_lz4_decompress_from_slice(&bg2_lz4_compressed).unwrap();
                assert_eq!(data.len(), bg2_lz4_uncompressed.len());
                assert_eq!(data, bg2_lz4_uncompressed);
                println!(
                    "Compression ratio: {:.2}, {:.2}, {:.2}, {:.5}, {:.5}",
                    data.len() as f32 / bg4_lz4_compressed.len() as f32,
                    data.len() as f32 / lz4_compressed.len() as f32,
                    data.len() as f32 / bg2_lz4_compressed.len() as f32,
                    bg4_total_entropy / 4.,
                    bg2_total_entropy / 2.,
                );
            }
        }
    }

    /// Compute the distribution of the number of 1-bits in each byte.
    ///
    /// Returns an array of length 9, where index `i` corresponds to how many bytes
    /// had exactly `i` 1-bits (i in [0..8]).
    fn byte_bit_count_distribution(data: &[u8]) -> [usize; 9] {
        let mut dist = [0usize; 9]; // Put in ghost counts for the kl divergence

        for &b in data {
            // Since Rust 1.37+, u8::count_ones() is stable.
            // It returns a u32 but in [0..8] for a u8.
            let ones = b.count_ones() as usize;
            dist[ones] += 1;
        }

        dist
    }

    fn combine_hist<const N: usize>(hist0: &[usize; N], hist1: &[usize; N]) -> [usize; N] {
        let mut ret = [0; N];
        for i in 0..N {
            ret[i] = hist0[i] + hist1[i];
        }
        ret
    }

    fn as_distribution<const N: usize>(dist: &[usize; N]) -> [f64; N] {
        // Normalize the input array to probabilities
        let total: usize = dist.iter().sum();
        if total == 0 {
            return [0.0; N];
        }
        let mut ret = [0.0; N];
        for i in 0..N {
            ret[i] = dist[i] as f64 / total as f64;
        }

        ret
    }

    /// Compute the Shannon entropy (base 2) from a bit-count distribution array.
    #[inline]
    fn distribution_entropy(dist: &[f64]) -> f64 {
        let mut entropy = 0.0;
        for &p in dist.iter() {
            if p > 0. {
                entropy -= p * p.log2();
            }
        }
        entropy
    }
}
