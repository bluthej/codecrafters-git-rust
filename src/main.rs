use anyhow::anyhow;
use anyhow::Context;
use anyhow::Result;
use flate2::bufread::ZlibDecoder;
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
    fs::create_dir(".git")?;
    fs::create_dir(".git/objects")?;
    fs::create_dir(".git/refs")?;
    fs::write(".git/HEAD", "ref: refs/heads/master\n")?;
    println!("Initialized git directory");
    Ok(())
}

fn git_cat_file(blob_sha: &str) -> Result<()> {
    // https://wyag.thb.lt/#objects

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

    print!("{contents}");

    Ok(())
}
