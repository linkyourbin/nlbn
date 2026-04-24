use std::collections::HashSet;
use std::path::Path;

pub fn load_checkpoint(path: &Path) -> HashSet<String> {
    match std::fs::read_to_string(path) {
        Ok(content) => content
            .lines()
            .map(|l| l.trim().to_string())
            .filter(|l| !l.is_empty())
            .collect(),
        Err(_) => HashSet::new(),
    }
}

pub fn append_checkpoint(path: &Path, lcsc_id: &str) {
    use std::io::Write;
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
    {
        let _ = writeln!(f, "{}", lcsc_id);
    }
}
