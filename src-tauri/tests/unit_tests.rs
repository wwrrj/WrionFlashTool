use tempfile::tempdir;
use wrion_flash_tool_lib::config::load_profile;
use wrion_flash_tool_lib::flash::{is_overflow_flash_error, logical_flash_size, needs_logical_resize};
use wrion_flash_tool_lib::package::is_preserved_data_partition;
use wrion_flash_tool_lib::sdat2img::{parse_ranges, sdat2img};
use wrion_flash_tool_lib::sparse::{
    build_android_sparse_from_raw, build_wipe_super_image, extract_sparse_prefix,
};

const TEST_PROFILE: &str = r#"
[package]
author = "Wrion Team"
qq_group = "123456"
build_date = "2026-08-20"

[device]
codenames = ["shennong"]
display_name = "Test ROM"

[requirements]
minimum_super_bytes = 1000

[paths]
images = "images"
super = "super"
work = "work"
logs = "logs"
tools = "tools"

[stages]
firmware = [{ action = "flash", partition = "boot_ab", file = "boot.img" }]
boot_chain = []
final_data = []

[dynamic]
partitions = []

[options]
enable_root_menu = false
post_flash_id = false
"#;

#[test]
fn test_config_load_and_fields() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    let profile_p = root.join("flash_profile.toml");
    std::fs::write(&profile_p, TEST_PROFILE).unwrap();

    let profile = load_profile(root, &profile_p).unwrap();
    assert_eq!(profile.author, "Wrion Team");
    assert_eq!(profile.qq_group, "123456");
    assert_eq!(profile.build_date, "2026-08-20");
    assert_eq!(profile.codenames, vec!["shennong"]);
    assert_eq!(profile.minimum_super_bytes, 1000);
    assert_eq!(profile.wipe_super_image, Some("super_empty_16g.img".to_string()));
    assert_eq!(profile.wipe_super_dat, None);
    assert!(!profile.enable_root_menu);
    assert!(!profile.post_flash_id);
}

#[test]
fn test_config_dat_only_wipe_super() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    let profile_p = root.join("flash_profile.toml");
    let content = TEST_PROFILE.replace("[dynamic]\n", "[dynamic]\nwipe_super_dat = \"super_empty\"\n");
    std::fs::write(&profile_p, content).unwrap();

    let profile = load_profile(root, &profile_p).unwrap();
    assert_eq!(profile.wipe_super_image, None);
    assert_eq!(profile.wipe_super_dat, Some("super_empty".to_string()));
}

#[test]
fn test_config_invalid_date_rejected() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    let profile_p = root.join("flash_profile.toml");
    let content = TEST_PROFILE.replace("2026-08-20", "2026/08/20");
    std::fs::write(&profile_p, content).unwrap();

    assert!(load_profile(root, &profile_p).is_err());
}

#[test]
fn test_config_path_traversal_rejected() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    let profile_p = root.join("flash_profile.toml");
    let content = TEST_PROFILE.replace("file = \"boot.img\"", "file = \"../boot.img\"");
    std::fs::write(&profile_p, content).unwrap();

    assert!(load_profile(root, &profile_p).is_err());
}

#[test]
fn test_preserved_data_partitions() {
    for name in &["userdata", "userdata_a", "userdata_b", "userdata_ab", "metadata", "metadata_a", "metadata_b", "metadata_ab"] {
        assert!(is_preserved_data_partition(name), "{} should be preserved", name);
    }
    for name in &["boot_ab", "system_a", "vendor_a", "mi_ext_a", "misc"] {
        assert!(!is_preserved_data_partition(name), "{} should NOT be preserved", name);
    }
}

#[test]
fn test_dynamic_partition_size_guard() {
    assert!(needs_logical_resize(None, 12 * 1024 * 1024));
    assert!(needs_logical_resize(Some(8 * 1024 * 1024), 12 * 1024 * 1024));
    assert!(!needs_logical_resize(Some(16 * 1024 * 1024), 12 * 1024 * 1024));
    assert_eq!(logical_flash_size(12 * 1024 * 1024 + 1, None), 12 * 1024 * 1024 + 4096);
}

