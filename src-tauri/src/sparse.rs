use std::fs::File;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use anyhow::{anyhow, bail, Context, Result};
use byteorder::{LittleEndian, ReadBytesExt, WriteBytesExt};

pub const SPARSE_MAGIC: u32 = 0xED26FF3A;
pub const CHUNK_RAW: u16 = 0xCAC1;
pub const CHUNK_FILL: u16 = 0xCAC2;
pub const CHUNK_DONT_CARE: u16 = 0xCAC3;
pub const CHUNK_CRC32: u16 = 0xCAC4;
const LP_GEOMETRY_MAGIC: &[u8] = b"gDla";
const LP_METADATA_MAGIC: &[u8] = b"0PLA";
const LP_GEOMETRY_SIZE: usize = 4096;

pub fn is_android_sparse(path: &Path) -> bool {
    let mut file = match File::open(path) {
        Ok(f) => f,
        Err(_) => return false,
    };
    let mut buf = [0u8; 4];
    if file.read_exact(&mut buf).is_err() {
        return false;
    }
    u32::from_le_bytes(buf) == SPARSE_MAGIC
}

pub fn extract_sparse_prefix(sparse_path: &Path, out_path: &Path, max_bytes: usize) -> Result<usize> {
    let mut fh = File::open(sparse_path)
        .with_context(|| format!("无法打开 sparse 文件：{:?}", sparse_path))?;

    let magic = fh.read_u32::<LittleEndian>()?;
    if magic != SPARSE_MAGIC {
        bail!("not an Android sparse image: {:?}", sparse_path);
    }
    let _major = fh.read_u16::<LittleEndian>()?;
    let _minor = fh.read_u16::<LittleEndian>()?;
    let file_hdr_sz = fh.read_u16::<LittleEndian>()? as usize;
    let chunk_hdr_sz = fh.read_u16::<LittleEndian>()? as usize;
    let blk_sz = fh.read_u32::<LittleEndian>()? as usize;
    let _total_blks = fh.read_u32::<LittleEndian>()?;
    let total_chunks = fh.read_u32::<LittleEndian>()? as usize;
    let _crc = fh.read_u32::<LittleEndian>()?;

    if file_hdr_sz > 28 {
        let mut skip = vec![0u8; file_hdr_sz - 28];
        fh.read_exact(&mut skip)?;
    }

    if let Some(parent) = out_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut out = File::create(out_path)?;
    let mut written = 0usize;

    for _ in 0..total_chunks {
        if written >= max_bytes {
            break;
        }
        let chunk_type = fh.read_u16::<LittleEndian>()?;
        let _reserved = fh.read_u16::<LittleEndian>()?;
        let chunk_sz = fh.read_u32::<LittleEndian>()? as usize;
        let total_sz = fh.read_u32::<LittleEndian>()? as usize;

        if chunk_hdr_sz > 12 {
            let mut skip = vec![0u8; chunk_hdr_sz - 12];
            fh.read_exact(&mut skip)?;
        }

        let data_size = total_sz.saturating_sub(chunk_hdr_sz);
        let unsparsed_chunk = chunk_sz * blk_sz;
        let need = unsparsed_chunk.min(max_bytes - written);

        match chunk_type {
            CHUNK_RAW => {
                let mut payload = vec![0u8; data_size];
                fh.read_exact(&mut payload)?;
                out.write_all(&payload[..need])?;
                written += need;
            }
            CHUNK_FILL => {
                let mut fill = [0u8; 4];
                fh.read_exact(&mut fill)?;
                if data_size > 4 {
                    let mut rest = vec![0u8; data_size - 4];
                    fh.read_exact(&mut rest)?;
                }
                let mut chunk_buf = Vec::with_capacity(need);
                for i in 0..need {
                    chunk_buf.push(fill[i % 4]);
                }
                out.write_all(&chunk_buf)?;
                written += need;
            }
            CHUNK_DONT_CARE => {
                if data_size > 0 {
                    let mut skip = vec![0u8; data_size];
                    fh.read_exact(&mut skip)?;
                }
                let zeros = vec![0u8; need];
                out.write_all(&zeros)?;
                written += need;
            }
            CHUNK_CRC32 => {
                let mut skip = vec![0u8; data_size];
                fh.read_exact(&mut skip)?;
            }
            _ => {
                bail!("unknown sparse chunk type 0x{:x}", chunk_type);
            }
        }
    }

    Ok(written)
}

fn magic_at(data: &[u8], offset: usize, magic: &[u8]) -> bool {
    data.get(offset..offset.saturating_add(magic.len())) == Some(magic)
}

