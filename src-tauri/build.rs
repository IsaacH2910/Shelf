use std::path::PathBuf;
use std::process::Command;

fn main() {
    #[cfg(target_os = "macos")]
    {
        println!("cargo:rerun-if-changed=src/ocr/vision.m");
        println!("cargo:rerun-if-changed=src/translate/apple.swift");
        println!("cargo:rustc-link-lib=framework=Vision");
        println!("cargo:rustc-link-lib=framework=AppKit");
        println!("cargo:rustc-link-lib=framework=Foundation");
        println!("cargo:rustc-link-lib=framework=NaturalLanguage");
        println!("cargo:rustc-link-lib=framework=Translation");
        cc::Build::new()
            .file("src/ocr/vision.m")
            .flag("-fobjc-arc")
            .compile("shelf_apple_vision");

        compile_apple_translate();
    }
    tauri_build::build();
}

#[cfg(target_os = "macos")]
fn compile_apple_translate() {
    let out = PathBuf::from(std::env::var("OUT_DIR").expect("OUT_DIR"));
    let obj = out.join("apple_translate.o");
    let arch = std::env::var("CARGO_CFG_TARGET_ARCH").unwrap_or_else(|_| "aarch64".into());
    let triple = match arch.as_str() {
        "x86_64" => "x86_64-apple-macos15.0",
        _ => "arm64-apple-macos15.0",
    };
    let sdk = Command::new("xcrun")
        .args(["--sdk", "macosx", "--show-sdk-path"])
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .unwrap_or_default();

    let mut cmd = Command::new("xcrun");
    cmd.args([
        "swiftc",
        "-emit-object",
        "-parse-as-library",
        "-O",
        "-target",
        triple,
        "-o",
        obj.to_str().expect("utf8 path"),
        "src/translate/apple.swift",
    ]);
    if !sdk.is_empty() {
        cmd.args(["-sdk", &sdk]);
    }
    let status = cmd.status().expect("swiftc");
    if !status.success() {
        panic!("swiftc failed while building Apple Translation ({status})");
    }
    println!("cargo:rustc-link-arg={}", obj.display());
    println!("cargo:rustc-link-search=native=/usr/lib/swift");
    println!("cargo:rustc-link-arg=-Wl,-rpath,/usr/lib/swift");
    println!("cargo:rustc-link-lib=dylib=swiftCore");
    println!("cargo:rustc-link-lib=dylib=swift_Concurrency");
}

fn _pdfium_resource_hint() -> PathBuf {
    PathBuf::from("resources/pdfium")
}
