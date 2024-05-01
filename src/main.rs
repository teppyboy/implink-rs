#![feature(absolute_path)]
use clap::Parser;
use fs_extra::{dir, file, file::move_file_with_progress};
use std::fs::{create_dir_all, remove_dir_all, remove_file, rename};
#[cfg(not(target_os = "windows"))]
use std::os::unix::fs::symlink;
#[cfg(target_os = "windows")]
use std::os::windows::fs::{symlink_dir, symlink_file};
use std::path::{absolute, PathBuf};

/// File symlinking made easy.
#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Args {
    /// Source file or directory to be symlinked
    src: Option<String>,
    /// Symlink location
    dst: Option<String>,
    /// Force overwrite of existing destination
    #[arg(short, long)]
    force: bool,
    /// Use NTFS junction for directories on Windows
    #[arg(short, long)]
    junction: bool,
    /// Move file or directory to the destination and create a symlink back
    #[arg(short, long)]
    move_and_link: bool,
    /// Generate a mapping file
    #[arg(short, long, default_value_t = String::from("implink-mapping.json"))]
    generate_mapping: String,
    /// Restore mapping from a file
    #[arg(short, long)]
    restore_mapping: Option<String>,
}

fn clear_last_line() {
    // This "works" apparently.
    print!("\r");
    let a = " ".repeat(86);
    print!("{}", a);
    print!("\r");
}

/// Actual symlink implementation for Windows
#[cfg(target_os = "windows")]
fn _make_symlink(src: &PathBuf, dst: &PathBuf, use_junction: bool) -> Result<(), std::io::Error> {
    if src.is_dir() {
        if use_junction {
            return junction::create(src, dst);
        }
        return symlink_dir(src, dst);
    }
    return symlink_file(src, dst);
}

/// Actual symlink implementation for other platforms
#[cfg(not(target_os = "windows"))]
fn _make_symlink(src: &Path, dst: &Path) -> Result<(), std::io::Error> {
    symlink(src, dst)
}

fn move_file_or_directory(src: &PathBuf, dst: &PathBuf, force: bool) -> Result<(), String> {
    if src.is_file() {
        match rename(src, dst) {
            Ok(_) => (),
            Err(e) => {
                return Err(format!(
                    "Failed to move file '{}' to '{}': {}",
                    src.display(),
                    dst.display(),
                    e
                ))
            }
        }
    } else {
        let dir_options = dir::CopyOptions {
            buffer_size: 1024 * 1024,
            ..Default::default()
        };
        let file_options = file::CopyOptions {
            buffer_size: 1024 * 1024,
            ..Default::default()
        };
        let dir_handler = |process_info: dir::TransitProcess| {
            clear_last_line();
            print!(
                "Moving '{}' to '{}'... {}%",
                process_info.file_name,
                dst.display(),
                process_info.copied_bytes * 100 / process_info.total_bytes
            );
            dir::TransitProcessResult::ContinueOrAbort
        };
        if !dst.exists() {
            match create_dir_all(dst) {
                Ok(_) => (),
                Err(e) => {
                    return Err(format!(
                        "Failed to create destination directory '{}': {}",
                        dst.display(),
                        e
                    ))
                }
            }
        } else {
            if !dst.read_dir().unwrap().next().is_none() {
                if !force {
                    return Err(format!(
                        "Destination directory '{}' is not empty",
                        dst.display()
                    ));
                }
                match remove_dir_all(dst) {
                    Ok(_) => (),
                    Err(e) => {
                        return Err(format!(
                            "Failed to remove destination directory '{}': {}",
                            dst.display(),
                            e
                        ))
                    }
                }
                create_dir_all(dst).unwrap();
            }
        }
        for path in src.read_dir().unwrap() {
            let path = path.unwrap().path();
            if path.is_dir() {
                match dir::move_dir_with_progress(path, dst, &dir_options, dir_handler) {
                    Ok(_) => (),
                    Err(e) => {
                        return Err(format!(
                            "Failed to move directory '{}' to '{}': {}",
                            src.display(),
                            dst.display(),
                            e
                        ));
                    }
                }
            } else {
                let path_clone = path.clone();
                let path_clone2 = path.clone();
                let abc = path_clone2.strip_prefix(src).unwrap();
                match move_file_with_progress(
                    path,
                    dst.join(abc),
                    &file_options,
                    |process_info: file::TransitProcess| {
                        clear_last_line();
                        print!(
                            "Moving '{}' to '{}'... {}%",
                            path_clone.display(),
                            dst.display(),
                            process_info.copied_bytes * 100 / process_info.total_bytes
                        );
                    },
                ) {
                    Ok(_) => (),
                    Err(e) => {
                        return Err(format!(
                            "Failed to move directory '{}' to '{}': {}",
                            src.display(),
                            dst.display(),
                            e
                        ));
                    }
                }
            }
        }
    }
    println!("\nMoved '{}' to '{}'.", src.display(), dst.display());
    Ok(())
}

fn make_symlink(
    src: &PathBuf,
    dst: &PathBuf,
    force: bool,
    use_junction: bool,
) -> Result<(), String> {
    if !src.exists() {
        return Err(format!(
            "Source file or directory '{}' does not exist",
            src.display()
        ));
    }
    if dst.exists() {
        if !force {
            return Err(format!(
                "Destination file or directory '{}' already exists",
                dst.display()
            ));
        }
        if dst.is_file() {
            match remove_file(dst) {
                Ok(_) => (),
                Err(e) => {
                    return Err(format!(
                        "Failed to remove destination file '{}': {}",
                        dst.display(),
                        e
                    ))
                }
            }
        } else {
            match remove_dir_all(dst) {
                Ok(_) => (),
                Err(e) => {
                    return Err(format!(
                        "Failed to remove destination directory '{}': {}",
                        dst.display(),
                        e
                    ))
                }
            }
        }
    }
    #[cfg(target_os = "windows")]
    let result = _make_symlink(src, dst, use_junction);
    #[cfg(not(target_os = "windows"))]
    let result = _make_symlink(src, dst);
    match result {
        Ok(_) => (),
        Err(e) => {
            return Err(format!(
                "Failed to create symlink '{}': {}",
                dst.display(),
                e
            ))
        }
    }
    println!(
        "Symlinked '{}' to '{}'",
        src.to_str().unwrap(),
        dst.to_str().unwrap()
    );
    Ok(())
}

fn main() {
    println!(
        "implink-rs v{} - https://github.com/teppyboy/implink-rs",
        env!("CARGO_PKG_VERSION")
    );
    let args = Args::parse();
    if args.src.is_none() || args.dst.is_none() {
        println!("Usage: implink <SRC> <DST>");
        println!("Execute 'implink --help' for more information.");
        return;
    }
    let src = absolute(args.src.unwrap()).unwrap();
    let dst = absolute(args.dst.unwrap()).unwrap();
    if args.move_and_link {
        match move_file_or_directory(&src, &dst, args.force) {
            Ok(_) => {}
            Err(e) => {
                eprintln!("{}", e);
                return;
            }
        }
        match make_symlink(&dst, &src, args.force, args.junction) {
            Ok(_) => {}
            Err(e) => {
                eprintln!("{}", e);
                return;
            }
        }
    } else {
        match make_symlink(&src, &dst, args.force, args.junction) {
            Ok(_) => {}
            Err(e) => {
                eprintln!("{}", e);
                return;
            }
        }
    }
}
