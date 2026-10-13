//! Stable diagnostic codes of the io crate. `docs/diagnostics.md` lists them.

/// A `.msh` file has a syntax error.
pub const MSH_SYNTAX: &str = "MSH001";
/// A `.msh` file has a feature that the import does not support.
pub const MSH_UNSUPPORTED: &str = "MSH002";
/// A `.msh` file defines a node or element twice.
pub const MSH_DUPLICATE: &str = "MSH003";
/// A `.msh` element uses a node that is not defined.
pub const MSH_UNDEFINED: &str = "MSH004";
/// A grid cannot be written because its data is not consistent.
pub const VTU_INVALID_GRID: &str = "VTU001";
/// A VTU or PVD file is malformed.
pub const VTU_MALFORMED: &str = "VTU002";
/// A VTU file has a feature that the reader or writer does not support.
pub const VTU_UNSUPPORTED: &str = "VTU003";
/// A write to the output failed.
pub const VTU_WRITE_FAILED: &str = "VTU004";