#[test]
fn test_overflow_error_detection() {
    assert!(is_overflow_flash_error("FAILED (remote: Value too large for defined data type)"));
    assert!(is_overflow_flash_error("EOVERFLOW"));
    assert!(!is_overflow_flash_error("FAILED (remote: Not enough space)"));
}

#[test]
fn test_parse_ranges() {
    let pairs = parse_ranges("2,0,1").unwrap();
    assert_eq!(pairs, vec![(0, 1)]);

    let pairs2 = parse_ranges("4,0,1,2,3").unwrap();
    assert_eq!(pairs2, vec![(0, 1), (2, 3)]);
}

#[test]
fn test_sdat2img_and_wipe_super() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    let tf = root.join("super_empty.transfer.list");
    let dat = root.join("super_empty.new.dat");
    let raw = root.join("super_empty.raw.img");
    let wipe = root.join("wipe.img");

    std::fs::write(&tf, "4\n4\n0\n0\nnew 2,0,4\n").unwrap();
    let mut payload = vec![0u8; 16384];
    payload[4096..4100].copy_from_slice(b"gDla");
    payload[8192..8196].copy_from_slice(b"gDla");
    payload[12288..12292].copy_from_slice(b"0PLA");
    std::fs::write(&dat, &payload).unwrap();

    let sz = sdat2img(&tf, &dat, &raw, 4096, |_, _| {}).unwrap();
    assert_eq!(sz, 16384);

    let built = build_wipe_super_image(&raw, &wipe, 16 * 1024 * 1024).unwrap();
    assert!(built.is_file());
    let wipe_data = std::fs::read(&wipe).unwrap();
    assert_eq!(&wipe_data[0..4], b"gDla");
    assert_eq!(&wipe_data[4096..4100], b"gDla");
    assert_eq!(&wipe_data[8192..8196], b"0PLA");
}

#[test]
fn test_build_physical_super_sparse_from_raw_prefix() {
    let dir = tempdir().unwrap();
    let raw = dir.path().join("super.raw.img");
    let sparse = dir.path().join("super.sparse.img");
    let prefix = dir.path().join("prefix.img");
    let mut data = vec![0u8; 8192];
    data[4096..4100].copy_from_slice(b"gDla");
    std::fs::write(&raw, &data).unwrap();

    build_android_sparse_from_raw(&raw, 16384, &sparse, 16 * 1024 * 1024).unwrap();
    extract_sparse_prefix(&sparse, &prefix, 16384).unwrap();
    let restored = std::fs::read(&prefix).unwrap();
    assert_eq!(restored.len(), 16384);
    assert_eq!(&restored[4096..4100], b"gDla");
}

fn write_rom_zip(path: &std::path::Path, prefix: &str) {
    use std::io::Write;
    let mut zip = zip::ZipWriter::new(std::fs::File::create(path).unwrap());
    let opts = zip::write::SimpleFileOptions::default();
    zip.start_file(format!("{prefix}META-INF/com/android/flash_profile.toml"), opts).unwrap();
    zip.write_all(b"[package]\nname = \"Test ROM\"\n[device]\ndisplay_name = \"Test Phone\"\ncodenames = [\"alpha\", \"beta\"]\n").unwrap();
    zip.add_directory(format!("{prefix}images/"), opts).unwrap();
    zip.start_file(format!("{prefix}images/boot.img"), opts).unwrap();
    zip.write_all(&[7u8; 300_000]).unwrap();
    zip.finish().unwrap();
}

#[test]
fn test_rom_zip_inspect_and_extract() {
    use wrion_flash_tool_lib::rompkg::{default_dest, extract_package, inspect_package};
    let dir = tempdir().unwrap();
    for prefix in ["", "Wrapped/"] {
        let zip_path = dir.path().join(format!("rom{}.zip", prefix.len()));
        write_rom_zip(&zip_path, prefix);

        let info = inspect_package(&zip_path).unwrap();
        assert_eq!(info.package_name, "Test ROM");
        assert_eq!(info.codenames, vec!["alpha", "beta"]);
        assert_eq!(info.file_count, 2);
        assert!(!info.already_extracted);

        let dest = default_dest(&zip_path);
        let root = extract_package(&zip_path, &dest, &mut |_| {}).unwrap();
        assert_eq!(std::fs::metadata(root.join("images/boot.img")).unwrap().len(), 300_000);
        assert!(root.join("META-INF/com/android/flash_profile.toml").is_file());
        assert!(inspect_package(&zip_path).unwrap().already_extracted);
        // 已完整解压时直接复用
        extract_package(&zip_path, &dest, &mut |_| panic!("不应重新解压")).unwrap();
    }
}

