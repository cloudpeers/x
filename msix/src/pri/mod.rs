use anyhow::{bail, ensure, Result};
use byteorder::{LittleEndian, ReadBytesExt, WriteBytesExt};
use std::io::{Read, Seek, SeekFrom, Write};

mod data_item;
mod decision_info;
mod hierarchical_schema;
mod pri_descriptor;
mod resource_map;

pub use data_item::DataItem;
pub use decision_info::{Decision, DecisionInfo, Qualifier, QualifierSet, QualifierType};
pub use hierarchical_schema::{HierarchicalSchema, ResourceMapEntry};
pub use pri_descriptor::{PriDescriptor, PriDescriptorFlags};
pub use resource_map::ResourceMap;

#[derive(Clone, Debug, PartialEq)]
pub struct PriFile {
    sections: Vec<Section>,
}

impl PriFile {
    pub const MRM_PRI0: &'static str = "mrm_pri0";
    pub const MRM_PRI1: &'static str = "mrm_pri1";
    pub const MRM_PRI2: &'static str = "mrm_pri2";
    pub const MRM_PRIF: &'static str = "mrm_prif";

    pub fn read<R: Read + Seek>(r: &mut R) -> Result<Self> {
        let mut magic = [0; 8];
        r.read_exact(&mut magic)?;
        let version = match &magic {
            b"mrm_pri0" => Self::MRM_PRI0,
            b"mrm_pri1" => Self::MRM_PRI1,
            b"mrm_pri2" => Self::MRM_PRI2,
            b"mrm_prif" => Self::MRM_PRIF,
            _ => bail!("Data does not start with a PRI file header."),
        };
        ensure!(r.read_u16::<LittleEndian>()? == 0);
        ensure!(r.read_u16::<LittleEndian>()? == 1);
        let total_file_size = r.read_u32::<LittleEndian>()?;
        let toc_offset = r.read_u32::<LittleEndian>()?;
        let section_start_offset = r.read_u32::<LittleEndian>()?;
        let num_sections = r.read_u16::<LittleEndian>()?;
        ensure!(r.read_u16::<LittleEndian>()? == 0xffff);
        ensure!(r.read_u32::<LittleEndian>()? == 0, "expected 0");
        r.seek(SeekFrom::Start(total_file_size as u64 - 16))?;
        ensure!(r.read_u32::<LittleEndian>()? == 0xdefffade);
        ensure!(r.read_u32::<LittleEndian>()? == total_file_size);
        r.read_exact(&mut magic)?;
        ensure!(magic == version.as_bytes());
        r.seek(SeekFrom::Start(toc_offset as u64))?;
        let mut toc = Vec::with_capacity(num_sections as usize);
        for _ in 0..num_sections {
            toc.push(TocEntry::read(r)?);
        }
        let mut sections = Vec::with_capacity(num_sections as usize);
        for i in 0..(num_sections as usize) {
            r.seek(SeekFrom::Start(
                section_start_offset as u64 + toc[i].section_offset as u64,
            ))?;
            sections.push(Section::read(r)?);
        }
        Ok(Self { sections })
    }

    pub fn write<W: Write + Seek>(&self, w: &mut W) -> Result<()> {
        w.write_all(Self::MRM_PRI2.as_bytes())?;
        w.write_u16::<LittleEndian>(0)?;
        w.write_u16::<LittleEndian>(1)?;
        w.write_u32::<LittleEndian>(0)?;
        let toc_offset = 30;
        w.write_u32::<LittleEndian>(toc_offset)?;
        let section_start_offset = self.sections.len() as u64 * 32 + toc_offset as u64;
        w.write_u32::<LittleEndian>(section_start_offset as u32)?;
        w.write_u16::<LittleEndian>(self.sections.len() as u16)?;
        w.write_u16::<LittleEndian>(0xffff)?;
        w.write_u32::<LittleEndian>(0)?;
        for section in &self.sections {
            TocEntry {
                section_identifier: section.data.section_identifier(),
                flags: section.flags,
                section_flags: section.section_flags,
                section_qualifier: section.section_qualifier,
                section_offset: 0,
                section_length: 0,
            }
            .write(w)?;
        }
        for (i, section) in self.sections.iter().enumerate() {
            let start = w.seek(SeekFrom::Current(0))?;
            section.write(w)?;
            let end = w.seek(SeekFrom::Current(0))?;
            let offset = start - section_start_offset;
            let length = end - start;
            w.seek(SeekFrom::Start(toc_offset as u64 + 32 * i as u64 + 24))?;
            w.write_u32::<LittleEndian>(offset as u32)?;
            w.write_u32::<LittleEndian>(length as u32)?;
            w.seek(SeekFrom::Start(end))?;
        }
        let pos = w.seek(SeekFrom::Current(0))?;
        let total_file_size = pos + 16;
        w.write_u32::<LittleEndian>(0xdefffade)?;
        w.write_u32::<LittleEndian>(total_file_size as u32)?;
        w.write_all(Self::MRM_PRI2.as_bytes())?;
        w.seek(SeekFrom::Start(12))?;
        w.write_u32::<LittleEndian>(total_file_size as u32)?;
        Ok(())
    }

