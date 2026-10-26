//! Building a test's .sy file into an ELF.

use std::{path::PathBuf, process::Command};

/// Compile tests/{name}.sy with the symbolasm in this workspace, and link
/// it at `text` with `entry` as its entry point. Needs arm-none-eabi
/// binutils.
pub fn build(name: &str, text: u32, entry: &str) -> Vec<u8> {
    let dir = std::env::temp_dir().join(format!("m33-{name}-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let src = manifest_dir.join(format!("tests/{name}.sy"));
    let out = Command::new(env!("CARGO"))
        .args(["run", "-q", "-p", "symbolasm", "--"])
        .arg(&src)
        .current_dir(manifest_dir.parent().unwrap())
        .output()
        .expect("running symbolasm");
    assert!(
        out.status.success(),
        "symbolasm failed:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let file = |ext: &str| dir.join(format!("{name}.{ext}"));
    std::fs::write(file("s"), &out.stdout).unwrap();
    let run = |cmd: &mut Command| assert!(cmd.status().unwrap().success(), "{cmd:?} failed");
    run(Command::new("arm-none-eabi-as")
        .args(["-mcpu=cortex-m33", "-o"])
        .arg(file("o"))
        .arg(file("s")));
    run(Command::new("arm-none-eabi-ld")
        .arg(format!("-Ttext={text:#x}"))
        .args(["-e", entry, "-o"])
        .arg(file("elf"))
        .arg(file("o")));
    std::fs::read(file("elf")).unwrap()
}
