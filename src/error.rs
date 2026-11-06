//! Error type for D64 parsing.

use std::fmt;

/// Everything that can go wrong reading a D64 image.
///
/// Every variant is a *rejected malformed input*, not a panic: the parser is
/// written so that no attacker-controlled value can index out of bounds. The
/// variants exist precisely to turn the bugs that a typical C implementation
/// would hit (out-of-range track/sector, truncated files, self-referential
/// directory chains) into recoverable errors.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    /// The file length matches no known D64 geometry.
    UnknownImageSize(usize),
    /// A track/sector pair is outside the disk geometry.
    InvalidTrackSector { track: u8, sector: u8 },
    /// A block would extend past the end of the loaded data area.
    Truncated {
        track: u8,
        sector: u8,
        offset: usize,
        data_len: usize,
    },
    /// The directory chain revisited a sector it had already read (a loop).
    DirectoryCycle { track: u8, sector: u8 },
    /// The directory chain exceeded the maximum plausible length.
    DirectoryTooLong { visited: usize },
    /// A file's data-block chain revisited a sector it had already read.
    FileChainCycle { track: u8, sector: u8 },
    /// A file's data-block chain had more blocks than the disk can hold.
    FileTooLong { blocks: usize },
    /// A REL operation was requested on a non-relative file.
    NotRelativeFile,
    /// A REL file's record length is zero (invalid).
    InvalidRecordLength,
    /// A REL file's directory entry has no side-sector pointer.
    MissingSideSectors,
    /// Not enough free blocks on the disk to write a file.
    DiskFull,
    /// The directory track (18) has no room for another directory sector.
    DirectoryFull,
    /// No directory entry matched the requested name.
    FileNotFound,
    /// A container archive (T64/LNX/ARC) was malformed or not recognized.
    InvalidContainer,
    /// An ARC entry uses a compression method this crate cannot yet decode.
    UnsupportedArcMethod(u8),
    /// An LHA entry uses a compression method this crate cannot yet decode.
    UnsupportedLhaMethod([u8; 5]),
    /// An operation isn't supported for this disk geometry (e.g. the CMD-native
    /// block-availability map, which this crate does not decode).
    UnsupportedGeometry,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::UnknownImageSize(n) => {
                write!(f, "unknown D64 image size: {n} bytes")
            }
            Error::InvalidTrackSector { track, sector } => {
                write!(f, "invalid track/sector {track}/{sector} for this geometry")
            }
            Error::Truncated {
                track,
                sector,
                offset,
                data_len,
            } => write!(
                f,
                "block {track}/{sector} at offset {offset} extends past data area ({data_len} bytes)"
            ),
            Error::DirectoryCycle { track, sector } => {
                write!(f, "directory chain loops back to {track}/{sector}")
            }
            Error::DirectoryTooLong { visited } => {
                write!(f, "directory chain too long ({visited} sectors)")
            }
            Error::FileChainCycle { track, sector } => {
                write!(f, "file chain loops back to {track}/{sector}")
            }
            Error::FileTooLong { blocks } => {
                write!(f, "file chain too long ({blocks} blocks)")
            }
            Error::NotRelativeFile => write!(f, "not a relative (REL) file"),
            Error::InvalidRecordLength => write!(f, "REL record length is zero"),
            Error::MissingSideSectors => write!(f, "REL file has no side-sector pointer"),
            Error::DiskFull => write!(f, "not enough free blocks on the disk"),
            Error::DirectoryFull => write!(f, "directory is full"),
            Error::FileNotFound => write!(f, "file not found"),
            Error::InvalidContainer => write!(f, "malformed or unrecognized container archive"),
            Error::UnsupportedArcMethod(m) => {
                write!(f, "ARC compression method {m} is not yet supported")
            }
            Error::UnsupportedLhaMethod(m) => {
                write!(f, "LHA method {} is not yet supported", String::from_utf8_lossy(m))
            }
            Error::UnsupportedGeometry => {
                write!(f, "operation not supported for this disk geometry")
            }
        }
    }
}

impl std::error::Error for Error {}

/// Crate result alias.
pub type Result<T> = std::result::Result<T, Error>;
