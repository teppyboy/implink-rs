use clap::Parser;
use fs_extra::{dir, file, file::move_file_with_progress};
use serde::{Deserialize, Serialize};
use std::fs::{
    create_dir_all, read_to_string, remove_dir, remove_dir_all, remove_file, rename, write,
};
#[cfg(not(target_os = "windows"))]
use std::os::unix::fs::symlink;
#[cfg(target_os = "windows")]
use std::os::windows::fs::{symlink_dir, symlink_file};
use std::path::{absolute, PathBuf};
#[cfg(target_os = "windows")]
use std::process::Command;
use terminal_size::terminal_size;

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
    #[arg(short, long)]
    generate_mapping: Option<String>,
    /// Restore mapping from a file
    #[arg(short, long)]
    restore_mapping: Option<String>,
}

#[derive(Serialize, Deserialize, Debug)]
struct Mapping {
    src: String,
    dst: String,
    force: bool,
    junction: bool,
}

#[derive(Serialize, Deserialize, Debug)]
struct MappingFile {
    mapping: Vec<Mapping>,
}

fn clear_last_line() {
    // This "works" apparently.
    let width = match terminal_size() {
        Some((w, _)) => w.0 as usize,
        // Fallback to 86 if terminal size is not available
        None => 86,
    };
    print!("\r{}\r", " ".repeat(width));
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
fn _make_symlink(src: &PathBuf, dst: &PathBuf, _: bool) -> Result<(), std::io::Error> {
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

fn rm_rf(dst: &PathBuf) -> Result<(), String> {
    let mut result: Result<(), String>;
    if dst.is_file() {
        match remove_file(dst) {
            Ok(_) => {
                result = Ok(());
            },
            Err(e) => {
                result = Err(format!(
                    "Failed to remove destination file '{}': {}",
                    dst.display(),
                    e
                ));
            }
        }
    } else {
        match remove_dir_all(dst) {
            Ok(_) => {
                result = Ok(());
            },
            Err(e) => {
                result = Err(format!(
                    "Failed to remove destination directory '{}': {}",
                    dst.display(),
                    e
                ));
            }
        }
    }
    // I do love Windows bro ("truly" :D)
    // Fallback to "del" command which works on Windows :D
    // del documentation: https://learn.microsoft.com/en-us/windows-server/administration/windows-commands/del
    #[cfg(target_os = "windows")]
    {
        if result.is_err() {
            // This should be equivalent to "rm -rf" on Unix-like systems
            let command = Command::new("cmd").args(["/C", "del", "/f", "/q", "/s", dst.to_str().unwrap()]);
            match command.output() {
                Ok(output) => {
                    result = Ok(());
                },
                Err(e) => {
                    result = Err(format!(
                        "Failed to remove destination file or directory '{}': {}",
                        dst.display(),
                        e
                    ));
                }
            }
        }
    }
    return result;
}

fn make_symlink(
    src: &PathBuf,
    dst: &PathBuf,
    force: bool,
    _use_junction: bool,
) -> Result<(), String> {
    if !src.exists() {
        return Err(format!(
            "Source file or directory '{}' does not exist",
            src.display()
        ));
    }
    let dst_exists: bool;
    match dst.try_exists() {
        Ok(result) => {
            dst_exists = result;
        },
        Err(e) => {
            if !force {
                return Err(format!(
                    "Failed to check destination file or directory '{}': {}",
                    dst.display(),
                    e
                ))
            }
            dst_exists = true;
        }
    } 
    if dst_exists {
        if !force {
            match remove_dir(dst) {
                Ok(_) => (),
                Err(_) => {
                    return Err(format!(
                        "Destination file or directory '{}' already exists",
                        dst.display()
                    ));
                }
            }
        } else {
            rm_rf(dst).unwrap();
        }
    }
    let result = _make_symlink(src, dst, _use_junction);
    match result {
        Ok(_) => (),
        Err(e) => {
            if !force {
                return Err(format!(
                    "Failed to create symlink '{}': {}",
                    dst.display(),
                    e
                ));
            }
            // Workaround :)
            println!("Error: {}", e);
            print!("Trying to remove destination file or directory '{}'... ", dst.display());
            match rm_rf(dst) {
                Ok(_) => {
                    println!("OK");
                },
                Err(e) => {
                    println!("FAILED");
                    println!("Failed to remove destination file or directory '{}': {}", dst.display(), e);
                    // Return because we can't even remove the destination
                    return Err(format!(
                        "Failed to create symlink '{}': {}",
                        dst.display(),
                        e
                    ));
                }
            }
            match _make_symlink(src, dst, _use_junction) {
                Ok(_) => (),
                Err(e) => {
                    return Err(format!(
                        "Failed to create symlink '{}': {}",
                        dst.display(),
                        e
                    ));
                }
            }
        }
    }
    println!(
        "Symlinked '{}' to '{}'",
        src.to_str().unwrap(),
        dst.to_str().unwrap()
    );
    Ok(())
}

fn generate_mapping(
    src: &PathBuf,
    dst: &PathBuf,
    force: bool,
    use_junction: bool,
    out_file: &String,
) {
    println!("Generating mapping file...");
    let mapping = Mapping {
        src: src.to_str().unwrap().to_string(),
        dst: dst.to_str().unwrap().to_string(),
        force: force,
        junction: use_junction,
    };
    let mapping_file = MappingFile {
        mapping: vec![mapping],
    };
    let json = serde_json::to_string_pretty(&mapping_file).unwrap();
    write(out_file, json).unwrap();
    println!("Mapping file has been written to '{}'.", out_file);
}

fn restore_mapping(file: &String) {
    println!("Restoring mapping from file '{}'...", file);
    let json = read_to_string(file).unwrap();
    let mapping_file: MappingFile = serde_json::from_str(&json).unwrap();
    for mapping in mapping_file.mapping {
        let src = PathBuf::from(mapping.src);
        let dst = PathBuf::from(mapping.dst);
        match make_symlink(&src, &dst, mapping.force, mapping.junction) {
            Ok(_) => (),
            Err(e) => {
                eprintln!("{}", e);
                return;
            }
        }
    }
    println!("Mapping has been restored.");
}

fn main() {
    println!(
        "implink-rs v{} - https://github.com/teppyboy/implink-rs",
        env!("CARGO_PKG_VERSION")
    );
    let args = Args::parse();
    if args.restore_mapping.is_some() {
        restore_mapping(&args.restore_mapping.unwrap());
        return;
    }
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
        if !args.generate_mapping.is_none() {
            generate_mapping(
                &dst,
                &src,
                args.force,
                args.junction,
                &args.generate_mapping.unwrap(),
            );
        }
    } else {
        match make_symlink(&src, &dst, args.force, args.junction) {
            Ok(_) => {}
            Err(e) => {
                eprintln!("{}", e);
                return;
            }
        }
        if !args.generate_mapping.is_none() {
            generate_mapping(
                &src,
                &dst,
                args.force,
                args.junction,
                &args.generate_mapping.unwrap(),
            );
        }
    }
}
