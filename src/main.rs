use anyhow::anyhow;
use anyhow::Context;
use anyhow::Result;
use flate2::bufread::ZlibDecoder;
// use std::env;
use std::fs;
use std::fs::File;
use std::io::prelude::*;
use std::io::BufReader;
use std::path::PathBuf;

use clap::Parser;
use clap::Subcommand;

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
}

fn main() -> Result<()> {
    let cli = Cli::try_parse()?;

    match &cli.command {
        Command::Init => git_init(),
        Command::CatFile { blob_sha } => match blob_sha {
            Some(blob_sha) => git_cat_file(blob_sha),
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
    let path = PathBuf::from(format!(
        ".git/objects/{}/{}", // Objects are stored in .git/objects
        &blob_sha[..2], // They are in a folder named after the first two characters of the hash
        &blob_sha[2..]  // The remaining characters are used for the file name
    ));

    let f = File::open(path)?;
    let reader = BufReader::new(f);

    let mut z = ZlibDecoder::new(reader);
    let mut s = String::new();
    z.read_to_string(&mut s)?;

    // The object should start with a header made up of:
    // - the object type
    // - an ASCII space
    // - the size in bytes
    // - a null byte (b"\x00" or '\0')
    let (_header, contents) = s
        .split_once('\0')
        .ok_or(anyhow!("No null byte found"))
        .context("Strip header")?;

    writer.write_all(contents.as_bytes())?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use std::{env::set_current_dir, io::Cursor, path::Path, process::Command};

    use anyhow::Ok;

    use super::*;

    fn create_git_repo(path: &Path) -> Result<()> {
        let output = Command::new("git")
            .arg("init")
            .current_dir(path)
            .output()
            .context("Initialize git repo")?;
        if !output.status.success() {
            return Err(anyhow!("Did not initialize git repo successfully"));
        }

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
}
