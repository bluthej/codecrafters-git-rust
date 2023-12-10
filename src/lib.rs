#[allow(unused)]
use anyhow::{anyhow, Context, Result};
use flate2::{bufread::ZlibDecoder, write::ZlibEncoder, Compression};
use sha1::{Digest, Sha1};
use std::fs::{self, File};
use std::io::{prelude::*, BufReader};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

mod git_object;

use git_object::{GitObject, GitObjectType};

pub fn git_init() -> Result<()> {
    fs::create_dir(".git").context("Create .git directory")?;
    fs::create_dir(".git/objects").context("Create objects directory")?;
    fs::create_dir(".git/refs").context("Create refs directory")?;
    fs::write(".git/HEAD", "ref: refs/heads/master\n").context("Create HEAD file")?;
    println!("Initialized git directory");
    Ok(())
}

// Wrapper function to make the underlying logic testable
pub fn git_cat_file(blob_sha: &str) -> Result<()> {
    _git_cat_file(blob_sha, &mut std::io::stdout())
}

// Implementation based on information in https://wyag.thb.lt/#objects
fn _git_cat_file<W: Write>(blob_sha: &str, writer: &mut W) -> Result<()> {
    let object_bytes = read_object(blob_sha)?;
    let object = GitObject::from_bytes(&object_bytes)?;

    writer.write_all(object.contents.as_bytes())?;

    Ok(())
}

fn read_object(sha: &str) -> Result<Vec<u8>> {
    let path = PathBuf::from(format!(
        ".git/objects/{}/{}", // Objects are stored in .git/objects
        &sha[..2], // They are in a folder named after the first two characters of the hash
        &sha[2..]  // The remaining characters are used for the file name
    ));

    let f = File::open(path)?;
    let reader = BufReader::new(f);

    let mut z = ZlibDecoder::new(reader);
    let mut buffer = Vec::new();
    z.read_to_end(&mut buffer)?;

    Ok(buffer)
}

pub fn git_hash_object(path: &Path) -> Result<()> {
    _git_hash_object(path, &mut std::io::stdout())
}

fn _git_hash_object<W: Write>(path: &Path, writer: &mut W) -> Result<()> {
    // Read file
    let f = File::open(path)?;
    let mut reader = BufReader::new(f);
    let mut file_contents = Vec::new();
    let bytes = reader.read_to_end(&mut file_contents)?;

    // Add header to create a blob
    let blob = [format!("blob {}\x00", bytes).as_bytes(), &file_contents].concat();

    // Create hasher, compute sha1 hash and print it to stdout
    let mut hasher = Sha1::new();
    hasher.update(&blob);
    let hash = hex::encode(hasher.finalize());
    writer.write_all(hash.as_bytes())?;

    // Split hash to get dir name and file name (see `git_cat_file`)
    let (dir_name, file_name) = hash.split_at(2);
    // Create dir if necessary
    let dir_path = format!(".git/objects/{}", dir_name);
    if !PathBuf::from(&dir_path).exists() {
        fs::create_dir(&dir_path).context("Create directory in .git/objects")?;
    }
    let path = format!("{}/{}", dir_path, file_name);
    // Create file
    let mut file = File::create(path)?;

    // Create encoder and compress blob
    let mut e = ZlibEncoder::new(Vec::new(), Compression::default());
    e.write_all(&blob)?;
    let compressed = e.finish()?;

    // Write blob to file
    file.write_all(&compressed)?;

    Ok(())
}

pub fn git_ls_tree(tree_sha: &str) -> Result<()> {
    _git_ls_tree(tree_sha, &mut std::io::stdout())
}

fn _git_ls_tree<W: Write>(tree_sha: &str, writer: &mut W) -> Result<()> {
    let object_bytes = read_object(tree_sha).context("read object")?;
    let object = GitObject::from_bytes(&object_bytes).context("parse git object")?;

    if object.obj_type != GitObjectType::Tree {
        return Err(anyhow!("Expected `tree` object, got: {}", object.obj_type));
    }

    writer.write_all(object.contents.as_bytes())?;

    Ok(())
}

// TODO: clean up these functions!
pub fn git_write_tree() -> Result<()> {
    let hash = read_dir(&PathBuf::from("."))?;
    println!("{}", hex::encode(hash));

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
                let hash = write_blob(&entry)?;
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

    write_obj(&tree)
}