#[test]
fn test_rom_zip_rejects_foreign_dir_and_non_rom() {
    use wrion_flash_tool_lib::rompkg::{extract_package, inspect_package};
    let dir = tempdir().unwrap();
    let zip_path = dir.path().join("rom.zip");
    write_rom_zip(&zip_path, "");

    let foreign = dir.path().join("foreign");
    std::fs::create_dir(&foreign).unwrap();
    std::fs::write(foreign.join("keep.txt"), "x").unwrap();
    assert!(extract_package(&zip_path, &foreign, &mut |_| {}).is_err());
    assert!(foreign.join("keep.txt").is_file());

    let other = dir.path().join("other.zip");
    let mut zip = zip::ZipWriter::new(std::fs::File::create(&other).unwrap());
    zip.start_file("readme.txt", zip::write::SimpleFileOptions::default()).unwrap();
    zip.finish().unwrap();
    assert!(inspect_package(&other).is_err());
}

fn test_expansion_cfg(backup_dir: &std::path::Path) -> wrion_flash_tool_lib::config::ExpansionConfig {
    wrion_flash_tool_lib::config::ExpansionConfig {
        enabled: true,
        codenames: vec!["alpha".into()],
        target_bytes: 1 << 30,
        disk: "/dev/block/sda".into(),
        super_part: 32,
        userdata_part: 33,
        backup_dir: backup_dir.to_path_buf(),
        sgdisk_dir: backup_dir.to_path_buf(),
        remote_sgdisk_dir: "/tmp/sg".into(),
        confirm_word: "EXPAND".into(),
        restore_confirm_word: "RESTORE".into(),
    }
}

const TEST_PLAN: &str = "disk=/dev/block/sda\ndevice=alpha\nsector_bytes=4096\nsuper_old=100-199\nsuper_new=100-299\nuserdata_old=200-999\nuserdata_new=300-999\n";

fn fake_gpt() -> Vec<u8> {
    let mut gpt = vec![0u8; 4096 * 3];
    gpt[4096..4104].copy_from_slice(b"EFI PART");
    gpt
}

#[test]
fn test_gpt_backup_zip_roundtrip_and_listing() {
    use wrion_flash_tool_lib::backup::{inspect_backup, list_backups, read_backup, write_backup_zip};
    let dir = tempdir().unwrap();
    let cfg = test_expansion_cfg(dir.path());

    let zip_path = dir.path().join("alpha-super-expand-20260101-120000.zip");
    write_backup_zip(&zip_path, &fake_gpt(), TEST_PLAN).unwrap();
    let info = inspect_backup(&zip_path, &cfg);
    assert!(info.supported, "{:?}", info.problems);
    assert_eq!(info.device.as_deref(), Some("alpha"));
    assert_eq!(info.super_old_bytes, Some(100 * 4096));
    assert_eq!(info.super_new_bytes, Some(200 * 4096));
    assert_eq!(read_backup(&zip_path).unwrap().gpt, fake_gpt());

    // 旧版目录备份也能识别
    let legacy = dir.path().join("alpha-super-expand-old");
    std::fs::create_dir(&legacy).unwrap();
    std::fs::write(legacy.join("gpt-sda-before.bin"), fake_gpt()).unwrap();
    std::fs::write(legacy.join("plan.txt"), TEST_PLAN).unwrap();
    assert!(inspect_backup(&legacy, &cfg).supported);

    let listed = list_backups(&cfg);
    assert_eq!(listed.len(), 2);
    assert!(listed.iter().all(|b| b.supported));

    // 无效 GPT 不会写出备份
    assert!(write_backup_zip(&dir.path().join("bad.zip"), &[0u8; 64], TEST_PLAN).is_err());
    assert!(!dir.path().join("bad.zip").exists());
}