    pub fn add_section(&mut self, section: Section) {
        self.sections.push(section);
    }

    pub fn num_sections(&self) -> usize {
        self.sections.len()
    }

    pub fn section(&self, index: usize) -> Option<&Section> {
        self.sections.get(index)
    }
}

#[derive(Clone, Copy, Eq, PartialEq)]
struct TocEntry {
    pub section_identifier: [u8; 16],
    pub flags: u16,
    pub section_flags: u16,
    pub section_qualifier: u32,
    pub section_offset: u32,
    pub section_length: u32,
}

impl std::fmt::Debug for TocEntry {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        let section_identifier = if let Ok(s) = std::str::from_utf8(&self.section_identifier) {
            s.to_string()
        } else {
            format!("{:?}", &self.section_identifier)
        };
        f.debug_struct("SectionHeader")
            .field("section_identifier", &section_identifier)
            .field("flags", &self.flags)
            .field("section_flags", &self.section_flags)
            .field("section_qualifier", &self.section_qualifier)
            .field("section_offset", &self.section_offset)
            .field("section_length", &self.section_length)
            .finish()
    }
}
impl TocEntry {
    pub fn read(r: &mut impl Read) -> Result<Self> {
        let mut section_identifier = [0; 16];
        r.read_exact(&mut section_identifier)?;
        let flags = r.read_u16::<LittleEndian>()?;
        let section_flags = r.read_u16::<LittleEndian>()?;
        let section_qualifier = r.read_u32::<LittleEndian>()?;
        let section_offset = r.read_u32::<LittleEndian>()?;
        let section_length = r.read_u32::<LittleEndian>()?;
        Ok(Self {
            section_identifier,
            flags,
            section_flags,
            section_qualifier,
            section_offset,
            section_length,
        })
    }

