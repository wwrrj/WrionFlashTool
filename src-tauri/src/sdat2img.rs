use std::fs::File;
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom, Write};
use std::path::Path;
use anyhow::{anyhow, bail, Context, Result};

pub fn parse_ranges(text: &str) -> Result<Vec<(u64, u64)>> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Ok(Vec::new());
    }
    let parts: Vec<u64> = trimmed
        .split(',')
        .map(|s| s.trim().parse::<u64>().map_err(|e| anyhow!("无效整数: {:?}: {}", s, e)))
        .collect::<Result<Vec<_>>>()?;

    if parts.is_empty() {
        return Ok(Vec::new());
    }

    let n = parts[0] as usize;
    let rest = &parts[1..];
    if n != rest.len() {
        if parts.len() % 2 == 0 {
            let mut pairs = Vec::new();
            for chunk in parts.chunks_exact(2) {
                pairs.push((chunk[0], chunk[1]));
            }
            return Ok(pairs);
        }
        bail!("range 数量不匹配：声明 {}，得到 {} 个数值：{:?}", n, rest.len(), text);
    }
    if rest.len() % 2 != 0 {
        bail!("range 数量为奇数：{:?}", text);
    }
    let mut pairs = Vec::new();
    for chunk in rest.chunks_exact(2) {
        pairs.push((chunk[0], chunk[1]));
    }
    Ok(pairs)
}

pub fn sdat2img<F>(
    transfer_list: &Path,
    new_data: &Path,
    output_img: &Path,
    block_size: u64,
    mut progress: F,
) -> Result<u64>
where
    F: FnMut(u64, u64),
{
    let tf = File::open(transfer_list)
        .with_context(|| format!("无法打开 transfer.list：{:?}", transfer_list))?;
    let reader = BufReader::new(tf);
    let lines: Vec<String> = reader.lines().collect::<std::io::Result<Vec<_>>>()
        .with_context(|| format!("读取 transfer.list 失败：{:?}", transfer_list))?;

    if lines.len() < 4 {
        bail!("transfer.list 过短：{:?}", transfer_list);
    }

    let version: u32 = lines[0].trim().parse()
        .map_err(|_| anyhow!("bad transfer.list version: {:?}", lines[0]))?;
    if version < 1 || version > 4 {
        bail!("unsupported transfer.list version {}", version);
    }

    let total_blocks: u64 = lines[1].trim().parse()
        .map_err(|_| anyhow!("bad total blocks: {:?}", lines[1]))?;

    let cmd_start = if version == 1 { 2 } else { 4 };

    if let Some(parent) = output_img.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let out_size = total_blocks * block_size;
    let mut out_fh = File::create(output_img)
        .with_context(|| format!("无法创建输出镜像：{:?}", output_img))?;
    let mut dat_fh = File::open(new_data)
        .with_context(|| format!("无法打开 new.dat：{:?}", new_data))?;

    if out_size > 0 {
        out_fh.seek(SeekFrom::Start(out_size - 1))?;
        out_fh.write_all(&[0])?;
        out_fh.seek(SeekFrom::Start(0))?;
    }

    let dat_total = dat_fh.metadata()?.len().max(1);
    let mut dat_pos = 0u64;
    let mut last_report_pct: i32 = -1;

    progress(0, dat_total);

    let mut buf = vec![0u8; 1024 * 1024]; // 1MB buffer

    for raw in &lines[cmd_start..] {
        let line = raw.trim();
        if line.is_empty() {
            continue;
        }

        let mut parts = line.splitn(2, ' ');
        let cmd = parts.next().unwrap_or("");
        let arg = parts.next().unwrap_or("");

        if cmd == "erase" || cmd == "zero" {
            continue;
        }

        if cmd == "new" {
            let pairs = parse_ranges(arg)?;
            for (start, end) in pairs {
                if end < start {
                    bail!("bad range {}-{}", start, end);
                }
                let nblocks = end - start;
                let mut remaining_bytes = nblocks * block_size;
                let file_pos = start * block_size;
                out_fh.seek(SeekFrom::Start(file_pos))?;

                while remaining_bytes > 0 {
                    let to_read = (remaining_bytes as usize).min(buf.len());
                    dat_fh.read_exact(&mut buf[..to_read])
                        .with_context(|| format!("new.dat 数据意外截断，预期读取 {} 字节", to_read))?;
                    out_fh.write_all(&buf[..to_read])?;
                    dat_pos += to_read as u64;
                    remaining_bytes -= to_read as u64;

                    let pct = ((dat_pos * 100) / dat_total).min(100) as i32;
                    if pct != last_report_pct || dat_pos >= dat_total {
                        last_report_pct = pct;
                        progress(dat_pos, dat_total);
                    }
                }
            }
        }
    }

    progress(dat_total, dat_total);
    Ok(out_size)
}