#[test]
fn test_gpt_backup_rejects_mismatch() {
    use wrion_flash_tool_lib::backup::{inspect_backup, write_backup_zip};
    let dir = tempdir().unwrap();
    let cfg = test_expansion_cfg(dir.path());

    let other_device = dir.path().join("other.zip");
    write_backup_zip(&other_device, &fake_gpt(), &TEST_PLAN.replace("device=alpha", "device=gamma")).unwrap();
    let info = inspect_backup(&other_device, &cfg);
    assert!(!info.supported);
    assert!(info.problems.iter().any(|p| p.contains("gamma")));

    let other_disk = dir.path().join("disk.zip");
    write_backup_zip(&other_disk, &fake_gpt(), &TEST_PLAN.replace("/dev/block/sda", "/dev/block/sdb")).unwrap();
    assert!(!inspect_backup(&other_disk, &cfg).supported);

    // 普通 zip（ROM 包等）不是备份
    let rom = dir.path().join("rom.zip");
    write_rom_zip(&rom, "");
    assert!(!inspect_backup(&rom, &cfg).supported);
}

fn official_fixture(device: &str, name: &str) -> String {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/official").join(device).join(name);
    String::from_utf8_lossy(&std::fs::read(path).unwrap()).into_owned()
}

#[test]
fn test_official_flash_all_parses_to_whitelisted_ops() {
    use wrion_flash_tool_lib::official::{flatten, parse_bat, touches_user_data, ScriptOp};
    let root = std::path::Path::new("R:/pkg");
    let ops = parse_bat(&official_fixture("shennong", "flash_all.bat"), root).unwrap();

    assert_eq!(ops[0], ScriptOp::CheckProduct(vec!["shennong".into(), "shennongpro".into()]));
    assert_eq!(ops.iter().filter(|op| matches!(op, ScriptOp::CheckProduct(_))).count(), 1);
    assert_eq!(ops.iter().filter(|op| **op == ScriptOp::CheckAnti).count(), 1);
    // crc 条件块里刷入两个 crc 列表
    let crc = ops.iter().find_map(|op| match op {
        ScriptOp::IfVar { name, expect, body } if name == "crc" => Some((expect.clone(), body.len())),
        _ => None,
    });
    assert_eq!(crc, Some(("1".to_string(), 2)));
    assert!(ops.iter().any(|op| matches!(op, ScriptOp::IfVar { name, expect, .. } if name == "partition-type:opcust" && expect == "raw")));
    assert!(ops.contains(&ScriptOp::Flash { partition: "super".into(), file: root.join("images/super.img") }));
    assert!(ops.contains(&ScriptOp::Flash { partition: "misc".into(), file: root.join("images/misc.img") }));
    assert_eq!(&ops[ops.len() - 2..], &[ScriptOp::SetActive("a".into()), ScriptOp::Reboot]);
    assert!(touches_user_data(&ops));
    assert!(flatten(&ops).len() > 50);

    let houji = parse_bat(&official_fixture("houji", "flash_all.bat"), root).unwrap();
    assert_eq!(houji[0], ScriptOp::CheckProduct(vec!["houji".into()]));
}

#[test]
fn test_official_keep_data_script_and_lock_refusal() {
    use wrion_flash_tool_lib::official::{parse_bat, touches_user_data};
    let root = std::path::Path::new("R:/pkg");
    for device in ["shennong", "houji"] {
        let keep = parse_bat(&official_fixture(device, "flash_all_except_storage.bat"), root).unwrap();
        assert!(!touches_user_data(&keep), "{device} except_storage 不应改写用户数据");
        let lock = parse_bat(&official_fixture(device, "flash_all_lock.bat"), root).unwrap_err();
        assert!(format!("{lock:#}").contains("锁定 Bootloader"), "{lock:#}");
    }
}

