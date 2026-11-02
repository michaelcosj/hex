// screencapturekit build.rs — builds the ScreenCaptureKitBridge Swift static
// library and links it into the Rust crate.
//
// Vendored for Intel builds: the macOS 15 standalone Command Line Tools cannot
// link a SwiftPM manifest, so compile the bridge with swiftc directly. The
// original SwiftPM package split the bridge into three targets; they only
// import Apple frameworks, so one module produces the same C symbols.

use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

/// A parsed macOS SDK version (`major`, `minor`).
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
struct SdkVersion {
    major: u32,
    minor: u32,
}

impl SdkVersion {
    const fn new(major: u32, minor: u32) -> Self {
        Self { major, minor }
    }
}

impl std::fmt::Display for SdkVersion {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}.{}", self.major, self.minor)
    }
}

fn parse_sdk_version(raw: &str) -> Option<SdkVersion> {
    let mut parts = raw.trim().split('.');
    let major = parts.next()?.trim().parse().ok()?;
    let minor = match parts.next() {
        Some(minor) => minor.trim().parse().ok()?,
        None => 0,
    };
    Some(SdkVersion::new(major, minor))
}

fn detect_sdk_version() -> Option<SdkVersion> {
    let output = Command::new("xcrun")
        .args(["--sdk", "macosx", "--show-sdk-version"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    parse_sdk_version(&String::from_utf8_lossy(&output.stdout))
}

/// Must match the deployment target the former SwiftPM manifest used.
const SWIFT_DEPLOYMENT_TARGET: &str = "13.0";

/// Cargo feature -> minimum macOS SDK -> Swift compile-time define.
const VERSION_FEATURES: [(&str, SdkVersion, Option<&str>); 7] = [
    ("CARGO_FEATURE_MACOS_13_0", SdkVersion::new(13, 0), None),
    (
        "CARGO_FEATURE_MACOS_14_0",
        SdkVersion::new(14, 0),
        Some("SCREENCAPTUREKIT_HAS_MACOS14_SDK"),
    ),
    (
        "CARGO_FEATURE_MACOS_14_2",
        SdkVersion::new(14, 2),
        Some("SCREENCAPTUREKIT_HAS_MACOS14_2_SDK"),
    ),
    (
        "CARGO_FEATURE_MACOS_14_4",
        SdkVersion::new(14, 4),
        Some("SCREENCAPTUREKIT_HAS_MACOS14_4_SDK"),
    ),
    (
        "CARGO_FEATURE_MACOS_15_0",
        SdkVersion::new(15, 0),
        Some("SCREENCAPTUREKIT_HAS_MACOS15_SDK"),
    ),
    (
        "CARGO_FEATURE_MACOS_15_2",
        SdkVersion::new(15, 2),
        Some("SCREENCAPTUREKIT_HAS_MACOS15_2_SDK"),
    ),
    (
        "CARGO_FEATURE_MACOS_26_0",
        SdkVersion::new(26, 0),
        Some("SCREENCAPTUREKIT_HAS_MACOS26_SDK"),
    ),
];

fn configure_swift_version_defines(sdk_version: Option<SdkVersion>) -> Vec<String> {
    let mut define_flags: Vec<String> = Vec::new();
    if sdk_version.is_some_and(|version| version >= SdkVersion::new(13, 1)) {
        define_flags.push("-DSCREENCAPTUREKIT_HAS_MACOS13_1_SDK".to_string());
    }
    let mut stubbed_features: Vec<&str> = Vec::new();
    for (cargo_feature, min_sdk, swift_define) in VERSION_FEATURES {
        if env::var(cargo_feature).is_err() {
            continue;
        }
        if sdk_version.is_some_and(|version| version >= min_sdk) {
            if let Some(swift_define) = swift_define {
                define_flags.push(format!("-D{swift_define}"));
            }
        } else {
            stubbed_features.push(cargo_feature.trim_start_matches("CARGO_FEATURE_"));
        }
    }

    if !stubbed_features.is_empty() {
        let feature_list = stubbed_features.join(", ").to_lowercase();
        let detected = sdk_version.map_or_else(|| "unknown".to_string(), |v| v.to_string());
        panic!(
            "screencapturekit: Cargo feature(s) [{feature_list}] require a newer macOS SDK \
             than the detected SDK ({detected}). Install a matching Xcode, set DEVELOPER_DIR \
             or SDKROOT to it, or remove the unsupported feature(s)."
        );
    }

    define_flags
}

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

    println!("cargo:rustc-link-lib=framework=ScreenCaptureKit");

    let src_root = Path::new("swift-bridge/Sources");
    let out_dir = env::var("OUT_DIR").unwrap();
    let lib_dir = PathBuf::from(&out_dir).join("swift-build");
    fs::create_dir_all(&lib_dir).expect("create the Swift bridge output directory");

    println!("cargo:rerun-if-changed={}", src_root.display());

    let sdk_version = detect_sdk_version();
    assert!(
        sdk_version.is_none_or(|version| version >= SdkVersion::new(13, 0)),
        "screencapturekit: the Swift bridge requires the macOS 13.0 SDK or later; \
         install a newer Xcode or point DEVELOPER_DIR/SDKROOT at it"
    );

    let target_arch = env::var("CARGO_CFG_TARGET_ARCH").unwrap_or_default();
    let swift_arch = match target_arch.as_str() {
        "x86_64" => "x86_64",
        "aarch64" => "arm64",
        other => panic!(
            "screencapturekit: unsupported target arch '{other}'. \
             Expected x86_64 or aarch64."
        ),
    };
    let swift_triple = format!("{swift_arch}-apple-macosx{SWIFT_DEPLOYMENT_TARGET}");

    let define_flags = configure_swift_version_defines(sdk_version);
    let sdk_key = sdk_version.map_or_else(
        || "unknown-sdk".to_string(),
        |version| format!("sdk{}-{}", version.major, version.minor),
    );
    let variant = if define_flags.is_empty() {
        "base".to_string()
    } else {
        let mut hash = 0xcbf2_9ce4_8422_2325_u64;
        for byte in define_flags.iter().flat_map(|flag| flag.bytes().chain([0])) {
            hash ^= u64::from(byte);
            hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
        }
        format!("defines-{hash:016x}")
    };
    let lib_dir = PathBuf::from(&out_dir).join(format!(
        "swift-build-{swift_arch}-{sdk_key}-{variant}"
    ));
    fs::create_dir_all(&lib_dir).expect("create the Swift bridge output directory");

    let sdk = Command::new("xcrun")
        .args(["--sdk", "macosx", "--show-sdk-path"])
        .output()
        .expect("xcrun is required to locate the macOS SDK");
    assert!(sdk.status.success(), "could not locate the macOS SDK");
    let sdk_path = String::from_utf8_lossy(&sdk.stdout).trim().to_owned();

    let mut sources = Vec::new();
    swift_sources(src_root, &mut sources);
    sources.sort();

    let library = lib_dir.join("libScreenCaptureKitBridge.a");
    let mut swift = Command::new("xcrun");
    swift.args([
        "swiftc",
        "-emit-library",
        "-static",
        "-module-name",
        "ScreenCaptureKitBridge",
        "-swift-version",
        "5",
        "-O",
        "-whole-module-optimization",
        "-sdk",
        &sdk_path,
        "-target",
        &swift_triple,
    ]);
    swift.args(&define_flags);
    swift.arg("-o").arg(&library).args(&sources);

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
    println!("cargo:rustc-link-lib=static=ScreenCaptureKitBridge");

    println!("cargo:rustc-link-lib=framework=Foundation");
    println!("cargo:rustc-link-lib=framework=CoreGraphics");
    println!("cargo:rustc-link-lib=framework=CoreMedia");
    println!("cargo:rustc-link-lib=framework=IOSurface");

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
