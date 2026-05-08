use std::{fs, path::PathBuf};

fn main() {
    let manifest_dir =
        PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"));
    let repo_root = manifest_dir
        .parent()
        .expect("crate should live under the repository root");
    let proto_dir = repo_root.join("proto");
    let proto = proto_dir.join("patch_tools.proto");
    let versions_toml = repo_root.join("engine/gradle/libs.versions.toml");
    let main_kts_template = manifest_dir.join("templates/main.kts");
    let agents_md_template = manifest_dir.join("templates/AGENTS.md");
    let engine_jar = std::env::var_os("MORPHE_ENGINE_JAR").map_or_else(
        || repo_root.join("engine/build/libs/engine-all.jar"),
        PathBuf::from,
    );

    println!("cargo:rerun-if-env-changed=MORPHE_ENGINE_JAR");
    println!("cargo:rerun-if-changed={}", proto.display());
    println!("cargo:rerun-if-changed={}", versions_toml.display());
    println!("cargo:rerun-if-changed={}", main_kts_template.display());
    println!("cargo:rerun-if-changed={}", agents_md_template.display());
    println!("cargo:rerun-if-changed={}", engine_jar.display());

    assert!(
        engine_jar.exists(),
        "embedded engine JAR not found at {}. Run `cd engine && ./gradlew shadowJar` first.",
        engine_jar.display()
    );

    let engine_jar = engine_jar
        .canonicalize()
        .expect("failed to canonicalize embedded engine JAR path");

    // zstd-compress the JAR into OUT_DIR; engine_jni decompresses in memory at startup.
    let out_dir = PathBuf::from(std::env::var("OUT_DIR").expect("OUT_DIR"));
    let jar_bytes = fs::read(&engine_jar).expect("failed to read embedded engine JAR");
    let compressed_jar = zstd::encode_all(jar_bytes.as_slice(), 19)
        .expect("failed to zstd-compress embedded engine JAR");
    let compressed_jar_path = out_dir.join("engine-all.jar.zst");
    fs::write(&compressed_jar_path, &compressed_jar)
        .expect("failed to write compressed engine JAR to OUT_DIR");
    println!(
        "cargo:rustc-env=MORPHE_ENGINE_JAR_ZST={}",
        compressed_jar_path.display()
    );

    let morphe_patcher_version = format!("v{}", read_version(&versions_toml, "morphe-patcher"));
    let morphe_patches_library_version = format!(
        "v{}",
        read_version(&versions_toml, "morphe-patches-library")
    );
    let rendered_main_kts = fs::read_to_string(&main_kts_template)
        .expect("failed to read main.kts template")
        .replace("{{MORPHE_PATCHER_VERSION}}", &morphe_patcher_version)
        .replace(
            "{{MORPHE_PATCHES_LIBRARY_VERSION}}",
            &morphe_patches_library_version,
        );
    let rendered_agents_md = fs::read_to_string(&agents_md_template)
        .expect("failed to read AGENTS.md template")
        .replace("{{MORPHE_PATCHER_VERSION}}", &morphe_patcher_version)
        .replace(
            "{{MORPHE_PATCHES_LIBRARY_VERSION}}",
            &morphe_patches_library_version,
        );
    let out_dir = PathBuf::from(std::env::var("OUT_DIR").expect("OUT_DIR"));
    fs::write(out_dir.join("main.kts"), rendered_main_kts)
        .expect("failed to write generated main.kts template");
    fs::write(out_dir.join("AGENTS.md"), rendered_agents_md)
        .expect("failed to write generated AGENTS.md template");

    prost_build::Config::new()
        .compile_protos(&[proto], &[proto_dir])
        .expect("failed to compile patch tools protobuf schema");
}

fn read_version(path: &std::path::Path, key: &str) -> String {
    let contents = fs::read_to_string(path).expect("failed to read Gradle version catalog");
    let table: toml::Table = contents
        .parse()
        .expect("failed to parse Gradle version catalog");
    table["versions"][key]
        .as_str()
        .unwrap_or_else(|| panic!("{key} version not found in Gradle version catalog"))
        .to_owned()
}
