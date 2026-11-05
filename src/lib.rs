//! A small, clean-room reader for Commodore 1541 **D64** disk images.
//!
//! Written from the publicly documented 1541 format — not derived from any
//! existing tool's code. The design goal beyond "it reads a disk" is that
//! **no attacker-controlled byte can drive an out-of-bounds access**: every
//! track/sector is validated in [`geometry`], every block read checks start
//! *and* length in [`Disk::block`], and the directory walk in
//! [`Disk::directory`] rejects cycles and over-long chains.
//!
//! ```no_run
//! use d64::Disk;
//! let disk = Disk::load(std::fs::read("disk.d64")?)?;
//! let header = disk.header()?;
//! println!("0 \"{}\" {}", header.name, header.id);
//! for e in disk.directory()? {
//!     println!("{:<4} \"{}\" {}", e.blocks, e.name, e.file_type.mnemonic());
//! }
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```
#![forbid(unsafe_code)]

pub mod bam;
pub mod container;
pub mod directory;
pub mod error;
pub mod geometry;
pub mod petscii;
pub mod disk;

pub use bam::{Bam, DosFormat};
pub use container::{parse_lnx, parse_t64, parse_zipcode, write_zipcode, ImportFile};
pub use directory::{Entry, FileType};
pub use disk::{Disk, Header, RelFile, ValidationReport};
pub use error::{Error, Result};
