//! The bounded diagnostic bundle uses the standard ZIP stored-file format.
use std::io::{self, Seek, Write};

pub fn write(mut output: impl Write + Seek, files: &[(String, Vec<u8>)]) -> io::Result<()> {
    let overflow = |_| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "diagnostic archive is too large",
        )
    };
    let mut directory = Vec::new();
    for (name, data) in files {
        let name = name.as_bytes();
        let name_len = u16::try_from(name.len()).map_err(overflow)?;
        let sized = u32::try_from(data.len()).map_err(overflow)?;
        let offset = u32::try_from(output.stream_position()?).map_err(overflow)?;
        let mut entry = Vec::new();
        entry.extend(20u16.to_le_bytes()); // extraction version
        entry.extend(0x800u16.to_le_bytes()); // UTF-8 names, stored data, no descriptor
        entry.extend([0, 0, 0, 0, 33, 0]); // method and DOS date (1980-01-01)
        entry.extend(crc32fast::hash(data).to_le_bytes());
        entry.extend(sized.to_le_bytes());
        entry.extend(sized.to_le_bytes());
        entry.extend(name_len.to_le_bytes());
        entry.extend([0; 2]); // extra field length
        output.write_all(b"PK\x03\x04")?;
        output.write_all(&entry)?;
        output.write_all(name)?;
        output.write_all(data)?;
        directory.extend(b"PK\x01\x02");
        directory.extend(20u16.to_le_bytes());
        directory.extend(entry);
        directory.extend([0; 10]); // comment, disk and attributes
        directory.extend(offset.to_le_bytes());
        directory.extend(name);
    }
    let start = u32::try_from(output.stream_position()?).map_err(overflow)?;
    let size = u32::try_from(directory.len()).map_err(overflow)?;
    let count = u16::try_from(files.len()).map_err(overflow)?;
    output.write_all(&directory)?;
    output.write_all(b"PK\x05\x06")?;
    output.write_all(&[0; 4])?;
    output.write_all(&count.to_le_bytes())?;
    output.write_all(&count.to_le_bytes())?;
    output.write_all(&size.to_le_bytes())?;
    output.write_all(&start.to_le_bytes())?;
    output.write_all(&[0; 2])
}
