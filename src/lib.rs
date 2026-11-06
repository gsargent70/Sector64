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
//! use sector64::Disk;
//! let disk = Disk::load(std::fs::read("disk.d64")?)?;
//! let header = disk.header()?;
//! println!("0 \"{}\" {}", header.name, header.id);
//! for e in disk.directory()? {
//!     println!("{:<4} \"{}\" {}", e.blocks, e.name, e.file_type.mnemonic());
//! }
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```
#![forbid(unsafe_code)]

pub mod arc;
pub mod bam;
pub mod container;
pub mod cvt;
pub mod directory;
pub mod error;
pub mod g64;
pub mod geometry;
pub mod lha;
pub mod lzhuf;
pub mod tap;
pub mod wr3;
pub mod petscii;
pub mod disk;

pub use arc::{parse_arc, parse_sda, ArcEntry, ArcMethod};
pub use cvt::{geos_type_name, parse_cvt, CvtInfo, GeosStructure};
pub use bam::{Bam, DosFormat};
pub use container::{
    parse_lbr, parse_lnx, parse_p00, parse_t64, parse_zipcode, write_zipcode, ImportFile,
};
pub use directory::{Entry, FileType};
pub use g64::to_d64 as g64_to_d64;
pub use lha::{parse_lha, LhaEntry};
pub use tap::{parse_tap, TapImage};
pub use wr3::{parse_wr3, Wr3Entry};
pub use disk::{Disk, Header, RelFile, ValidationReport};
pub use error::{Error, Result};
