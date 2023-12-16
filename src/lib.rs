#[allow(unused)]
use anyhow::{anyhow, Context, Result};
use flate2::bufread::ZlibDecoder;
use std::fs::{self, File};
use std::io::{prelude::*, BufReader};
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

mod git_object;

use git_object::{Object, TreeEntry};

pub fn git_init() -> Result<()> {
    _git_init(Path::new("."))
}

fn _git_init(root: &Path) -> Result<()> {
    let dot_git = root.join(".git");
    fs::create_dir(&dot_git).context("Create .git directory")?;
    fs::create_dir(dot_git.join("objects")).context("Create objects directory")?;
    fs::create_dir(dot_git.join("refs")).context("Create refs directory")?;
    fs::write(dot_git.join("HEAD"), "ref: refs/heads/master\n").context("Create HEAD file")?;
    println!("Initialized git directory");
    Ok(())
}

// Wrapper function to make the underlying logic testable
pub fn git_cat_file(blob_sha: &str) -> Result<()> {
    _git_cat_file(blob_sha, Path::new("."), &mut std::io::stdout())
}

// Implementation based on information in https://wyag.thb.lt/#objects
fn _git_cat_file<W: Write>(blob_sha: &str, root: &Path, writer: &mut W) -> Result<()> {
    let bytes = read_object(blob_sha, root)?;
    let object = Object::from_bytes(&bytes)?;

    writer.write_all(&object.content_bytes())?;

    Ok(())
}

fn read_object(sha: &str, root: &Path) -> Result<Vec<u8>> {
    // Objects are stored in .git/objects
    // They are in a folder named after the first two characters of the hash
    // The remaining characters are used for the file name
    let path = root
        .join(".git")
        .join("objects")
        .join(&sha[..2])
        .join(&sha[2..]);

    let f = File::open(path)?;
    let reader = BufReader::new(f);

    let mut z = ZlibDecoder::new(reader);
    let mut buffer = Vec::new();
    z.read_to_end(&mut buffer)?;

    Ok(buffer)
}

pub fn git_hash_object(file: &Path) -> Result<()> {
    _git_hash_object(file, Path::new("."), &mut std::io::stdout())
}

fn _git_hash_object<W: Write>(file: &Path, root: &Path, writer: &mut W) -> Result<()> {
    let blob = Object::blobify(file, root)?;
    let hash = blob.hash();
    blob.write(root)?;
    writer.write_all(hex::encode(hash).as_bytes())?;

    Ok(())
}

pub fn git_ls_tree(tree_sha: &str) -> Result<()> {
    _git_ls_tree(tree_sha, Path::new("."), &mut std::io::stdout())
}

fn _git_ls_tree<W: Write>(tree_sha: &str, root: &Path, writer: &mut W) -> Result<()> {
    let object_bytes = read_object(tree_sha, root).context("read object")?;
    let object = Object::from_bytes(&object_bytes).context("parse git object")?;

    let Object::Tree(entries) = object else {
        return Err(anyhow!("Expected `tree` object, got: {}", object.kind()));
    };

    let bytes: Vec<u8> = entries
        .into_iter()
        .flat_map(|entry| format!("{}\n", entry.name).as_bytes().to_owned())
        .collect();

    writer.write_all(&bytes)?;

    Ok(())
}

// TODO: clean up these functions!
pub fn git_write_tree() -> Result<()> {
    _git_write_tree(Path::new("."), &mut std::io::stdout())
}

fn _git_write_tree<W: Write>(root: &Path, writer: &mut W) -> Result<()> {
    let hash = read_dir(root)?;
    writer.write_all(hex::encode(hash).as_bytes())?;

    Ok(())
}

fn read_dir(root: &Path) -> Result<[u8; 20]> {
    let mut entries = Vec::new();
    for entry in fs::read_dir(root)? {
        let entry = entry?.path();
        if let Some(basename) = entry.file_name().and_then(std::ffi::OsStr::to_str) {
            if basename.starts_with('.') {
                continue;
            }
            let (mode, hash) = if entry.is_dir() {
                let hash = read_dir(&entry)?;
                (40000, hash)
            } else {
                let mode = entry.metadata()?.permissions().mode();
                let is_exec = mode & 0o111 != 0;
                let mode = if is_exec { 100755 } else { 100644 };
                let blob = Object::blobify(&entry, root)?;
                let hash = blob.hash();
                blob.write(root)?;
                (mode, hash)
            };
            let tree_entry = TreeEntry {
                mode,
                name: basename.to_string(),
                sha1: hash.to_vec(),
            };
            entries.push(tree_entry);
        }
    }

    entries.sort_unstable_by_key(|tree_entry| tree_entry.name.clone());

    let tree = Object::Tree(entries);
    let hash = tree.hash();
    tree.write(root)?;

    Ok(hash)
}

#[cfg(test)]
mod tests {
    use std::{
        fs::{self, File},
        io::Cursor,
        path::{Path, PathBuf},
        process::Command,
    };

    use anyhow::Ok;

    use super::*;

    const EMPTY_FILE_HASH: &str = "e69de29bb2d1d6434b8b29ae775ad8c2e48c5391";
    const REPO_WITH_UNCOMMITED_FILES_HASH: &str = "7fa1ce0ba9e8fcc9d83854e44f48f0f25c477a1c";

    pub fn create_empty_git_repo(path: &Path) -> Result<()> {
        let output = Command::new("git")
            .arg("init")
            .current_dir(path)
            .output()
            .context("Initialize git repo")?;
        if !output.status.success() {
            return Err(anyhow!("Did not initialize git repo successfully"));
        }

        Ok(())
    }