fn normalize_wipe_super_payload(data: &[u8]) -> Result<Vec<u8>> {
    let min_len = LP_GEOMETRY_SIZE * 3 + LP_METADATA_MAGIC.len();
    if data.len() < min_len {
        bail!("空 super 元数据过短，无法重组 wipe-super 镜像");
    }

    // A host wipe-super image starts with primary geometry, backup geometry,
    // then the metadata header. A DAT restored from a physical super image
    // has an additional 4 KiB reserved region before those structures.
    if magic_at(data, 0, LP_GEOMETRY_MAGIC)
        && magic_at(data, LP_GEOMETRY_SIZE, LP_GEOMETRY_MAGIC)
        && magic_at(data, LP_GEOMETRY_SIZE * 2, LP_METADATA_MAGIC)
    {
        return Ok(data.to_vec());
    }

    if magic_at(data, LP_GEOMETRY_SIZE, LP_GEOMETRY_MAGIC)
        && magic_at(data, LP_GEOMETRY_SIZE * 2, LP_GEOMETRY_MAGIC)
        && magic_at(data, LP_GEOMETRY_SIZE * 3, LP_METADATA_MAGIC)
    {
        let geometry = &data[LP_GEOMETRY_SIZE..LP_GEOMETRY_SIZE * 2];
        let mut rebuilt = Vec::with_capacity(data.len() - LP_GEOMETRY_SIZE);
        rebuilt.extend_from_slice(geometry);
        rebuilt.extend_from_slice(geometry);
        rebuilt.extend_from_slice(&data[LP_GEOMETRY_SIZE * 3..]);
        return Ok(rebuilt);
    }

    bail!("空 super 元数据中未找到有效的 LP geometry/metadata 布局")
}

pub fn build_wipe_super_image(source: &Path, out_path: &Path, max_bytes: usize) -> Result<PathBuf> {
    if let Some(parent) = out_path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let prefix = out_path.with_extension("prefix.bin");
    if is_android_sparse(source) {
        extract_sparse_prefix(source, &prefix, max_bytes)?;
    } else {
        let mut src_fh = File::open(source)
            .with_context(|| format!("无法打开源 super 文件：{:?}", source))?;
        let mut buf = vec![0u8; max_bytes];
        let n = src_fh.read(&mut buf)?;
        let mut p_fh = File::create(&prefix)?;
        p_fh.write_all(&buf[..n])?;
    }

    let data = std::fs::read(&prefix)?;
    let rebuilt = normalize_wipe_super_payload(&data)
        .map_err(|error| anyhow!("无法规范化 {:?} 的 wipe-super 元数据：{}", source, error))?;

    std::fs::write(out_path, &rebuilt)?;
    let _ = std::fs::remove_file(&prefix);

    Ok(out_path.to_path_buf())
}

/// Build a physical-super sparse image from the metadata prefix of a raw
/// super image. The remaining blocks are DONT_CARE, so a 16 GiB super image
/// does not need to be materialized on the host disk.
pub fn build_android_sparse_from_raw(
    raw_path: &Path,
    total_bytes: u64,
    out_path: &Path,
    max_prefix_bytes: usize,
) -> Result<PathBuf> {
    const BLOCK_SIZE: u64 = 4096;
    if total_bytes == 0 || total_bytes % BLOCK_SIZE != 0 {
        bail!("空 super 逻辑大小必须是 4096 的整数倍：{}", total_bytes);
    }

    let prefix_limit = (total_bytes as usize).min(max_prefix_bytes);
    let raw = File::open(raw_path)
        .with_context(|| format!("无法打开 raw super：{:?}", raw_path))?;
    let mut prefix = Vec::with_capacity(prefix_limit);
    raw.take(prefix_limit as u64).read_to_end(&mut prefix)?;
    if prefix.is_empty() {
        bail!("raw super 元数据为空：{:?}", raw_path);
    }

    let padded_prefix_bytes = ((prefix.len() as u64 + BLOCK_SIZE - 1) / BLOCK_SIZE) * BLOCK_SIZE;
    let total_blocks = total_bytes / BLOCK_SIZE;
    let raw_blocks = (padded_prefix_bytes / BLOCK_SIZE).min(total_blocks);
    prefix.resize((raw_blocks * BLOCK_SIZE) as usize, 0);
    let rest_blocks = total_blocks - raw_blocks;

    if let Some(parent) = out_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut out = File::create(out_path)
        .with_context(|| format!("无法创建 sparse super：{:?}", out_path))?;
    out.write_u32::<LittleEndian>(SPARSE_MAGIC)?;
    out.write_u16::<LittleEndian>(1)?;
    out.write_u16::<LittleEndian>(0)?;
    out.write_u16::<LittleEndian>(28)?;
    out.write_u16::<LittleEndian>(12)?;
    out.write_u32::<LittleEndian>(BLOCK_SIZE as u32)?;
    out.write_u32::<LittleEndian>(total_blocks as u32)?;
    out.write_u32::<LittleEndian>(if rest_blocks > 0 { 2 } else { 1 })?;
    out.write_u32::<LittleEndian>(0)?;

    out.write_u16::<LittleEndian>(CHUNK_RAW)?;
    out.write_u16::<LittleEndian>(0)?;
    out.write_u32::<LittleEndian>(raw_blocks as u32)?;
    out.write_u32::<LittleEndian>((12 + prefix.len()) as u32)?;
    out.write_all(&prefix)?;

    if rest_blocks > 0 {
        out.write_u16::<LittleEndian>(CHUNK_DONT_CARE)?;
        out.write_u16::<LittleEndian>(0)?;
        out.write_u32::<LittleEndian>(rest_blocks as u32)?;
        out.write_u32::<LittleEndian>(12)?;
    }
    Ok(out_path.to_path_buf())
}