#[test]
fn test_official_parser_rejects_unknown_and_unsafe_lines() {
    use wrion_flash_tool_lib::official::parse_bat;
    let root = std::path::Path::new("R:/pkg");
    let base = "fastboot %* getvar product 2>&1 | findstr /r /c:\"^product: *alpha\" || exit /B 1\n";
    let cases = [
        "del /q C:\\Windows\\foo",
        "fastboot %* flash boot_ab %~dp0..\\evil.img || exit 1",
        "fastboot %* flash boot_ab C:\\evil.img || exit 1",
        "fastboot %* flashing lock",
        "fastboot %* oem unlock-go",
        "fastboot %* reboot bootloader",
        "fastboot -w",
    ];
    for line in cases {
        assert!(parse_bat(&format!("{base}{line}\n"), root).is_err(), "应拒绝：{line}");
    }
    // 没有机型校验的脚本不执行
    assert!(parse_bat("fastboot %* reboot\n", root).is_err());
    assert!(parse_bat(&format!("{base}fastboot %* reboot\n"), root).is_ok());
}

#[test]
fn test_official_package_loads_as_profile() {
    use wrion_flash_tool_lib::official::{flatten, load_official, parse_bat, ScriptOp};
    let dir = tempdir().unwrap();
    let root = dir.path();
    std::fs::create_dir(root.join("images")).unwrap();
    for name in ["flash_all.bat", "flash_all_except_storage.bat", "misc.txt"] {
        std::fs::write(root.join(name), official_fixture("shennong", name)).unwrap();
    }
    std::fs::write(root.join("images/anti_version.txt"), "1").unwrap();
    // 只创建脚本引用到的文件（内容不重要）
    let ops = parse_bat(&official_fixture("shennong", "flash_all.bat"), root).unwrap();
    for op in flatten(&ops) {
        if let ScriptOp::Flash { file, .. } = op {
            std::fs::write(file, b"x").unwrap();
        }
    }

    let profile = load_official(root).unwrap();
    assert!(profile.is_official());
    assert_eq!(profile.codenames, vec!["shennong", "shennongpro"]);
    assert!(profile.package_name.contains("OS2.0.221.0.VNBCNXM"));
    assert_eq!(profile.build_date, "2025-09-28");
    assert_eq!(profile.minimum_super_bytes, 0);
    let info = profile.official.clone().unwrap();
    assert_eq!(info.clean_script.as_deref(), Some("flash_all.bat"));
    assert_eq!(info.keep_data_script.as_deref(), Some("flash_all_except_storage.bat"));
    assert!(profile.fastboot().is_file(), "内置 fastboot 应已释放：{:?}", profile.fastboot());

    // 缺文件时拒绝加载
    std::fs::remove_file(root.join("images/super.img")).unwrap();
    assert!(format!("{:#}", load_official(root).unwrap_err()).contains("super.img"));
}

#[test]
fn test_embedded_platform_tools_are_complete() {
    let dir = wrion_flash_tool_lib::platform_tools::tools_dir().unwrap();
    // fastboot -w 需要同目录的 make_f2fs / mke2fs 生成 userdata 镜像
    for name in ["fastboot.exe", "adb.exe", "AdbWinApi.dll", "AdbWinUsbApi.dll", "make_f2fs.exe", "mke2fs.exe", "mke2fs.conf"] {
        assert!(dir.join(name).is_file(), "缺少 {name}：{dir:?}");
    }
}

#[test]
fn test_usb_driver_extracts_signed_package_intact() {
    use wrion_flash_tool_lib::usb_driver::extract;
    let inf = extract().unwrap();
    let dir = inf.parent().unwrap();
    for name in ["android_winusb.inf", "android_winusb.cat", "amd64/WdfCoInstaller01009.dll", "amd64/winusbcoinstaller2.dll"] {
        assert!(dir.join(name).is_file(), "缺少 {name}");
    }
    // .cat 签名覆盖 .inf 原始字节：必须保留 CRLF，且声明了按接口类型匹配的 ADB / Fastboot
    let text = std::fs::read(&inf).unwrap();
    assert!(text.windows(2).any(|w| w == b"\r\n"));
    let text = String::from_utf8_lossy(&text);
    assert!(text.contains(r"USB\Class_FF&SubClass_42&Prot_01"));
    assert!(text.contains(r"USB\Class_FF&SubClass_42&Prot_03"));
}
