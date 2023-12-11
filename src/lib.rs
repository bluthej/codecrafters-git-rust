#[allow(unused)]
use anyhow::{anyhow, Context, Result};
use flate2::{bufread::ZlibDecoder, write::ZlibEncoder, Compression};
use sha1::{Digest, Sha1};
use std::fs::{self, File};
use std::io::{prelude::*, BufReader};
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

mod git_object;

use git_object::{GitObject, GitObjectType};

pub fn git_init() -> Result<()> {
    _git_init(Path::new("."))
}

fn _git_init(path: &Path) -> Result<()> {
    let dot_git = path.join(".git");
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
fn _git_cat_file<W: Write>(blob_sha: &str, path: &Path, writer: &mut W) -> Result<()> {
    let object_bytes = read_object(blob_sha, path)?;
    let object = GitObject::from_bytes(&object_bytes)?;

    writer.write_all(object.contents.as_bytes())?;

    Ok(())
}

fn read_object(sha: &str, path: &Path) -> Result<Vec<u8>> {
    // Objects are stored in .git/objects
    // They are in a folder named after the first two characters of the hash
    // The remaining characters are used for the file name
    let path = path
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

fn _git_hash_object<W: Write>(file: &Path, path: &Path, writer: &mut W) -> Result<()> {
    let hash_bytes = write_blob(file, path)?;

    writer.write_all(hex::encode(hash_bytes).as_bytes())?;

    Ok(())
}

pub fn git_ls_tree(tree_sha: &str) -> Result<()> {
    _git_ls_tree(tree_sha, Path::new("."), &mut std::io::stdout())
}

fn _git_ls_tree<W: Write>(tree_sha: &str, path: &Path, writer: &mut W) -> Result<()> {
    let object_bytes = read_object(tree_sha, path).context("read object")?;
    let object = GitObject::from_bytes(&object_bytes).context("parse git object")?;

    if object.obj_type != GitObjectType::Tree {
        return Err(anyhow!("Expected `tree` object, got: {}", object.obj_type));
    }

    writer.write_all(object.contents.as_bytes())?;

    Ok(())
}

// TODO: clean up these functions!
pub fn git_write_tree() -> Result<()> {
    _git_write_tree(Path::new("."), &mut std::io::stdout())
}

fn _git_write_tree<W: Write>(path: &Path, writer: &mut W) -> Result<()> {
    let hash = read_dir(path)?;
    writer.write_all(hex::encode(hash).as_bytes())?;

    Ok(())
}

fn read_dir(path: &Path) -> Result<[u8; 20]> {
    let mut tree = Vec::new();
    for entry in fs::read_dir(path)? {
        let entry = entry?.path();
        if let Some(basename) = entry.file_name().and_then(std::ffi::OsStr::to_str) {
            if basename.starts_with('.') {
                continue;
            }
            let (mode, hash) = if entry.is_dir() {
                let hash = read_dir(&entry)?;
                ("40000", hash)
                // tree.push(format!("040000 {}\x00{}", basename, hash));
            } else {
                let mode = entry.metadata()?.permissions().mode();
                let is_exec = mode & 0o111 != 0;
                let mode = if is_exec { "100755" } else { "100644" };
                let hash = write_blob(&entry, path)?;
                (mode, hash)
            };
            tree.push((mode, basename.to_string(), hash));
        }
    }

    tree.sort_unstable_by_key(|(_, basename, _)| basename.clone());
    let tree: Vec<u8> = tree
        .into_iter()
        .flat_map(|(mode, basename, hash)| {
            let mut entry = format!("{} {}\x00", mode, basename).as_bytes().to_vec();
            entry.extend(&hash);
            entry
        })
        .collect();

    let tree = [format!("tree {}\x00", tree.len()).as_bytes(), &tree].concat();

    write_obj(&tree, path)
}

fn write_blob(file: &Path, path: &Path) -> Result<[u8; 20]> {
    // Read file
    let f = File::open(path.join(file))?;
    let mut reader = BufReader::new(f);
    let mut file_contents = Vec::new();
    let bytes = reader.read_to_end(&mut file_contents)?;

    // Add header to create a blob
    let blob = [format!("blob {}\x00", bytes).as_bytes(), &file_contents].concat();

    write_obj(&blob, path)
}

fn write_obj(obj: &[u8], path: &Path) -> Result<[u8; 20]> {
    // Create hasher, compute sha1 hash and print it to stdout
    let mut hasher = Sha1::new();
    hasher.update(obj);
    let hash_bytes = hasher.finalize().into();
    let hash = hex::encode(hash_bytes);

    // Split hash to get dir name and file name (see `git_cat_file`)
    let (dir_name, file_name) = hash.split_at(2);
    // Create dir if necessary
    let dir_path = path.join(".git").join("objects").join(dir_name);
    if !dir_path.exists() {
        fs::create_dir_all(&dir_path).context("Create directory in .git/objects")?;
    }
    let file_path = dir_path.join(file_name);
    // Create file
    let mut file = File::create(file_path)?;

    // Create encoder and compress blob
    let mut e = ZlibEncoder::new(Vec::new(), Compression::default());
    e.write_all(obj)?;
    let compressed = e.finish()?;

    // Write blob to file
    file.write_all(&compressed)?;

    Ok(hash_bytes)
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
        let path = dir.path();

        _git_init(path)?;

        let dot_git = path.join(".git");
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
        let path = dir.path();
        create_git_repo(path)?;
        let hash = get_sha("HEAD", path)?;

        let mut buff = Cursor::new(Vec::new());
        _git_cat_file(&hash, path, &mut buff)?;

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
        let path = dir.path();
        create_empty_git_repo(path)?;

        let output = Command::new("touch")
            .arg("main.rs")
            .current_dir(path)
            .output()
            .context("Create empty file")?;
        if !output.status.success() {
            return Err(anyhow!("Could not create empty file"));
        }

        let mut buff = Cursor::new(Vec::new());
        _git_hash_object(&PathBuf::from("main.rs"), path, &mut buff)?;

        buff.set_position(0);
        let mut lines = buff.lines();
        assert!(lines
            .next()
            .is_some_and(|line| line.is_ok_and(|line| line.trim() == EMPTY_FILE_HASH)));

        assert!(path
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
        let path = dir.path();
        create_git_repo_with_files(path).context("create git repo with files")?;
        // HEAD is a commit so I have to pass a path in addition to get a tree object
        let hash = get_sha("HEAD:./", path)?;

        let mut buff = Cursor::new(Vec::new());
        _git_ls_tree(&hash, path, &mut buff).context("call ls-tree command with hash of root")?;

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
        let path = dir.path();
        create_git_repo_with_uncommited_files(path)
            .context("create git repo with uncommited files")?;

        let mut buff = Cursor::new(Vec::new());
        _git_write_tree(path, &mut buff).context("call write-tree command")?;

        buff.set_position(0);
        let mut lines = buff.lines();
        let hash = REPO_WITH_UNCOMMITED_FILES_HASH;
        assert!(lines
            .next()
            .is_some_and(|line| line.is_ok_and(|line| line.trim() == hash)));
        let objects = path.join(".git").join("objects");
        for entry in fs::read_dir(&objects)? {
            eprintln!("{entry:?}");
        }

        assert!(objects.join(&hash[..2]).is_dir());
        assert!(objects.join(&hash[..2]).join(&hash[2..]).exists());

        Ok(())
    }
}
