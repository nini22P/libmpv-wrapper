use std::env;
use std::fs;
use std::path::PathBuf;

fn main() {
    let out_dir = PathBuf::from(env::var("CRATE_OUT_DIR").unwrap());
    let profile = env::var("CRATE_PROFILE").unwrap();

    println!(
        "Post-build: Processing files in [{}] directory: {}",
        profile,
        out_dir.display()
    );

    match fs::read_dir(&out_dir) {
        Ok(entries) => {
            for entry in entries {
                if let Ok(entry) = entry {
                    let path = entry.path();
                    if !path.is_file() {
                        continue;
                    }

                    let file_name = entry.file_name();
                    let name_str = file_name.to_string_lossy();
                    let mut new_name = String::new();

                    // mpv_wrapper.* -> libmpv-wrapper.*
                    if cfg!(windows) && name_str.starts_with("mpv_wrapper.") {
                        let suffix = &name_str["mpv_wrapper".len()..];

                        let new_suffix = if suffix.starts_with(".dll.") {
                            &suffix[4..]
                        } else {
                            suffix
                        };

                        new_name = format!("libmpv-wrapper{}", new_suffix);
                    }
                    // libmpv_wrapper.* -> libmpv-wrapper.*
                    else if name_str.starts_with("libmpv_wrapper.") {
                        let suffix = &name_str["libmpv_wrapper".len()..];
                        new_name = format!("libmpv-wrapper{}", suffix);
                    }

                    if !new_name.is_empty() {
                        let new_path = out_dir.join(&new_name);

                        match fs::rename(&path, &new_path) {
                            Ok(_) => println!("Renamed: {} -> {}", name_str, new_name),
                            Err(e) => eprintln!("Failed to rename {}: {}", name_str, e),
                        }
                    }
                }
            }
        }
        Err(e) => eprintln!("Failed to read output directory: {}", e),
    }
}
