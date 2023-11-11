use anyhow::{anyhow, Context, Result};
use flate2::{bufread::ZlibDecoder, write::ZlibEncoder, Compression};
use sha1::{Digest, Sha1};
use std::fs::{self, File};
use std::io::{prelude::*, BufReader};
use std::path::{Path, PathBuf};

use clap::{Parser, Subcommand};

mod git_object;

use git_object::GitObject;

#[derive(Parser)]
#[command(author, version, about, long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    Init,
    CatFile {
        #[arg(short = 'p')]
        blob_sha: Option<String>,
    },
    HashObject {
        #[arg(short = 'w')]
        file: Option<PathBuf>,
    },
    LsTree {
        #[arg(long)]
        name_only: bool,
        tree_sha: Option<String>,
    },
}

fn main() -> Result<()> {
    let cli = Cli::try_parse()?;

    match &cli.command {
        Command::Init => git_init(),
        Command::CatFile { blob_sha } => match blob_sha {
            Some(blob_sha) => git_cat_file(blob_sha),
            None => Ok(()),
        },
        Command::HashObject { file } => match file {
            Some(file) => git_hash_object(file),
            None => Ok(()),
        },
        Command::LsTree {
            name_only: _,
            tree_sha,
        } => match tree_sha {
            Some(tree_sha) => git_ls_tree(tree_sha),
            None => Ok(()),
        },
    }
}

fn git_init() -> Result<()> {
    fs::create_dir(".git").context("Create .git directory")?;
    fs::create_dir(".git/objects").context("Create objects directory")?;
    fs::create_dir(".git/refs").context("Create refs directory")?;
    fs::write(".git/HEAD", "ref: refs/heads/master\n").context("Create HEAD file")?;
    println!("Initialized git directory");
    Ok(())
}

// Wrapper function to make the underlying logic testable
fn git_cat_file(blob_sha: &str) -> Result<()> {
    _git_cat_file(blob_sha, &mut std::io::stdout())
}

// Implementation based on information in https://wyag.thb.lt/#objects
fn _git_cat_file<W: Write>(blob_sha: &str, writer: &mut W) -> Result<()> {
    let object: GitObject = read_object(blob_sha)?.parse()?;

    writer.write_all(object.contents.as_bytes())?;

    Ok(())
}

fn read_object(sha: &str) -> Result<String> {
    let path = PathBuf::from(format!(
        ".git/objects/{}/{}", // Objects are stored in .git/objects
        &sha[..2], // They are in a folder named after the first two characters of the hash
        &sha[2..]  // The remaining characters are used for the file name
    ));

    let f = File::open(path)?;
    let reader = BufReader::new(f);

    let mut z = ZlibDecoder::new(reader);
    let mut s = String::new();
    z.read_to_string(&mut s)?;

    Ok(s)
}

fn git_hash_object(path: &Path) -> Result<()> {
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

fn git_ls_tree(_tree_sha: &str) -> Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::{env::set_current_dir, io::Cursor, path::Path, process::Command};

    use anyhow::Ok;

    use super::*;

    const EMPTY_FILE_HASH: &str = "e69de29bb2d1d6434b8b29ae775ad8c2e48c5391";

    fn create_empty_git_repo(path: &Path) -> Result<()> {
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
            .context("Initialize git repo")?;
        if !output.status.success() {
            return Err(anyhow!("Commit was not successful"));
        }

        Ok(())
    }

    fn get_head_sha() -> Result<String> {
        let output = Command::new("git")
            .args(["rev-parse", "HEAD"])
            .output()
            .context("Get hash of HEAD")?;
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

    #[test]
    fn cat_file() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let path = dir.path();
        create_git_repo(path)?;
        set_current_dir(path).context("cd into temporary directory")?;
        let hash = get_head_sha()?;

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
}
