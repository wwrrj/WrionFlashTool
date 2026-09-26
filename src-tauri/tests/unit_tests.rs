use tempfile::tempdir;
use harusora_flash_tool_lib::config::load_profile;
use harusora_flash_tool_lib::flash::{is_overflow_flash_error, logical_flash_size, needs_logical_resize};
use harusora_flash_tool_lib::package::is_preserved_data_partition;
use harusora_flash_tool_lib::sdat2img::{parse_ranges, sdat2img};
use harusora_flash_tool_lib::sparse::build_wipe_super_image;

const TEST_PROFILE: &str = r#"
[package]
author = "Harusora Team"
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
    assert_eq!(profile.author, "Harusora Team");
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

    std::fs::write(&tf, "4\n2\n0\n0\nnew 2,0,1\n").unwrap();
    let mut payload = b"gDla".to_vec();
    payload.resize(4096, 0);
    std::fs::write(&dat, &payload).unwrap();

    let sz = sdat2img(&tf, &dat, &raw, 4096, |_, _| {}).unwrap();
    assert_eq!(sz, 8192);

    let built = build_wipe_super_image(&raw, &wipe, 16 * 1024 * 1024).unwrap();
    assert!(built.is_file());
    let wipe_data = std::fs::read(&wipe).unwrap();
    assert_eq!(&wipe_data[0..4], b"gDla");
    assert_eq!(&wipe_data[4096..4100], b"gDla");
}