    fn create_git_repo(path: &Path) -> Result<()> {
        create_empty_git_repo(path)?;

        let output = Command::new("git")
            .args(["commit", "--allow-empty", "-m", "'Empty commit'"])
            .current_dir(path)
            .output()
            .context("Make empty commit")?;
        if !output.status.success() {
            return Err(anyhow!("Commit was not successful"));
        }

        Ok(())
    }

    fn create_git_repo_with_uncommited_files(path: &Path) -> Result<()> {
        create_empty_git_repo(path)?;

        fs::create_dir(path.join("src"))?;
        let _ = File::create(path.join("src").join("main.rs"))?;
        let _ = File::create(path.join("Cargo.toml"))?;

        Ok(())
    }

    fn create_git_repo_with_files(path: &Path) -> Result<()> {
        create_git_repo_with_uncommited_files(path)
            .context("Create git repo with uncommited files")?;

        let output = Command::new("git")
            .args(["add", "."])
            .current_dir(path)
            .output()
            .context("Stage new files")?;
        if !output.status.success() {
            return Err(anyhow!("Staging was not successful"));
        }

        let output = Command::new("git")
            .args(["commit", "-m", "'Add files'"])
            .current_dir(path)
            .output()
            .context("Commit changes")?;
        if !output.status.success() {
            return Err(anyhow!("Commit was not successful"));
        }

        Ok(())
    }

    fn get_sha(git_ref: &str, path: &Path) -> Result<String> {
        let output = Command::new("git")
            .args(["rev-parse", git_ref])
            .current_dir(path)
            .output()
            .context(format!("Get hash of {}", git_ref))?;
        if !output.status.success() {
            return Err(anyhow!("Did not get last hash successfully"));
        }

        String::from_utf8(output.stdout)
            .map(|sha| sha.trim().to_string())
            .map_err(From::from)
    }

    #[test]
    fn initialize_repo() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let root = dir.path();

        _git_init(root)?;

        let dot_git = root.join(".git");
        assert!(dot_git.is_dir());
        assert!(dot_git.join("objects").is_dir());
        assert!(dot_git.join("refs").is_dir());
        assert!(dot_git.join("HEAD").is_file());

        dir.close()?;

        Ok(())
    }

    #[test]
    fn cat_file() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let root = dir.path();
        create_git_repo(root)?;
        let hash = get_sha("HEAD", root)?;

        let mut buff = Cursor::new(Vec::new());
        _git_cat_file(&hash, root, &mut buff)?;

        buff.set_position(0);
        let mut lines = buff.lines();
        assert!(lines
            .next()
            .is_some_and(|line| line.is_ok_and(|line| line.starts_with("tree"))));
        assert!(lines
            .next()
            .is_some_and(|line| line.is_ok_and(|line| line.starts_with("author"))));
        assert!(lines
            .next()
            .is_some_and(|line| line.is_ok_and(|line| line.starts_with("committer"))));
        assert!(lines
            .next()
            .is_some_and(|line| line.is_ok_and(|line| line.is_empty())));
        assert!(lines
            .next()
            .is_some_and(|line| line.is_ok_and(|line| !line.is_empty())));

        dir.close()?;

        Ok(())
    }

    #[test]
    fn hash_object() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let root = dir.path();
        create_empty_git_repo(root)?;

        let output = Command::new("touch")
            .arg("main.rs")
            .current_dir(root)
            .output()
            .context("Create empty file")?;
        if !output.status.success() {
            return Err(anyhow!("Could not create empty file"));
        }

        let mut buff = Cursor::new(Vec::new());
        _git_hash_object(&PathBuf::from("main.rs"), root, &mut buff)?;

        buff.set_position(0);
        let mut lines = buff.lines();
        assert!(lines
            .next()
            .is_some_and(|line| line.is_ok_and(|line| line.trim() == EMPTY_FILE_HASH)));

        assert!(root
            .join(".git")
            .join("objects")
            .join(&EMPTY_FILE_HASH[..2])
            .join(&EMPTY_FILE_HASH[2..])
            .exists());

        dir.close()?;

        Ok(())
    }

    #[test]
    fn ls_tree() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let root = dir.path();
        create_git_repo_with_files(root).context("create git repo with files")?;
        // HEAD is a commit so I have to pass a path in addition to get a tree object
        let hash = get_sha("HEAD:./", root)?;

        let mut buff = Cursor::new(Vec::new());
        _git_ls_tree(&hash, root, &mut buff).context("call ls-tree command with hash of root")?;

        buff.set_position(0);
        let mut lines = buff.lines();
        assert!(lines
            .next()
            .is_some_and(|line| line.is_ok_and(|line| line.trim() == "Cargo.toml")));

        assert!(lines
            .next()
            .is_some_and(|line| line.is_ok_and(|line| line.trim() == "src")));

        dir.close()?;

        Ok(())
    }

    #[test]
    fn write_tree() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let root = dir.path();
        create_git_repo_with_uncommited_files(root)
            .context("create git repo with uncommited files")?;

        let mut buff = Cursor::new(Vec::new());
        _git_write_tree(root, &mut buff).context("call write-tree command")?;

        buff.set_position(0);
        for line in buff.clone().lines() {
            dbg!(&line?);
        }
        let mut lines = buff.lines();
        let hash = REPO_WITH_UNCOMMITED_FILES_HASH;
        assert!(lines
            .next()
            .is_some_and(|line| line.is_ok_and(|line| line.trim() == hash)));
        let objects = root.join(".git").join("objects");
        for entry in fs::read_dir(&objects)? {
            eprintln!("{entry:?}");
        }

        assert!(objects.join(&hash[..2]).is_dir());
        assert!(objects.join(&hash[..2]).join(&hash[2..]).exists());

        Ok(())
    }
}
