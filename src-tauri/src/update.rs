//! 检查更新：读取网站上的 version.json（由 website/release.ps1 生成），与当前版本比较。
//!
//! 安装包放在 123 云盘，工具不自动下载，只提示更新内容并在浏览器里打开下载页。

use std::io::Read;
use std::time::Duration;

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};

pub const MANIFEST_URL: &str = "https://flash.wwrrj.cn/version.json";

/// 当前程序版本，取自 Cargo.toml。
pub const CURRENT_VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Debug, Clone, Deserialize)]
pub struct Manifest {
    pub version: String,
    #[serde(default)]
    pub date: String,
    #[serde(default)]
    pub notes: Vec<String>,
    pub download_url: String,
    /// 低于这个版本必须更新；为空表示不强制。
    #[serde(default)]
    pub min_version: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct UpdateInfo {
    pub current: String,
    pub latest: String,
    pub available: bool,
    /// 当前版本低于 min_version，必须更新后才能继续使用。
    pub required: bool,
    pub date: String,
    pub notes: Vec<String>,
    pub download_url: String,
}

/// 解析 `x.y.z`（允许前缀 v）；格式不对返回 None。
pub fn parse_version(text: &str) -> Option<(u64, u64, u64)> {
    let mut parts = text.trim().trim_start_matches(['v', 'V']).split('.');
    let mut next = || parts.next()?.trim().parse::<u64>().ok();
    let version = (next()?, next()?, next()?);
    parts.next().is_none().then_some(version)
}

pub fn evaluate(manifest: Manifest, current: &str) -> Result<UpdateInfo> {
    let cur = parse_version(current).with_context(|| format!("当前版本号无效：{}", current))?;
    let latest = parse_version(&manifest.version)
        .with_context(|| format!("version.json 的版本号无效：{}", manifest.version))?;
    if !manifest.download_url.starts_with("https://") {
        bail!("version.json 的下载链接必须是 https 地址");
    }
    let required = parse_version(&manifest.min_version).is_some_and(|min| cur < min);
    Ok(UpdateInfo {
        current: current.to_string(),
        latest: manifest.version,
        available: latest > cur,
        required,
        date: manifest.date,
        notes: manifest.notes,
        download_url: manifest.download_url,
    })
}

pub fn check() -> Result<UpdateInfo> {
    let agent = ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(8))
        .user_agent(&format!("WrionFlashTool/{}", CURRENT_VERSION))
        .build();
    let response = agent.get(MANIFEST_URL).call().context("无法连接更新服务器")?;
    let manifest: Manifest = serde_json::from_reader(response.into_reader().take(64 * 1024))
        .context("version.json 格式错误")?;
    evaluate(manifest, CURRENT_VERSION)
}