fn write_blob(path: &Path) -> Result<[u8; 20]> {
    // Read file
    let f = File::open(path)?;
    let mut reader = BufReader::new(f);
    let mut file_contents = Vec::new();
    let bytes = reader.read_to_end(&mut file_contents)?;

    // Add header to create a blob
    let blob = [format!("blob {}\x00", bytes).as_bytes(), &file_contents].concat();

    write_obj(&blob)
}

fn write_obj(obj: &[u8]) -> Result<[u8; 20]> {
    // Create hasher, compute sha1 hash and print it to stdout
    let mut hasher = Sha1::new();
    hasher.update(obj);
    let hash_bytes = hasher.finalize().into();
    let hash = hex::encode(hash_bytes);

    // Split hash to get dir name and file name (see `git_cat_file`)
    let (dir_name, file_name) = hash.split_at(2);
    // Create dir if necessary
    let dir_path = format!(".git/objects/{}", dir_name);
    if !PathBuf::from(&dir_path).exists() {
        fs::create_dir(&dir_path).context("Create directory in .git/objects")?;
    }
    let path = format!("{}/{}", dir_path, file_name);
    // Create file
    let mut file = File::create(path)?;

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
        env::set_current_dir,
        fs::{self, File},
        io::Cursor,
        path::Path,
        process::Command,
    };

    use anyhow::Ok;

    use super::*;

    const EMPTY_FILE_HASH: &str = "e69de29bb2d1d6434b8b29ae775ad8c2e48c5391";

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

    fn create_git_repo_with_files(path: &Path) -> Result<()> {
        create_empty_git_repo(path)?;

        fs::create_dir(path.join("src"))?;
        let _ = File::create(path.join("src").join("main.rs"))?;
        let _ = File::create(path.join("Cargo.toml"))?;

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

    fn get_sha(path: &str) -> Result<String> {
        let output = Command::new("git")
            .args(["rev-parse", path])
            .output()
            .context(format!("Get hash of {}", path))?;
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
        set_current_dir(dir.path()).context("cd into temporary directory")?;

        git_init()?;

        assert!(Path::new("./.git/").is_dir());
        assert!(Path::new("./.git/objects/").is_dir());
        assert!(Path::new("./.git/refs/").is_dir());
        assert!(Path::new("./.git/HEAD").is_file());

        dir.close()?;

        Ok(())
    }

    // TODO: figure out why this test fails sometimes
    #[test]
    fn cat_file() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let path = dir.path();
        create_git_repo(path)?;
        set_current_dir(path).context("cd into temporary directory")?;
        let hash = get_sha("HEAD")?;

        let mut buff = Cursor::new(Vec::new());
        _git_cat_file(&hash, &mut buff)?;

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
        set_current_dir(path).context("cd into temporary directory")?;

        let output = Command::new("touch")
            .arg("main.rs")
            .output()
            .context("Create empty file")?;
        if !output.status.success() {
            return Err(anyhow!("Could not create empty file"));
        }

        let mut buff = Cursor::new(Vec::new());
        _git_hash_object(&PathBuf::from("main.rs"), &mut buff)?;

        buff.set_position(0);
        let mut lines = buff.lines();
        assert!(lines
            .next()
            .is_some_and(|line| line.is_ok_and(|line| line.trim() == EMPTY_FILE_HASH)));

        assert!(PathBuf::from(format!(
            ".git/objects/{}/{}",
            &EMPTY_FILE_HASH[..2],
            &EMPTY_FILE_HASH[2..]
        ))
        .exists());

        dir.close()?;

        Ok(())
    }

    // TODO: figure out why this test fails sometimes
    #[test]
    fn ls_tree() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let path = dir.path();
        create_git_repo_with_files(path).context("create git repo with files")?;
        set_current_dir(path).context("cd into temporary directory")?;
        // HEAD is a commit so I have to pass a path in addition to get a tree object
        let hash = get_sha("HEAD:./")?;

        let mut buff = Cursor::new(Vec::new());
        _git_ls_tree(&hash, &mut buff).context("call ls-tree command with hash of root")?;

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
}
