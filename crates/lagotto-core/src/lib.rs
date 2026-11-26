//! Lagotto: il motore.
//!
//! Regola numero uno: il disco da cui si recupera si apre **solo in lettura**.
//! Questo crate non apre mai un file o un dispositivo sorgente in scrittura e
//! rifiuta qualsiasi destinazione che si trovi sullo stesso disco.

pub mod carve;
pub mod diag;
pub mod disks;
pub mod found;
pub mod helper;
pub mod moments;
pub mod notice;
pub mod recover;
pub mod scan;
pub mod tools;

pub use diag::LagottoError;
pub use disks::{Disk, DiskKind, Probability};
pub use found::{FileKind, FileState, FoundFile};
