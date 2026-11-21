//! Packs the app-owned Paseo plugins in `paseo-plugins/` into one deterministic tar archive.
//! Compiled twice: into the crate, and into `src-tauri/build.rs` through a `#[path]` module, so it
//! may use only `std` and the `tar` crate (`docs/features/paseo-plugins/02_dev.md`, Phase B).

use std::io;
use std::path::Path;

/// The archive of every `mio-*` subdirectory of `dir` holding a `paseo-plugin.json`, and their
/// sorted ids. Deterministic (`docs/features/paseo-plugins/02_dev.md`, Resolved Decision 8).
pub fn build_plugin_archive(dir: &Path) -> io::Result<(Vec<u8>, Vec<String>)> {
    let mut ids: Vec<String> = Vec::new();
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with("mio-")
            && entry.file_type()?.is_dir()
            && entry.path().join("paseo-plugin.json").is_file()
        {
            ids.push(name);
        }
    }
    ids.sort();

    let mut builder = tar::Builder::new(Vec::new());
    for id in &ids {
        append_tree(&mut builder, &dir.join(id), id)?;
    }
    Ok((builder.into_inner()?, ids))
}

fn append_tree(builder: &mut tar::Builder<Vec<u8>>, path: &Path, rel: &str) -> io::Result<()> {
    builder.append_data(&mut header(tar::EntryType::Directory, 0o755, 0), format!("{rel}/"), io::empty())?;
    let mut entries: Vec<_> = std::fs::read_dir(path)?.collect::<Result<_, _>>()?;
    entries.sort_by_key(|e| e.file_name());
    for entry in entries {
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') || name == "node_modules" {
            continue;
        }
        let child_rel = format!("{rel}/{name}");
        let kind = entry.file_type()?;
        if kind.is_dir() {
            append_tree(builder, &entry.path(), &child_rel)?;
        } else if kind.is_file() {
            let data = std::fs::read(entry.path())?;
            let mut h = header(tar::EntryType::Regular, 0o644, data.len() as u64);
            builder.append_data(&mut h, child_rel, data.as_slice())?;
        }
    }
    Ok(())
}

fn header(kind: tar::EntryType, mode: u32, size: u64) -> tar::Header {
    let mut h = tar::Header::new_ustar();
    h.set_entry_type(kind);
    h.set_mode(mode);
    h.set_uid(0);
    h.set_gid(0);
    h.set_mtime(0);
    h.set_size(size);
    h
}
