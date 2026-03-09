use chrono::prelude::*;
use clap::Parser;
use std::fs::{self, Metadata};
use std::os::unix::fs::MetadataExt;
use std::time::UNIX_EPOCH;
use users::{get_group_by_gid, get_user_by_uid};

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

fn format_permissions(mode: u32) -> String {
    let file_type = if mode & 0o040000 != 0 { 'd' } else { '-' };

    let perm_bits = [
        (mode >> 6) & 0o7, // user
        (mode >> 3) & 0o7, // group
        mode & 0o7,        // others
    ];

    let perms: String = perm_bits
        .iter()
        .map(|&bits| {
            ['r', 'w', 'x']
                .iter()
                .enumerate()
                .map(|(i, ch)| if bits & (1 << (2 - i)) != 0 { *ch } else { '-' })
                .collect::<String>()
        })
        .collect();

    format!("{}{}", file_type, perms)
}
