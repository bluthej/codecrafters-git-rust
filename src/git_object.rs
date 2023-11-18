use std::error::Error;
use std::fmt::Display;
use std::num::ParseIntError;
use std::str;
use std::str::FromStr;

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

// TODO: improve error handling
impl GitObject {
    // A git object is made up of:
    // - the object type (blob, commit, tag or tree)
    // - an ASCII space
    // - the size of the contents in bytes
    // - a null byte (b"\x00" or '\0')
    // - the contents
    pub(crate) fn from_bytes(bytes: &[u8]) -> Result<Self, ParseGitObjectError> {
        let Some(space_idx) = bytes.iter().position(|&b| b == b' ') else {
            return Err(ParseGitObjectError::MissingSpace);
        };

        let obj_type: GitObjectType = str::from_utf8(&bytes[..space_idx])
            .map_err(|e| {
                eprintln!("Error parsing the object type: {e}");
                ParseGitObjectError::Type(ParseGitObjectTypeError)
            })?
            .parse()?;

        let Some(null_byte_idx) = bytes[space_idx + 1..].iter().position(|&b| b == b'\0') else {
            return Err(ParseGitObjectError::MissingNullByte);
        };

        let size: usize = str::from_utf8(&bytes[space_idx + 1..][..null_byte_idx])
            .map_err(|e| {
                eprintln!("Error parsing the content size: {e}");
                ParseGitObjectError::MissingSpace
            })?
            .parse()
            .map_err(ParseGitObjectError::IncorrectSize)?;

        let contents = match obj_type {
            GitObjectType::Tree => {
                let mut entries = Vec::new();
                let mut bytes = &bytes[space_idx + null_byte_idx + 2..];
                while let Some((entry, rest)) = parse_tree_entry(bytes) {
                    entries.push(entry.name);
                    bytes = rest;
                }
                entries.join("\n")
            }
            _ => str::from_utf8(&bytes[space_idx + null_byte_idx + 2..])
                .map_err(|_| ParseGitObjectError::MissingSpace)?
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
// TODO: improve error handling + consolidate logic with parsing a GitObject
fn parse_tree_entry(bytes: &[u8]) -> Option<(TreeEntry, &[u8])> {
    if bytes.is_empty() {
        return None;
    }

    let space_idx = bytes.iter().position(|&b| b == b' ').unwrap();
    let mode = str::from_utf8(&bytes[..space_idx])
        .unwrap()
        .parse::<usize>()
        .unwrap();

    let null_byte_idx = bytes[space_idx + 1..]
        .iter()
        .position(|&b| b == b'\0')
        .unwrap();
    let name = str::from_utf8(&bytes[space_idx + 1..][..null_byte_idx])
        .unwrap()
        .to_string();

    let sha1 = hex::encode(&bytes[space_idx + null_byte_idx + 2..]);

    Some((
        TreeEntry { mode, name, sha1 },
        &bytes[space_idx + null_byte_idx + 22..],
    ))
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

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum ParseGitObjectError {
    MissingSpace,
    Type(ParseGitObjectTypeError),
    MissingNullByte,
    IncorrectSize(ParseIntError),
}

impl Display for ParseGitObjectError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MissingSpace => {
                write!(f, "No space found between the object type and the size.")
            }
            Self::Type(_) => write!(f, "Unable to parse the object type."),
            Self::MissingNullByte => {
                write!(f, "No null byte found between the size and the contents.")
            }
            Self::IncorrectSize(e) => write!(f, "Unable to parse the size: {e}"),
        }
    }
}

impl Error for ParseGitObjectError {}

impl From<ParseGitObjectTypeError> for ParseGitObjectError {
    fn from(_value: ParseGitObjectTypeError) -> Self {
        ParseGitObjectError::Type(ParseGitObjectTypeError)
    }
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct ParseGitObjectTypeError;

impl FromStr for GitObjectType {
    type Err = ParseGitObjectTypeError;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        match s {
            "blob" => Ok(Self::Blob),
            "commit" => Ok(Self::Commit),
            "tag" => Ok(Self::Tag),
            "tree" => Ok(Self::Tree),
            _ => {
                eprintln!("Unexpected type, got: {}", s);
                Err(ParseGitObjectTypeError)
            }
        }
    }
}
