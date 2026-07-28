//! Atomic writes for derived files (`N-10`).
//!
//! The state file and the board are projections of the journal. A crash halfway
//! through rewriting one must not leave a truncated file that contradicts the
//! journal — so write a sibling temp file, flush it, and rename over the target.
//!
//! The journal itself does **not** go through here: it is append-only (`L-3`),
//! and rewriting it to add a line is exactly what append-only forbids.

use std::fs::{self, File};
use std::io::Write;
use std::path::Path;

use crate::error::{Error, Result};

pub fn write_atomic(path: &Path, contents: &str) -> Result<()> {
    let dir = path.parent().unwrap_or(Path::new("."));
    fs::create_dir_all(dir).map_err(|e| Error::io(dir, e))?;

    // The pid keeps two processes writing the same projection from colliding on
    // the temp name; the rename is what makes the swap atomic.
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(format!(".{}.tmp", std::process::id()));
    let temp = dir.join(name);

    let mut file = File::create(&temp).map_err(|e| Error::io(&temp, e))?;
    file.write_all(contents.as_bytes()).map_err(|e| Error::io(&temp, e))?;
    file.sync_all().map_err(|e| Error::io(&temp, e))?;
    drop(file);

    fs::rename(&temp, path).map_err(|e| Error::io(path, e))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::tmpdir;

    #[test]
    fn writes_a_new_file() {
        let dir = tmpdir("atomic-new");
        let path = dir.join("state.md");
        write_atomic(&path, "first").expect("write");
        assert_eq!(fs::read_to_string(&path).expect("read"), "first");
    }

    #[test]
    fn replaces_an_existing_file_whole() {
        let dir = tmpdir("atomic-replace");
        let path = dir.join("state.md");
        write_atomic(&path, "a longer first version").expect("write");
        write_atomic(&path, "short").expect("rewrite");
        // Not "shortr first version" — the replacement is whole, not in place.
        assert_eq!(fs::read_to_string(&path).expect("read"), "short");
    }

    #[test]
    fn creates_missing_parent_directories() {
        let dir = tmpdir("atomic-parents");
        let path = dir.join("nested/deeper/board.md");
        write_atomic(&path, "board").expect("write");
        assert_eq!(fs::read_to_string(&path).expect("read"), "board");
    }

    #[test]
    fn leaves_no_temp_file_behind() {
        let dir = tmpdir("atomic-clean");
        write_atomic(&dir.join("state.md"), "x").expect("write");
        let leftovers: Vec<_> = fs::read_dir(&dir)
            .expect("read dir")
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|n| n.ends_with(".tmp"))
            .collect();
        assert!(leftovers.is_empty(), "temp files left: {leftovers:?}");
    }
}