    pub fn write(&self, w: &mut impl Write) -> Result<()> {
        w.write_all(&self.section_identifier)?;
        w.write_u16::<LittleEndian>(self.flags)?;
        w.write_u16::<LittleEndian>(self.section_flags)?;
        w.write_u32::<LittleEndian>(self.section_qualifier)?;
        w.write_u32::<LittleEndian>(self.section_offset)?;
        w.write_u32::<LittleEndian>(self.section_length)?;
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Section {
    pub section_qualifier: u32,
    pub flags: u16,
    pub section_flags: u16,
    pub data: SectionData,
}

impl Section {
    pub fn read<R: Read + Seek>(r: &mut R) -> Result<Self> {
        let start = r.seek(SeekFrom::Current(0))?;
        let mut section_identifier = [0; 16];
        r.read_exact(&mut section_identifier)?;
        let section_qualifier = r.read_u32::<LittleEndian>()?;
        let flags = r.read_u16::<LittleEndian>()?;
        let section_flags = r.read_u16::<LittleEndian>()?;
        let section_length = r.read_u32::<LittleEndian>()?;
        ensure!(r.read_u32::<LittleEndian>()? == 0);
        let data = SectionData::read(section_identifier, section_length - 16 - 24, r)?;
        r.seek(SeekFrom::Start(start + section_length as u64 - 8))?;
        ensure!(r.read_u32::<LittleEndian>()? == 0xdef5fade);
        ensure!(r.read_u32::<LittleEndian>()? == section_length);
        Ok(Self {
            section_qualifier,
            flags,
            section_flags,
            data,
        })
    }

    pub fn write<W: Write + Seek>(&self, w: &mut W) -> Result<()> {
        w.write_all(&self.data.section_identifier())?;
        w.write_u32::<LittleEndian>(self.section_qualifier)?;
        w.write_u16::<LittleEndian>(self.flags)?;
        w.write_u16::<LittleEndian>(self.section_flags)?;
        w.write_u32::<LittleEndian>(0)?;
        w.write_u32::<LittleEndian>(0)?;
        let start = w.seek(SeekFrom::Current(0))?;
        self.data.write(w)?;
        let end = w.seek(SeekFrom::Current(0))?;
        let section_length = (end - start) as u32 + 40;
        w.write_u32::<LittleEndian>(0xdef5fade)?;
        w.write_u32::<LittleEndian>(section_length)?;
        w.seek(SeekFrom::Start(start - 8))?;
        w.write_u32::<LittleEndian>(section_length)?;
        w.seek(SeekFrom::Start(end + 8))?;
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum SectionData {
    DataItem(DataItem),
    PriDescriptor(PriDescriptor),
    ResourceMap(ResourceMap),
    DecisionInfo(DecisionInfo),
    HierarchicalSchema(HierarchicalSchema),
    Unknown(UnknownSection),
}

impl SectionData {
    pub fn section_identifier(&self) -> [u8; 16] {
        match self {
            Self::DataItem(_) => *DataItem::IDENTIFIER,
            Self::PriDescriptor(_) => *PriDescriptor::IDENTIFIER,
            Self::ResourceMap(_) => *ResourceMap::IDENTIFIER,
            Self::DecisionInfo(_) => *DecisionInfo::IDENTIFIER,
            Self::HierarchicalSchema(_) => *HierarchicalSchema::IDENTIFIER,
            Self::Unknown(unknown) => unknown.identifier,
        }
    }

    pub fn read<R: Read + Seek>(identifier: [u8; 16], length: u32, r: &mut R) -> Result<Self> {
        match &identifier {
            DataItem::IDENTIFIER => Ok(Self::DataItem(DataItem::read(r)?)),
            PriDescriptor::IDENTIFIER => Ok(Self::PriDescriptor(PriDescriptor::read(r)?)),
            //ResourceMap::IDENTIFIER => Ok(Self::ResourceMap(ResourceMap::read(r)?)),
            DecisionInfo::IDENTIFIER => Ok(Self::DecisionInfo(DecisionInfo::read(r)?)),
            HierarchicalSchema::IDENTIFIER => {
                Ok(Self::HierarchicalSchema(HierarchicalSchema::read(r)?))
            }
            _ => Ok(Self::Unknown(UnknownSection::read(identifier, length, r)?)),
        }
    }

    pub fn write<W: Write + Seek>(&self, w: &mut W) -> Result<()> {
        match self {
            Self::DataItem(section) => section.write(w)?,
            Self::PriDescriptor(section) => section.write(w)?,
            //Self::ResourceMap(section) => section.write(w)?,
            Self::DecisionInfo(section) => section.write(w)?,
            Self::HierarchicalSchema(section) => section.write(w)?,
            Self::Unknown(section) => section.write(w)?,
            _ => {}
        }
        Ok(())
    }
}

#[derive(Clone, Eq, PartialEq)]
pub struct UnknownSection {
    pub identifier: [u8; 16],
    pub data: Vec<u8>,
}

impl UnknownSection {
    pub fn read(identifier: [u8; 16], length: u32, r: &mut impl Read) -> Result<Self> {
        let mut data = Vec::with_capacity(length as usize);
        r.take(length as u64).read_to_end(&mut data)?;
        Ok(Self { identifier, data })
    }

    pub fn write(&self, w: &mut impl Write) -> Result<()> {
        w.write_all(&self.data)?;
        Ok(())
    }
}

impl std::fmt::Debug for UnknownSection {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        let identifier = if let Ok(s) = std::str::from_utf8(&self.identifier) {
            s.to_string()
        } else {
            format!("{:?}", &self.identifier)
        };
        f.debug_struct("SectionHeader")
            .field("identifier", &identifier)
            .field("length", &self.data.len())
            .finish_non_exhaustive()
    }
}
