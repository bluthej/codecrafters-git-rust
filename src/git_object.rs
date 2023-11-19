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
        let Some((obj_type, size, rest)) = parse_fields(bytes).context("parse fields")? else {
            return Err(anyhow!("No bytes to parse"));
        };

        let obj_type = match obj_type {
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

        let size: usize = size.parse().context("parse size")?;

        let contents = match obj_type {
            GitObjectType::Tree => {
                let mut entries = Vec::new();
                let mut bytes = rest;
                while let Some((entry, rest)) =
                    parse_tree_entry(bytes).context("parse tree entry")?
                {
                    entries.push(entry.name);
                    bytes = rest;
                }
                entries.join("\n")
            }
            _ => str::from_utf8(rest)
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
fn parse_tree_entry(bytes: &[u8]) -> Result<Option<(TreeEntry, &[u8])>> {
    let Some((mode, name, rest)) = parse_fields(bytes).context("parse fields")? else {
        return Ok(None);
    };

    let mode: usize = mode.parse().context("parse mode")?;

    let name = name.to_string();

    let sha1 = hex::encode(&rest[..20]);

    Ok(Some((TreeEntry { mode, name, sha1 }, &rest[20..])))
}

// There is a recurring logic of fields to parse:
// [field] [field]\x00[rest]
fn parse_fields(bytes: &[u8]) -> Result<Option<(&str, &str, &[u8])>> {
    if bytes.is_empty() {
        return Ok(None);
    }

    let mut bytes = bytes;

    let space_idx = bytes
        .iter()
        .position(|&b| b == b' ')
        .ok_or(anyhow!("Could not find an ASCII space"))?;
    let field1 = str::from_utf8(&bytes[..space_idx]).context("convert mode field to UTF8")?;
    bytes = &bytes[space_idx + 1..];

    let null_byte_idx = bytes
        .iter()
        .position(|&b| b == b'\0')
        .ok_or(anyhow!("Could not find a null byte"))?;
    let field2 = str::from_utf8(&bytes[..null_byte_idx]).context("convert name field to UTF8")?;
    bytes = &bytes[null_byte_idx + 1..];

    Ok(Some((field1, field2, bytes)))
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
