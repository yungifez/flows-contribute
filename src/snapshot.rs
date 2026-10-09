//! Download a public repository at a pinned commit and unpack it safely. Nothing from the
//! repository is run. Only regular files and folders are unpacked: links and special files are
//! skipped, and an archive with an absolute path or `..` is refused as a whole.

use flate2::read::GzDecoder;
use regex::Regex;
use std::fs;
use std::io::Read;
use std::path::{Component, Path, PathBuf};

pub fn valid_repository(value: &str) -> bool {
    Regex::new(r"^[A-Za-z0-9._-]+/[A-Za-z0-9._-]+$")
        .unwrap()
        .is_match(value)
        && !value.split('/').any(|part| part == "." || part == "..")
}

pub fn valid_commit(value: &str) -> bool {
    Regex::new(r"^[0-9a-f]{40}$").unwrap().is_match(value)
}

pub fn valid_dir(value: &str) -> bool {
    Regex::new(r"^[A-Za-z0-9._-]+$").unwrap().is_match(value) && value != "." && value != ".."
}

/// Make sure `root/dir` holds the repository tree. Returns the tree path.
pub fn ensure(
    codeload: &str,
    root: &Path,
    repository: &str,
    commit: &str,
    dir: &str,
) -> Result<PathBuf, String> {
    if !valid_repository(repository) || !valid_commit(commit) || !valid_dir(dir) {
        return Err(format!(
            "The server sent a bad repository entry ({repository}). Nothing was downloaded."
        ));
    }
    let target = root.join(dir);
    if target.is_dir() {
        return Ok(target);
    }
    fs::create_dir_all(root).map_err(|e| format!("I cannot create {}: {e}", root.display()))?;
    println!("  Downloading {repository} ...");
    let part = root.join(format!(".part-{dir}"));
    if part.exists() {
        fs::remove_dir_all(&part).map_err(|e| format!("I cannot clean {}: {e}", part.display()))?;
    }
    fs::create_dir_all(&part).map_err(|e| format!("I cannot create {}: {e}", part.display()))?;
    let url = format!("{codeload}/{repository}/tar.gz/{commit}");
    let response = crate::api::agent().get(&url).call().map_err(|e| {
        format!("The download of {repository} failed ({e}). Check the internet connection and try again.")
    })?;
    let skipped = extract(GzDecoder::new(response.into_reader()), &part).map_err(|e| {
        let _ = fs::remove_dir_all(&part);
        format!("The download of {repository} is not usable: {e}")
    })?;
    if skipped > 0 {
        println!("  Skipped {skipped} links or special files in {repository}.");
    }
    fs::rename(&part, &target).map_err(|e| format!("I cannot move {}: {e}", part.display()))?;
    Ok(target)
}

/// Unpack a tar stream into `dest`, without its first path component. Returns how many entries
/// were skipped because they are not regular files or folders.
pub fn extract<R: Read>(reader: R, dest: &Path) -> Result<usize, String> {
    let mut archive = tar::Archive::new(reader);
    let mut skipped = 0;
    for entry in archive.entries().map_err(|e| e.to_string())? {
        let mut entry = entry.map_err(|e| e.to_string())?;
        let path = entry.path().map_err(|e| e.to_string())?.into_owned();
        let mut relative = PathBuf::new();
        for (index, component) in path.components().enumerate() {
            match component {
                Component::Normal(part) if index > 0 => relative.push(part),
                Component::Normal(_) | Component::CurDir => {}
                _ => {
                    return Err(format!(
                        "the archive has an unsafe path: {}",
                        path.display()
                    ))
                }
            }
        }
        let kind = entry.header().entry_type();
        if kind.is_pax_global_extensions() || kind.is_pax_local_extensions() {
            continue;
        }
        if relative.as_os_str().is_empty() {
            continue;
        }
        let target = dest.join(&relative);
        if kind.is_dir() {
            fs::create_dir_all(&target).map_err(|e| e.to_string())?;
        } else if kind.is_file() {
            if let Some(parent) = target.parent() {
                fs::create_dir_all(parent).map_err(|e| e.to_string())?;
            }
            let mut file = fs::File::create(&target).map_err(|e| e.to_string())?;
            std::io::copy(&mut entry, &mut file).map_err(|e| e.to_string())?;
        } else {
            skipped += 1;
        }
    }
    Ok(skipped)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn header(path: &str, kind: tar::EntryType, size: u64) -> tar::Header {
        let mut header = tar::Header::new_gnu();
        header.set_entry_type(kind);
        header.set_size(size);
        header.set_mode(0o644);
        // Write the raw name so that unsafe paths reach the archive unchanged.
        let name = &mut header.as_old_mut().name;
        name[..path.len()].copy_from_slice(path.as_bytes());
        header.set_cksum();
        header
    }

    fn archive(entries: &[(&str, tar::EntryType, &[u8])]) -> Vec<u8> {
        let mut builder = tar::Builder::new(Vec::new());
        for (path, kind, data) in entries {
            let mut header = header(path, *kind, data.len() as u64);
            if *kind == tar::EntryType::Symlink {
                header.set_link_name("/etc/passwd").unwrap();
                header.set_cksum();
            }
            builder.append(&header, *data).unwrap();
        }
        builder.into_inner().unwrap()
    }

    #[test]
    fn extracts_files_without_the_top_folder() {
        let dir = tempfile::tempdir().unwrap();
        let data = archive(&[
            ("repo-abc/", tar::EntryType::Directory, b""),
            ("repo-abc/app/a.php", tar::EntryType::Regular, b"<?php"),
        ]);
        assert_eq!(extract(&data[..], dir.path()).unwrap(), 0);
        assert_eq!(fs::read(dir.path().join("app/a.php")).unwrap(), b"<?php");
    }

    #[test]
    fn skips_links() {
        let dir = tempfile::tempdir().unwrap();
        let data = archive(&[
            ("repo-abc/link", tar::EntryType::Symlink, b""),
            ("repo-abc/hard", tar::EntryType::Link, b""),
            ("repo-abc/a.txt", tar::EntryType::Regular, b"x"),
        ]);
        assert_eq!(extract(&data[..], dir.path()).unwrap(), 2);
        assert!(fs::symlink_metadata(dir.path().join("link")).is_err());
        assert!(fs::symlink_metadata(dir.path().join("hard")).is_err());
        assert!(dir.path().join("a.txt").is_file());
    }

    #[test]
    fn refuses_parent_and_absolute_paths() {
        let dir = tempfile::tempdir().unwrap();
        for bad in ["repo/../../evil", "/etc/evil", "../evil"] {
            let data = archive(&[(bad, tar::EntryType::Regular, b"x")]);
            assert!(extract(&data[..], dir.path()).is_err(), "{bad}");
        }
        assert!(!dir.path().parent().unwrap().join("evil").exists());
    }

    #[test]
    fn checks_formats() {
        assert!(valid_repository("owner/name.js"));
        assert!(!valid_repository("owner/../x"));
        assert!(!valid_repository("owner/.."));
        assert!(!valid_repository("owner"));
        assert!(valid_commit(&"a".repeat(40)));
        assert!(!valid_commit("ABC"));
        assert!(valid_dir("owner__name-0123456789ab"));
        assert!(!valid_dir("../x"));
        assert!(!valid_dir(".."));
    }
}
