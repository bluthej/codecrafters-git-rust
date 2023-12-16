use anyhow::{anyhow, Context, Ok, Result};
use flate2::{write::ZlibEncoder, Compression};
use sha1::{Digest, Sha1};
use std::fs::{self, File};
use std::io::{prelude::*, BufReader, Read};
use std::os::unix::prelude::PermissionsExt;
use std::path::Path;
use std::str;

pub(crate) enum Object {
    Blob(String),
    Commit(Vec<u8>),
    Tag,
    Tree(Vec<TreeEntry>),
}

impl Object {
    pub(crate) fn blobify(file: &Path, root: &Path) -> Result<Self> {
        let f = File::open(root.join(file))?;
        let mut reader = BufReader::new(f);
        let mut contents = String::new();
        let _bytes = reader.read_to_string(&mut contents)?;
        Ok(Self::Blob(contents))
    }

    pub(crate) fn from_bytes(bytes: &[u8]) -> Result<Self> {
        // A git object is stored as follows:
        // - the object type (blob, commit, tag or tree)
        // - an ASCII space
        // - the size of the contents in bytes
        // - a null byte (b"\x00" or '\0')
        // - the contents
        let Some((obj_type, _size, rest)) = parse_fields(bytes).context("parse fields")? else {
            return Err(anyhow!("No bytes to parse"));
        };

        match obj_type {
            "blob" => Ok(Self::Blob(
                String::from_utf8(rest.to_owned()).context("convert blob bytes to UTF8")?,
            )),
            "commit" => Ok(Self::Commit(rest.to_owned())),
            "tag" => Ok(Self::Tag),
            "tree" => {
                let mut entries = Vec::new();
                let mut bytes = rest;
                while let Some((entry, rest)) =
                    TreeEntry::from_bytes(bytes).context("parse tree entry")?
                {
                    entries.push(entry);
                    bytes = rest;
                }
                Ok(Self::Tree(entries))
            }
            s => Err(anyhow!(
                "object type should be either blob, commit, tag or tree, got: {}",
                s
            )),
        }
    }

    pub(crate) fn to_bytes(&self) -> Vec<u8> {
        let obj_type = self.kind();

        let contents = self.content_bytes();

        let mut bytes = format!("{} {}\x00", obj_type, contents.len())
            .as_bytes()
            .to_owned();
        bytes.extend(contents);

        bytes
    }

    pub(crate) fn kind(&self) -> &str {
        match self {
            Object::Blob(_) => "blob",
            Object::Tree(_) => "tree",
            Object::Commit(_) => "commit",
            Object::Tag => "tag",
        }
    }

    pub(crate) fn content_bytes(&self) -> Vec<u8> {
        match self {
            Object::Blob(blob) => blob.as_bytes().to_owned(),
            Object::Tree(entries) => entries
                .iter()
                .flat_map(|entry| entry.to_bytes().into_iter())
                .collect(),
            Object::Commit(commit) => commit.clone(),
            Object::Tag => unimplemented!(),
        }
    }

    pub(crate) fn hash(&self) -> [u8; 20] {
        let bytes = self.to_bytes();
        let mut hasher = Sha1::new();
        hasher.update(bytes);
        hasher.finalize().into()
    }

    pub(crate) fn write(&self, root: &Path) -> Result<()> {
        let hash = hex::encode(self.hash());

        let (dir_name, file_name) = hash.split_at(2);
        // Create dir if necessary
        let dir_path = root.join(".git").join("objects").join(dir_name);
        if !dir_path.exists() {
            fs::create_dir_all(&dir_path).context("Create directory in .git/objects")?;
        }
        let file_path = dir_path.join(file_name);
        // Create file
        let mut file = File::create(file_path)?;

        // Create encoder and compress object
        let mut e = ZlibEncoder::new(Vec::new(), Compression::default());
        e.write_all(&self.to_bytes())?;
        let compressed = e.finish()?;

        file.write_all(&compressed)?;

        Ok(())
    }
}

pub(crate) struct Tree(Vec<TreeNode>);

enum TreeNode {
    Blob {
        name: String,
        obj: Object,
        mode: usize,
    },
    Tree {
        name: String,
        obj: Tree,
    },
}

impl Tree {
    pub(crate) fn from_working_directory(path: &Path) -> Result<Self> {
        let mut tree = Vec::new();
        for entry in fs::read_dir(path)? {
            let entry = entry?.path();
            if let Some(basename) = entry.file_name().and_then(std::ffi::OsStr::to_str) {
                if basename.starts_with('.') {
                    continue;
                }
                if entry.is_dir() {
                    let sub_tree = Tree::from_working_directory(&entry)?;
                    tree.push(TreeNode::Tree {
                        name: basename.to_string(),
                        obj: sub_tree,
                    });
                } else {
                    let blob = Object::blobify(&entry, path)?;
                    let mode = entry.metadata()?.permissions().mode();
                    let is_exec = mode & 0o111 != 0;
                    let mode = if is_exec { 100755 } else { 100644 };
                    tree.push(TreeNode::Blob {
                        name: basename.to_string(),
                        obj: blob,
                        mode,
                    });
                };
            }
        }

        Ok(Self(tree))
    }

    pub(crate) fn write(&self, root: &Path) -> Result<[u8; 20]> {
        let mut entries = Vec::new();
        for node in &self.0 {
            let (mode, name, hash) = match node {
                TreeNode::Blob { name, obj, mode } => {
                    obj.write(root)?;
                    (*mode, name, obj.hash())
                }
                TreeNode::Tree { name, obj } => (40000, name, obj.write(root)?),
            };
            let tree_entry = TreeEntry {
                mode,
                name: name.to_string(),
                sha1: hash.to_vec(),
            };
            entries.push(tree_entry);
        }

        entries.sort_unstable_by_key(|tree_entry| tree_entry.name.clone());

        let tree = Object::Tree(entries);
        let hash = tree.hash();
        tree.write(root)?;

        Ok(hash)
    }
}

#[derive(Debug)]
pub(crate) struct TreeEntry {
    pub mode: usize,
    pub name: String,
    pub sha1: Vec<u8>,
}

impl TreeEntry {
    // A tree entry is made up of:
    // - a mode
    // - an ASCII space
    // - the file/folder name
    // - a null byte (b"\x00" or '\0')
    // - the sha1 hash
    fn from_bytes(bytes: &[u8]) -> Result<Option<(TreeEntry, &[u8])>> {
        let Some((mode, name, rest)) = parse_fields(bytes).context("parse fields")? else {
            return Ok(None);
        };

        let mode = mode.parse::<usize>().context("parse mode")?;

        let name = name.to_string();

        let sha1 = hex::encode(&rest[..20]).as_bytes().to_owned();

        Ok(Some((Self { mode, name, sha1 }, &rest[20..])))
    }

    fn to_bytes(&self) -> Vec<u8> {
        let mut bytes = format!("{} {}\x00", self.mode, self.name)
            .as_bytes()
            .to_owned();
        bytes.extend(&self.sha1);
        bytes
    }
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
