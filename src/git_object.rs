use anyhow::{anyhow, Context, Ok, Result};
use std::fmt::Display;
use std::str;

#[allow(dead_code)]
pub(crate) struct GitObject {
    pub(crate) obj_type: GitObjectType,
    pub(crate) size: usize,
    pub(crate) contents: String,
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum GitObjectType {
    Blob,
    Commit,
    Tag,
    Tree,
}

#[derive(Debug)]
struct TreeEntry {
    #[allow(unused)]
    mode: usize,
    name: String,
    #[allow(unused)]
    sha1: String,
}

impl GitObject {
    // A git object is made up of:
    // - the object type (blob, commit, tag or tree)
    // - an ASCII space
    // - the size of the contents in bytes
    // - a null byte (b"\x00" or '\0')
    // - the contents
    pub(crate) fn from_bytes(bytes: &[u8]) -> Result<Self> {
        let space_idx = bytes
            .iter()
            .position(|&b| b == b' ')
            .ok_or(anyhow!("Could not find an ASCII space"))?;

        let obj_type = match str::from_utf8(&bytes[..space_idx])
            .context("convert bytes of type field to UTF8")?
        {
            "blob" => GitObjectType::Blob,
            "commit" => GitObjectType::Commit,
            "tag" => GitObjectType::Tag,
            "tree" => GitObjectType::Tree,
            s => {
                return Err(anyhow!(
                    "object type should be either blob, commit, tag or tree, got: {}",
                    s
                ))
            }
        };

        let null_byte_idx = bytes[space_idx + 1..]
            .iter()
            .position(|&b| b == b'\0')
            .ok_or(anyhow!("Could not find a null byte"))?;

        let size: usize = str::from_utf8(&bytes[space_idx + 1..][..null_byte_idx])
            .context("convert bytes of size field to UTF8")?
            .parse()
            .context("parse size")?;

        let contents = match obj_type {
            GitObjectType::Tree => {
                let mut entries = Vec::new();
                let mut bytes = &bytes[space_idx + null_byte_idx + 2..];
                while let Some((entry, rest)) =
                    parse_tree_entry(bytes).context("parse tree entry")?
                {
                    entries.push(entry.name);
                    bytes = rest;
                }
                entries.join("\n")
            }
            _ => str::from_utf8(&bytes[space_idx + null_byte_idx + 2..])
                .context("convert bytes of contents field to UTF8")?
                .to_string(),
        };

        Ok(Self {
            obj_type,
            size,
            contents,
        })
    }
}

// A tree entry is made up of:
// - a mode
// - an ASCII space
// - the file/folder name
// - a null byte (b"\x00" or '\0')
// - the sha1 hash
//
// TODO: consolidate logic with parsing a GitObject
fn parse_tree_entry(bytes: &[u8]) -> Result<Option<(TreeEntry, &[u8])>> {
    if bytes.is_empty() {
        return Ok(None);
    }

    let mut bytes = bytes;

    let space_idx = bytes
        .iter()
        .position(|&b| b == b' ')
        .ok_or(anyhow!("Could not find an ASCII space"))?;
    let mode = str::from_utf8(&bytes[..space_idx])
        .context("convert mode field to UTF8")?
        .parse::<usize>()
        .context("parse mode")?;
    bytes = &bytes[space_idx + 1..];

    let null_byte_idx = bytes
        .iter()
        .position(|&b| b == b'\0')
        .ok_or(anyhow!("Could not find a null byte"))?;
    let name = str::from_utf8(&bytes[..null_byte_idx])
        .context("convert name field to UTF8")?
        .to_string();
    bytes = &bytes[null_byte_idx + 1..];

    let sha1 = hex::encode(&bytes[..20]);

    Ok(Some((TreeEntry { mode, name, sha1 }, &bytes[20..])))
}

impl Display for GitObjectType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}",
            match self {
                Self::Blob => "blob",
                Self::Commit => "commit",
                Self::Tag => "tag",
                Self::Tree => "tree",
            }
        )
    }
}
