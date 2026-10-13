//! The restart file: binary run state at the end of an increment.
//!
//! Layout (little endian): magic `FHRS`, version (u32), input hash (u64), step (u32),
//! increments done in the step (u32), step complete (u8), time, load factor, next
//! increment size (f64), then five blocks (displacement, step start displacement, integration point state,
//! element internal state, contact state) with a u64 count before each, then the
//! history text (u64 length, bytes) and a checksum (u64, FNV-1a of every byte before it).
//!
//! A file is written under a temporary name and renamed. The newest file is
//! `restart.fh` and the one before it is `restart.fh.prev`. A reader takes the newest
//! file with a valid checksum.

use std::fs::{self, File};
use std::io::{self, BufWriter, Write};
use std::path::Path;

use crate::state::StateImage;

pub const MAGIC: &[u8; 4] = b"FHRS";
pub const VERSION: u32 = 2;
pub const NEWEST: &str = "restart.fh";
pub const PREVIOUS: &str = "restart.fh.prev";

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Restart {
    pub hash: u64,
    /// Step of the last committed increment, from 0.
    pub step: usize,
    /// Increments of that step that are done.
    pub number: usize,
    /// True when that increment ended the step.
    pub complete: bool,
    pub time: f64,
    pub lambda: f64,
    pub next_size: f64,
    /// Committed displacement of all dofs.
    pub displacement: Vec<f64>,
    /// Displacement at the start of the step, when the step is not done. A step with
    /// increments ramps its prescribed values from it. Empty otherwise.
    pub base: Vec<f64>,
    pub image: StateImage,
    /// State of the contact pairs. Empty before M5.
    pub contact: Vec<u8>,
    /// Text for the writer of the results: the lines of the finished increments.
    pub history: String,
}

const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

pub fn fnv64(bytes: &[u8]) -> u64 {
    update(FNV_OFFSET, bytes)
}

fn update(mut h: u64, bytes: &[u8]) -> u64 {
    for &b in bytes {
        h ^= u64::from(b);
        h = h.wrapping_mul(FNV_PRIME);
    }
    h
}

/// Writer that hashes what it writes.
struct Hashing<W: Write> {
    inner: W,
    hash: u64,
}

impl<W: Write> Hashing<W> {
    fn put(&mut self, bytes: &[u8]) -> io::Result<()> {
        self.hash = update(self.hash, bytes);
        self.inner.write_all(bytes)
    }

    /// Write numbers in pieces, so no second copy of a big array exists.
    fn floats(&mut self, v: &[f64]) -> io::Result<()> {
        self.put(&(v.len() as u64).to_le_bytes())?;
        let mut buf = [0u8; 8 * 4096];
        for part in v.chunks(4096) {
            for (i, x) in part.iter().enumerate() {
                buf[8 * i..8 * i + 8].copy_from_slice(&x.to_le_bytes());
            }
            self.put(&buf[..8 * part.len()])?;
        }
        Ok(())
    }

    fn bytes(&mut self, v: &[u8]) -> io::Result<()> {
        self.put(&(v.len() as u64).to_le_bytes())?;
        self.put(v)
    }
}

impl Restart {
    /// Write the file in `dir`: temporary name, then rename. The file that was newest
    /// becomes the previous one.
    pub fn write(&self, dir: &Path) -> io::Result<()> {
        let tmp = dir.join(format!("{NEWEST}.tmp"));
        let file = File::create(&tmp)?;
        let mut w = Hashing {
            inner: BufWriter::new(file),
            hash: FNV_OFFSET,
        };
        w.put(MAGIC)?;
        w.put(&VERSION.to_le_bytes())?;
        w.put(&self.hash.to_le_bytes())?;
        w.put(&(self.step as u32).to_le_bytes())?;
        w.put(&(self.number as u32).to_le_bytes())?;
        w.put(&[u8::from(self.complete)])?;
        for v in [self.time, self.lambda, self.next_size] {
            w.put(&v.to_le_bytes())?;
        }
        w.floats(&self.displacement)?;
        w.floats(&self.base)?;
        w.floats(&self.image.ip)?;
        w.floats(&self.image.internal)?;
        w.bytes(&self.contact)?;
        w.bytes(self.history.as_bytes())?;
        let sum = w.hash;
        w.inner.write_all(&sum.to_le_bytes())?;
        let file = w.inner.into_inner().map_err(|e| e.into_error())?;
        file.sync_all()?;
        drop(file);
        let newest = dir.join(NEWEST);
        if newest.exists() {
            fs::rename(&newest, dir.join(PREVIOUS))?;
        }
        fs::rename(&tmp, newest)
    }

