#![cfg_attr(windows, windows_subsystem = "windows")]

#[cfg(windows)]
#[link(name = "kernel32")]
unsafe extern "system" {
    fn AttachConsole(process_id: u32) -> i32;
    fn AllocConsole() -> i32;
}

#[cfg(windows)]
fn ensure_cli_console() {
    const ATTACH_PARENT_PROCESS: u32 = u32::MAX;
    unsafe {
        if AttachConsole(ATTACH_PARENT_PROCESS) == 0 {
            let _ = AllocConsole();
        }
    }
}

#[cfg(not(windows))]
fn ensure_cli_console() {}

use std::path::PathBuf;
use harusora_flash_tool_lib::config::{load_profile, resolve_profile_path};
use harusora_flash_tool_lib::device::DeviceSession;
use harusora_flash_tool_lib::flash::{execute_flash_pipeline, gather_environment};
use harusora_flash_tool_lib::logger::Logger;

fn main() {
    let args: Vec<String> = std::env::args().collect();

    let is_cli = args.iter().any(|a| {
        matches!(a.as_str(), "--version" | "--check" | "--flash" | "--update")
    });
    if is_cli {
        ensure_cli_console();
    }

    if args.iter().any(|a| a == "--version") {
        println!("HarusoraFlashTool 2.0.0 (Rust + Tauri)");
        return;
    }

    if is_cli {
        let mut rom_root: Option<PathBuf> = None;
        let mut profile_arg: Option<PathBuf> = None;
        let mut confirm_word = String::new();
        let mut do_check = false;
        let mut do_flash = false;
        let mut do_update = false;

        let mut i = 1;
        while i < args.len() {
            match args[i].as_str() {
                "--rom-root" => {
                    if i + 1 < args.len() {
                        rom_root = Some(PathBuf::from(&args[i + 1]));
                        i += 1;
                    }
                }
                "--profile" => {
                    if i + 1 < args.len() {
                        profile_arg = Some(PathBuf::from(&args[i + 1]));
                        i += 1;
                    }
                }
                "--confirm" => {
                    if i + 1 < args.len() {
                        confirm_word = args[i + 1].clone();
                        i += 1;
                    }
                }
                "--check" => do_check = true,
                "--flash" => do_flash = true,
                "--update" => do_update = true,
                _ => {}
            }
            i += 1;
        }

        let (root, prof_path) = match resolve_profile_path(rom_root.as_deref(), profile_arg.as_deref()) {
            Ok(p) => p,
            Err(e) => {
                eprintln!("配置解析失败: {}", e);
                std::process::exit(2);
            }
        };

        let profile = match load_profile(&root, &prof_path) {
            Ok(p) => p,
            Err(e) => {
                eprintln!("配置错误: {}", e);
                std::process::exit(2);
            }
        };

        let mut logger = Logger::new(None);
        let _ = logger.start(&profile.logs_dir, "harusora-cli");

        if do_check {
            let mut session = DeviceSession::new(profile.fastboot(), profile.adb(), logger, false);
            match gather_environment(&profile, &mut session, None) {
                Ok(rep) => {
                    println!("OK package + device");
                    println!("  serial={}", rep.serial);
                    println!("  product={}", rep.product);
                    println!("  super={} ready={}", rep.super_bytes, rep.super_ready);
                    println!("  mode={}", if rep.is_userspace { "fastbootd" } else { "bootloader" });
                    println!("  anti={:?}", rep.anti);
                    println!("  {}", profile.package_summary());
                    std::process::exit(if rep.super_ready { 0 } else { 3 });
                }
                Err(e) => {
                    eprintln!("CHECK FAILED: {}", e);
                    std::process::exit(1);
                }
            }
        }

        if do_flash {
            if confirm_word.trim() != profile.confirmation_word {
                eprintln!("拒绝：完整线刷需 --confirm {}", profile.confirmation_word);
                std::process::exit(2);
            }
            let session = DeviceSession::new(profile.fastboot(), profile.adb(), logger, false);
            if let Err(e) = execute_flash_pipeline(&profile, session, false, None) {
                eprintln!("FLASH FAILED: {}", e);
                std::process::exit(1);
            }
            println!("线刷完成。");
            return;
        }

        if do_update {
            if confirm_word.trim() != profile.update_confirmation_word {
                eprintln!("拒绝：系统更新需 --confirm {}", profile.update_confirmation_word);
                std::process::exit(2);
            }
            let session = DeviceSession::new(profile.fastboot(), profile.adb(), logger, false);
            if let Err(e) = execute_flash_pipeline(&profile, session, true, None) {
                eprintln!("UPDATE FAILED: {}", e);
                std::process::exit(1);
            }
            println!("更新完成。");
            return;
        }
    }

    // Default to launching GUI
    harusora_flash_tool_lib::run();
}
