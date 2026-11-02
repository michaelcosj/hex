// apple-metal build.rs — builds the AppleMetalSwiftBridge Swift static
// library and links it into the Rust crate.
//
// Vendored for Intel builds: on macOS 15 the standalone Command Line Tools
// cannot link a SwiftPM manifest, and their SDK lacks the macOS 26 Metal
// tensor and sampler APIs. Compile the bridge with swiftc directly instead;
// the SDK 26 code is compiled only when the toolchain provides it.

use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-env-changed=DOCS_RS");
    println!("cargo:rerun-if-env-changed=DEVELOPER_DIR");

    if env::var("DOCS_RS").is_ok() {
        return;
    }

    println!("cargo:rustc-link-lib=framework=Metal");
    println!("cargo:rustc-link-lib=framework=MetalFX");
    println!("cargo:rustc-link-lib=framework=Foundation");
    println!("cargo:rustc-link-lib=framework=IOSurface");

    let src_dir = Path::new("swift-bridge/Sources/AppleMetalBridge");
    let out_dir = env::var("OUT_DIR").unwrap();
    let lib_dir = PathBuf::from(&out_dir).join("swift-build");
    fs::create_dir_all(&lib_dir).expect("create the Swift bridge output directory");

    println!("cargo:rerun-if-changed={}", src_dir.display());

    let target_arch = env::var("CARGO_CFG_TARGET_ARCH").unwrap_or_default();
    let swift_triple = match target_arch.as_str() {
        "x86_64" => "x86_64-apple-macosx11.0",
        "aarch64" => "arm64-apple-macosx11.0",
        other => panic!("apple-metal: unsupported target arch '{other}'"),
    };

    let sdk = Command::new("xcrun")
        .args(["--sdk", "macosx", "--show-sdk-path"])
        .output()
        .expect("xcrun is required to locate the macOS SDK");
    assert!(sdk.status.success(), "could not locate the macOS SDK");
    let sdk_path = String::from_utf8_lossy(&sdk.stdout).trim().to_owned();

    let mut sources: Vec<PathBuf> = fs::read_dir(src_dir)
        .expect("read the Swift bridge sources")
        .map(|entry| entry.expect("read a Swift bridge source").path())
        .filter(|path| path.extension().is_some_and(|extension| extension == "swift"))
        .collect();
    sources.sort();

    let library = lib_dir.join("libAppleMetalSwiftBridge.a");
    let mut swift = Command::new("xcrun");
    swift.args([
        "swiftc",
        "-emit-library",
        "-static",
        "-module-name",
        "AppleMetalSwiftBridge",
        "-swift-version",
        "5",
        "-O",
        "-whole-module-optimization",
        "-sdk",
        &sdk_path,
        "-target",
        swift_triple,
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
    println!("cargo:rustc-link-lib=static=AppleMetalSwiftBridge");
    println!("cargo:rustc-link-arg=-Wl,-rpath,/usr/lib/swift");

    if let Ok(output) = Command::new("xcode-select").arg("-p").output() {
        if output.status.success() {
            let xcode_path = String::from_utf8_lossy(&output.stdout).trim().to_string();
            let swift_lib_path =
                format!("{xcode_path}/Toolchains/XcodeDefault.xctoolchain/usr/lib/swift/macosx");
            println!("cargo:rustc-link-arg=-Wl,-rpath,{swift_lib_path}");
            println!("cargo:rustc-link-search=native={swift_lib_path}");
        }
    }
}
