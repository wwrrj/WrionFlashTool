use std::fs::File;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use anyhow::{anyhow, bail, Context, Result};
use byteorder::{LittleEndian, ReadBytesExt};

pub const SPARSE_MAGIC: u32 = 0xED26FF3A;
pub const CHUNK_RAW: u16 = 0xCAC1;
pub const CHUNK_FILL: u16 = 0xCAC2;
pub const CHUNK_DONT_CARE: u16 = 0xCAC3;
pub const CHUNK_CRC32: u16 = 0xCAC4;

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
    let magic = b"gDla";
    let idx = data.windows(magic.len()).position(|window| window == magic)
        .ok_or_else(|| anyhow!("未在 {:?} 中找到 LP geometry 魔数 (gDla)", source))?;

    if idx + 4096 > data.len() {
        bail!("LP geometry 块已被截断");
    }

    let geom = &data[idx..idx + 4096];
    let tail = &data[idx + 4096..];

    // Rebuild: primary geom (4096) + backup geom (4096) + tail
    let mut rebuilt = Vec::with_capacity(4096 * 2 + tail.len());
    rebuilt.extend_from_slice(geom);
    rebuilt.extend_from_slice(geom);
    rebuilt.extend_from_slice(tail);

    if !rebuilt.starts_with(b"gDla") {
        bail!("重建的 wipe-super 镜像偏移量 0 处缺少 gDla");
    }

    std::fs::write(out_path, &rebuilt)?;
    let _ = std::fs::remove_file(&prefix);

    Ok(out_path.to_path_buf())
}
