use std::fs::{self, Metadata};
use std::os::unix::fs::MetadataExt;
use std::time::UNIX_EPOCH;
use users::{get_user_by_uid, get_group_by_gid};
use chrono::prelude::*;
use clap::Parser;

#[derive(Parser)]
#[command(author, version, about = "Rust implementation of ls", long_about = None)]
struct Args {
    /// Show hidden files
    #[arg(short = 'a', long)]
    show_hidden: bool,

    /// Long format listing
    #[arg(short = 'l', long)]
    long_format: bool,

    /// Directory path
    #[arg(default_value = ".")]
    path: String,
}

fn main() {
    let args = Args::parse();
    run_ls(&args.path, args.show_hidden, args.long_format);
}

fn run_ls(path: &str, show_hidden: bool, long_format: bool) {
    match fs::read_dir(path) {
        Ok(entries) => {
            let mut files: Vec<_> = entries.filter_map(Result::ok).collect();
            files.sort_by_key(|f| f.file_name());

            for entry in files {
                let name = entry.file_name();
                let name_str = name.to_string_lossy();

                if !show_hidden && name_str.starts_with('.') {
                    continue;
                }

                if long_format {
                    match entry.metadata() {
                        Ok(metadata) => print_long(&metadata, &name_str),
                        Err(_) => println!("{}", name_str),
                    }
                } else {
                    println!("{}", name_str);
                }
            }
        }
        Err(err) => eprintln!("Error reading directory {}: {}", path, err),
    }
}

fn print_long(metadata: &Metadata, name: &str) {
    let perms = format_permissions(metadata.mode());
    let nlink = metadata.nlink();
    let uid = metadata.uid();
    let gid = metadata.gid();
    let size = metadata.size();
    let mtime = metadata.mtime();

    let user = get_user_by_uid(uid)
        .map(|u| u.name().to_string_lossy().to_string())
        .unwrap_or(uid.to_string());
    let group = get_group_by_gid(gid)
        .map(|g| g.name().to_string_lossy().to_string())
        .unwrap_or(gid.to_string());

    let datetime = UNIX_EPOCH + std::time::Duration::new(mtime as u64, 0);
    let datetime: DateTime<Local> = datetime.into();

    println!(
        "{} {:>2} {} {} {:>6} {} {}",
        perms,
        nlink,
        user,
        group,
        size,
        datetime.format("%b %d %H:%M"),
        name
    );
}

fn is_dir(mode: u32) -> char {
    match mode & 0o040000 {
        0 => '-',
        _ => 'd'
    }
}

fn rwx(bits: u32) -> [char; 3] {
    let bits = bits & 0b111;

    [
        match bits & 0b100 { 0 => '-', _ => 'r' },
        match bits & 0b010 { 0 => '-', _ => 'w' },
        match bits & 0b001 { 0 => '-', _ => 'x' }
    ]
}

fn format_permissions(mode: u32) -> String {
    std::iter::once(is_dir(mode))
        .chain(rwx(mode >> 6))
        .chain(rwx(mode >> 3))
        .chain(rwx(mode >> 0))
        .collect()
}