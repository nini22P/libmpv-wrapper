use std::env;
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use zip::write::SimpleFileOptions;

fn main() {
    let out_dir = PathBuf::from(env::var("CRATE_OUT_DIR").unwrap());
    let manifest_dir = PathBuf::from(env::var("CRATE_MANIFEST_DIR").unwrap());
    let profile = env::var("CRATE_PROFILE").unwrap();

    let target_triple = {
        let os = std::env::consts::OS;
        let arch = std::env::consts::ARCH;
        format!("{}-{}", os, arch)
    };

    println!("Target triple: {}", target_triple);

    println!(
        "Post-build: Processing files in [{}] directory: {}",
        profile,
        out_dir.display()
    );

    let mut final_dylib_name = String::new();

    if let Ok(entries) = fs::read_dir(&out_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_file() {
                continue;
            }

            let file_name = entry.file_name();
            let name_str = file_name.to_string_lossy();
            let mut new_name = String::new();

            // Windows: mpv_wrapper.* -> libmpv-wrapper.*
            if cfg!(windows) && name_str.starts_with("mpv_wrapper.") {
                let suffix = &name_str["mpv_wrapper".len()..];
                let new_suffix = if suffix.starts_with(".dll.") {
                    &suffix[4..]
                } else {
                    suffix
                };
                new_name = format!("libmpv-wrapper{}", new_suffix);
            }
            // All Platforms: libmpv_wrapper.* -> libmpv-wrapper.*
            else if name_str.starts_with("libmpv_wrapper.") {
                let suffix = &name_str["libmpv_wrapper".len()..];
                new_name = format!("libmpv-wrapper{}", suffix);
            }

            if !new_name.is_empty() {
                let new_path = out_dir.join(&new_name);
                match fs::rename(&path, &new_path) {
                    Ok(_) => {
                        println!("Renamed: {} -> {}", name_str, new_name);
                        if new_name.ends_with(".dll")
                            || new_name.ends_with(".so")
                            || new_name.ends_with(".dylib")
                        {
                            final_dylib_name = new_name.clone();
                        }
                    }
                    Err(e) => eprintln!("Failed to rename {}: {}", name_str, e),
                }
            } else {
                if name_str == "libmpv-wrapper.so"
                    || name_str == "libmpv-wrapper.dylib"
                    || name_str == "libmpv-wrapper.dll"
                {
                    final_dylib_name = name_str.into_owned();
                }
            }
        }
    } else {
        eprintln!("Failed to read output directory");
    }

    if profile == "release" {
        println!("Creating release package...");
        let mut files_map: Vec<(PathBuf, String)> = Vec::new();

        if !final_dylib_name.is_empty() {
            files_map.push((
                out_dir.join(&final_dylib_name),
                format!("bin/{}", final_dylib_name),
            ));
        } else {
            eprintln!("Warning: Could not determine main library name for packaging.");
        }

        #[cfg(target_os = "windows")]
        {
            files_map.push((
                out_dir.join("libmpv-wrapper.lib"),
                "lib/libmpv-wrapper.lib".to_string(),
            ));
        }

        files_map.push((
            manifest_dir.join("include/libmpv_wrapper.h"),
            "include/libmpv_wrapper.h".to_string(),
        ));
        files_map.push((manifest_dir.join("LICENSE"), "LICENSE".to_string()));

        let zip_name = format!("libmpv-wrapper-{}.zip", target_triple);

        if let Err(e) = create_zip_archive(&out_dir, &zip_name, files_map) {
            eprintln!("Failed to create zip package: {}", e);
            std::process::exit(1);
        }
    }
}

fn create_zip_archive(
    out_dir: &Path,
    zip_filename: &str,
    files: Vec<(PathBuf, String)>,
) -> Result<(), Box<dyn std::error::Error>> {
    let zip_path = out_dir.join(zip_filename);
    let file = File::create(&zip_path)?;
    let mut zip = zip::ZipWriter::new(file);

    let options = SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated)
        .unix_permissions(0o755);

    for (src_path, dest_path) in files {
        if src_path.exists() {
            println!("  Adding to zip: {} -> {}", src_path.display(), dest_path);

            zip.start_file(dest_path, options)?;

            let mut f = File::open(&src_path)?;
            let mut buffer = Vec::new();
            f.read_to_end(&mut buffer)?;
            zip.write_all(&buffer)?;
        } else {
            println!(
                "  Warning: File not found, skipping: {}",
                src_path.display()
            );
        }
    }

    zip.finish()?;
    println!("Successfully created package: {}", zip_path.display());
    Ok(())
}
