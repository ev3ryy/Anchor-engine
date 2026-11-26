use std::fs::File;
use std::path::Path;

use bytemuck::{Pod, Zeroable, bytes_of, try_from_bytes};
use scene::AssetId;
use scene::cooked::AlignedBytes;

use crate::Error;

pub const ALIGN: u64 = 64;
const MAGIC: u32 = u32::from_le_bytes(*b"PAK1");
const VERSION: u32 = 1;

#[repr(u32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Scene = 1,
    Prefab = 2,
}

impl Kind {
    fn from_u32(value: u32) -> Result<Self, Error> {
        match value {
            1 => Ok(Self::Scene),
            2 => Ok(Self::Prefab),
            _ => Err(Error::Pak("unknown asset kind")),
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct PakHeader {
    magic: u32,
    version: u32,
    toc_offset: u64,
    toc_count: u32,
    _pad: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct TocEntry {
    guid: [u8; 16],
    kind: u32,
    _pad: u32,
    offset: u64,
    size: u64,
    checksum: u64,
}

pub struct Entry<'a> {
    pub id: AssetId,
    pub kind: Kind,
    pub bytes: &'a [u8],
}

pub fn write(entries: &[Entry<'_>]) -> Result<AlignedBytes, Error> {
    let mut order: Vec<usize> = (0..entries.len()).collect();
    order.sort_by(|&left, &right| entries[left].id.0.cmp(&entries[right].id.0));
    for pair in order.windows(2) {
        if entries[pair[0]].id == entries[pair[1]].id {
            return Err(Error::Pak("duplicate guid"));
        }
    }
    let mut buf = vec![0u8; std::mem::size_of::<PakHeader>()];
    pad(&mut buf);
    let mut toc = Vec::with_capacity(entries.len());
    for index in order {
        let entry = &entries[index];
        let offset = buf.len() as u64;
        if offset % ALIGN != 0 {
            return Err(Error::Pak("payload offset is not aligned"));
        }
        buf.extend_from_slice(entry.bytes);
        toc.push(TocEntry {
            guid: entry.id.0,
            kind: entry.kind as u32,
            _pad: 0,
            offset,
            size: entry.bytes.len() as u64,
            checksum: fnv1a64(entry.bytes),
        });
        pad(&mut buf);
    }
    let toc_offset = buf.len() as u64;
    for entry in &toc {
        buf.extend_from_slice(bytes_of(entry));
    }
    let header = PakHeader {
        magic: MAGIC,
        version: VERSION,
        toc_offset,
        toc_count: u32::try_from(toc.len()).map_err(|_| Error::Pak("too many entries"))?,
        _pad: 0,
    };
    buf[..std::mem::size_of::<PakHeader>()].copy_from_slice(bytes_of(&header));
    Ok(AlignedBytes::from_slice(&buf))
}

fn pad(buf: &mut Vec<u8>) {
    let extra = (ALIGN as usize - (buf.len() % ALIGN as usize)) % ALIGN as usize;
    buf.extend(std::iter::repeat_n(0, extra));
}

pub struct Pak<'a> {
    bytes: &'a [u8],
    toc: Vec<TocEntry>,
}

impl<'a> Pak<'a> {
    pub fn parse(bytes: &'a [u8]) -> Result<Self, Error> {
        let header_size = std::mem::size_of::<PakHeader>();
        if bytes.len() < header_size {
            return Err(Error::Pak("truncated header"));
        }
        let header: &PakHeader =
            try_from_bytes(&bytes[..header_size]).map_err(|_| Error::Pak("header alignment"))?;
        if header.magic != MAGIC {
            return Err(Error::Pak("magic"));
        }
        if header.version != VERSION {
            return Err(Error::Pak("version"));
        }
        let toc_size = std::mem::size_of::<TocEntry>()
            .checked_mul(header.toc_count as usize)
            .ok_or(Error::Pak("toc size"))?;
        let toc_offset = header.toc_offset as usize;
        let toc_end = toc_offset
            .checked_add(toc_size)
            .ok_or(Error::Pak("toc end"))?;
        if toc_end > bytes.len() {
            return Err(Error::Pak("truncated toc"));
        }
        let toc: &[TocEntry] =
            try_cast_or(&bytes[toc_offset..toc_end]).map_err(|_| Error::Pak("toc alignment"))?;
        for pair in toc.windows(2) {
            if pair[0].guid >= pair[1].guid {
                return Err(Error::Pak("toc is not sorted"));
            }
        }
        Ok(Self {
            bytes,
            toc: toc.to_vec(),
        })
    }

    pub fn get(&self, id: AssetId) -> Result<(Kind, &'a [u8]), Error> {
        let index = self
            .toc
            .binary_search_by_key(&id.0, |entry| entry.guid)
            .map_err(|_| Error::Pak("missing asset"))?;
        let entry = &self.toc[index];
        if entry.offset % ALIGN != 0 {
            return Err(Error::Pak("payload offset is not aligned"));
        }
        let start = entry.offset as usize;
        let end = start
            .checked_add(entry.size as usize)
            .ok_or(Error::Pak("payload end"))?;
        if end > self.bytes.len() {
            return Err(Error::Pak("truncated payload"));
        }
        let payload = &self.bytes[start..end];
        if fnv1a64(payload) != entry.checksum {
            return Err(Error::Pak("checksum"));
        }
        Ok((Kind::from_u32(entry.kind)?, payload))
    }
}

fn try_cast_or(bytes: &[u8]) -> Result<&[TocEntry], ()> {
    bytemuck::try_cast_slice(bytes).map_err(|_| ())
}

pub struct PakFile {
    map: memmap2::Mmap,
}

impl PakFile {
    pub fn open(path: &Path) -> Result<Self, Error> {
        let file = File::open(path).map_err(|_| Error::Pak("open"))?;
        // The file stays open for the map, and this owner does not write it.
        let map = unsafe { memmap2::Mmap::map(&file) }.map_err(|_| Error::Pak("map"))?;
        Ok(Self { map })
    }

    pub fn pak(&self) -> Result<Pak<'_>, Error> {
        Pak::parse(&self.map)
    }
}

pub fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut hash = 0xcbf29ce484222325u64;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}
