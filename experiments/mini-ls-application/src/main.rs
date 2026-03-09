use std::io::{self, Write};
use std::fs::{self, Metadata};
use std::os::unix::fs::MetadataExt;
use std::time::UNIX_EPOCH;
use users::{get_user_by_uid, get_group_by_gid};
use chrono::prelude::*;

fn main() {
    let stdin = io::stdin();
    loop {
        print!("> ");
        io::stdout().flush().unwrap();

        let mut input = String::new();
        if stdin.read_line(&mut input).is_err() {
            break;
        }

        let input = input.trim();
        if input.is_empty() {
            continue;
        }

        if input == "exit" || input == "quit" {
            break;
        }

        let mut parts = input.split_whitespace();
        let cmd = parts.next().unwrap();
        let args: Vec<&str> = parts.collect();

        match cmd {
            "ls" => run_ls(&args),
            _ => println!("Unknown command: {}", cmd),
        }
    }
}

fn run_ls(args: &[&str]) {
    let mut show_hidden = false;
    let mut long_format = false;
    let mut path = ".";

    for arg in args {
        if arg.starts_with('-') {
            if arg.contains('a') { show_hidden = true; }
            if arg.contains('l') { long_format = true; }
        } else {
            path = arg;
        }
    }

    match fs::read_dir(path) {
        Ok(entries) => {
            let mut files: Vec<_> = entries.filter_map(|e| e.ok()).collect();
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
    let mode = metadata.mode();
    let perms = format_permissions(mode);
    let nlink = metadata.nlink();
    let uid = metadata.uid();
    let gid = metadata.gid();
    let size = metadata.size();
    let mtime = metadata.mtime();

    let user = get_user_by_uid(uid).map(|u| u.name().to_string_lossy().to_string()).unwrap_or(uid.to_string());
    let group = get_group_by_gid(gid).map(|g| g.name().to_string_lossy().to_string()).unwrap_or(gid.to_string());

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
    let mut perms = String::new();
    perms.push(if mode & 0o040000 != 0 { 'd' } else { '-' });
    let flags = [(0o400, 'r'), (0o200, 'w'), (0o100, 'x'),
        (0o040, 'r'), (0o020, 'w'), (0o010, 'x'),
        (0o004, 'r'), (0o002, 'w'), (0o001, 'x')];

    for (bit, ch) in flags.iter() {
        perms.push(if mode & bit != 0 { *ch } else { '-' });
    }

    perms
}