    pub fn decode(bytes: &[u8]) -> Result<Restart, String> {
        if bytes.len() < 16 || &bytes[..4] != MAGIC {
            return Err("not a restart file".into());
        }
        let (body, tail) = bytes.split_at(bytes.len() - 8);
        let want = u64::from_le_bytes(tail.try_into().map_err(|_| "short file")?);
        if fnv64(body) != want {
            return Err("the checksum does not match: the file is damaged".into());
        }
        let mut r = Reader { b: body, at: 4 };
        let version = r.u32()?;
        if version != VERSION {
            return Err(format!("unknown restart format version {version}"));
        }
        let mut out = Restart {
            hash: r.u64()?,
            step: r.u32()? as usize,
            number: r.u32()? as usize,
            complete: r.take(1)?[0] != 0,
            ..Restart::default()
        };
        out.time = r.f64()?;
        out.lambda = r.f64()?;
        out.next_size = r.f64()?;
        out.displacement = r.floats()?;
        out.base = r.floats()?;
        out.image.ip = r.floats()?;
        out.image.internal = r.floats()?;
        let n = r.u64()? as usize;
        out.contact = r.take(n)?.to_vec();
        let n = r.u64()? as usize;
        out.history = String::from_utf8(r.take(n)?.to_vec()).map_err(|_| "bad history text")?;
        if r.at != body.len() {
            return Err("extra bytes in the restart file".into());
        }
        Ok(out)
    }
}

struct Reader<'a> {
    b: &'a [u8],
    at: usize,
}

impl Reader<'_> {
    fn take(&mut self, n: usize) -> Result<&[u8], String> {
        let end = self.at.checked_add(n).filter(|&e| e <= self.b.len());
        let end = end.ok_or("the restart file is too short")?;
        let s = &self.b[self.at..end];
        self.at = end;
        Ok(s)
    }
    fn u32(&mut self) -> Result<u32, String> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into().map_err(|_| "short")?))
    }
    fn u64(&mut self) -> Result<u64, String> {
        Ok(u64::from_le_bytes(self.take(8)?.try_into().map_err(|_| "short")?))
    }
    fn f64(&mut self) -> Result<f64, String> {
        Ok(f64::from_bits(self.u64()?))
    }
    fn floats(&mut self) -> Result<Vec<f64>, String> {
        let n = self.u64()? as usize;
        let raw = self.take(n.checked_mul(8).ok_or("bad count")?)?;
        Ok(raw
            .chunks_exact(8)
            .map(|c| f64::from_le_bytes(c.try_into().unwrap_or([0; 8])))
            .collect())
    }
}

/// Read the newest valid restart file in `dir` and check that it belongs to the input
/// with `hash`. A damaged newest file falls back to the previous one.
pub fn read_latest(dir: &Path, hash: u64) -> Result<Restart, String> {
    let mut problems = Vec::new();
    for name in [NEWEST, PREVIOUS] {
        let path = dir.join(name);
        let Ok(bytes) = fs::read(&path) else {
            continue;
        };
        match Restart::decode(&bytes) {
            Ok(r) if r.hash == hash => return Ok(r),
            Ok(_) => {
                return Err(format!(
                    "{name}: the input file is not the one of the earlier run"
                ))
            }
            Err(e) => problems.push(format!("{name}: {e}")),
        }
    }
    if problems.is_empty() {
        Err(format!("{NEWEST}: no restart file in the run folder"))
    } else {
        Err(problems.join("; "))
    }
}
