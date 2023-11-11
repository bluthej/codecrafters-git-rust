use std::path::PathBuf;

use anyhow::Result;
use clap::{Parser, Subcommand};

use git_starter_rust::{git_cat_file, git_hash_object, git_init, git_ls_tree};

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
