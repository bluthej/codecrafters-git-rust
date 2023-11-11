use std::error::Error;
use std::fmt::Display;
use std::num::ParseIntError;
use std::str::FromStr;

#[allow(dead_code)]
pub(crate) struct GitObject {
    pub(crate) obj_type: GitObjectType,
    pub(crate) size: usize,
    pub(crate) contents: String,
}

pub(crate) enum GitObjectType {
    Blob,
    Commit,
    Tag,
    Tree,
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

impl FromStr for GitObject {
    type Err = ParseGitObjectError;

    // A git object is made up of:
    // - the object type (blob, commit, tag or tree)
    // - an ASCII space
    // - the size of the contents in bytes
    // - a null byte (b"\x00" or '\0')
    // - the contents
    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        let Some((obj_type, rest)) = s.split_once(' ') else {
            return Err(ParseGitObjectError::MissingSpace);
        };

        let obj_type = GitObjectType::from_str(obj_type).map_err(ParseGitObjectTypeError::from)?;

        let Some((size, contents)) = rest.split_once('\0') else {
            return Err(ParseGitObjectError::MissingNullByte);
        };

        let size = size.parse().map_err(ParseGitObjectError::IncorrectSize)?;

        let contents = contents.to_string();

        Ok(Self {
            obj_type,
            size,
            contents,
        })
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
            _ => Err(ParseGitObjectTypeError),
        }
    }
}
