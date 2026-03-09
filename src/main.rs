use std::fs;
use std::path::Path;

const RESET: &str = "\x1b[0m";
const BOLD_BLUE: &str = "\x1b[1;34m";  // directories
const BOLD_GREEN: &str = "\x1b[1;32m"; // executables
const CYAN: &str = "\x1b[0;36m";       // symlinks
const DIM: &str = "\x1b[2m";           // tree lines

fn colored_name(path: &Path) -> String {
    let name = path.file_name().unwrap_or(path.as_os_str()).to_string_lossy();
    if path.is_symlink() {
        format!("{}{}{}", CYAN, name, RESET)
    } else if path.is_dir() {
        format!("{}{}{}", BOLD_BLUE, name, RESET)
    } else if is_executable(path) {
        format!("{}{}{}", BOLD_GREEN, name, RESET)
    } else {
        name.to_string()
    }
}

#[cfg(unix)]
fn is_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    path.metadata()
        .map(|m| m.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}

#[cfg(not(unix))]
fn is_executable(_path: &Path) -> bool {
    false
}

fn print_tree(path: &Path, prefix: &str, is_last: bool) {
    let connector = if is_last { "└── " } else { "├── " };
    println!("{}{}{}{}{}", DIM, prefix, connector, RESET, colored_name(path));

    if path.is_dir() && !path.is_symlink() {
        let child_prefix = format!("{}{}   ", prefix, if is_last { " " } else { "│" });
        let mut entries: Vec<_> = fs::read_dir(path)
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .collect();
        entries.sort();

        for (i, entry) in entries.iter().enumerate() {
            print_tree(entry, &child_prefix, i == entries.len() - 1);
        }
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let root = if args.len() > 1 { &args[1] } else { "." };
    let path = Path::new(root);

    println!("{}{}{}", BOLD_BLUE, path.display(), RESET);
    if path.is_dir() {
        let mut entries: Vec<_> = fs::read_dir(path)
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .collect();
        entries.sort();

        for (i, entry) in entries.iter().enumerate() {
            print_tree(entry, "", i == entries.len() - 1);
        }
    }
}