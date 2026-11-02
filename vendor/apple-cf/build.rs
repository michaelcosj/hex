// apple-cf build.rs — builds the AppleCFBridge Swift static library and links
// it into the Rust crate.
//
// Vendored for Intel builds: the macOS 15 standalone Command Line Tools cannot
// link a SwiftPM manifest, so compile the bridge with swiftc directly. The
// original SwiftPM package kept each framework bridge in its own target; they
// only import Apple frameworks, so one module produces the same C symbols.

use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

const DEPLOYMENT_TARGET: &str = "13.0";

fn swift_sources(root: &Path, sources: &mut Vec<PathBuf>) {
    let entries = fs::read_dir(root).expect("read the Swift bridge sources");
    for entry in entries {
        let path = entry.expect("read a Swift bridge source").path();
        if path.is_dir() {
            swift_sources(&path, sources);
        } else if path.extension().is_some_and(|extension| extension == "swift") {
            sources.push(path);
        }
    }
}

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-env-changed=DOCS_RS");
    println!("cargo:rerun-if-env-changed=DEVELOPER_DIR");
    println!("cargo:rerun-if-env-changed=SDKROOT");

    if env::var("DOCS_RS").is_ok() {
        return;
    }

    println!("cargo:rustc-link-lib=framework=CoreGraphics");
    println!("cargo:rustc-link-lib=framework=IOSurface");
    println!("cargo:rustc-link-lib=framework=CoreFoundation");
    println!("cargo:rustc-link-lib=framework=CoreMedia");
    println!("cargo:rustc-link-lib=framework=CoreVideo");
    println!("cargo:rustc-link-lib=framework=Metal");

    let src_root = Path::new("swift-bridge/Sources");
    let out_dir = env::var("OUT_DIR").unwrap();
    let lib_dir = PathBuf::from(&out_dir).join("swift-build");
    fs::create_dir_all(&lib_dir).expect("create the Swift bridge output directory");

    println!("cargo:rerun-if-changed={}", src_root.display());

    let target_arch = env::var("CARGO_CFG_TARGET_ARCH").unwrap_or_default();
    let swift_triple = match target_arch.as_str() {
        "x86_64" => format!("x86_64-apple-macosx{DEPLOYMENT_TARGET}"),
        "aarch64" => format!("arm64-apple-macosx{DEPLOYMENT_TARGET}"),
        other => panic!("apple-cf: unsupported target arch '{other}'. Expected x86_64 or aarch64."),
    };

    let sdk = Command::new("xcrun")
        .args(["--sdk", "macosx", "--show-sdk-path"])
        .output()
        .expect("xcrun is required to locate the macOS SDK");
    assert!(sdk.status.success(), "could not locate the macOS SDK");
    let sdk_path = String::from_utf8_lossy(&sdk.stdout).trim().to_owned();

    let mut sources = Vec::new();
    swift_sources(src_root, &mut sources);
    sources.sort();

    let library = lib_dir.join("libAppleCFBridge.a");
    let mut swift = Command::new("xcrun");
    swift.args([
        "swiftc",
        "-emit-library",
        "-static",
        "-module-name",
        "AppleCFBridge",
        "-swift-version",
        "5",
        "-O",
        "-whole-module-optimization",
        "-sdk",
        &sdk_path,
        "-target",
        &swift_triple,
        "-o",
    ]);
    swift.arg(&library).args(&sources);

    let output = swift.output().expect("Failed to build Swift bridge");
    if !output.status.success() {
        eprintln!(
            "Swift build STDOUT:\n{}",
            String::from_utf8_lossy(&output.stdout)
        );
        eprintln!(
            "Swift build STDERR:\n{}",
            String::from_utf8_lossy(&output.stderr)
        );
        panic!(
            "Swift build failed with exit code: {:?}",
            output.status.code()
        );
    }

    println!("cargo:rustc-link-search=native={}", lib_dir.display());
    println!("cargo:rustc-link-lib=static=AppleCFBridge");

    println!("cargo:rustc-link-lib=framework=Foundation");

    println!("cargo:rustc-link-arg=-Wl,-rpath,/usr/lib/swift");

    if let Ok(output) = Command::new("xcode-select").arg("-p").output() {
        if output.status.success() {
            let xcode_path = String::from_utf8_lossy(&output.stdout).trim().to_string();
            let swift_lib_path =
                format!("{xcode_path}/Toolchains/XcodeDefault.xctoolchain/usr/lib/swift/macosx");
            println!("cargo:rustc-link-arg=-Wl,-rpath,{swift_lib_path}");
        }
    }
}
