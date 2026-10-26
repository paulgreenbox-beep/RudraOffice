use super::*;

pub const ZIP_LOCAL_FILE_SIGNATURE: u32 = 0x04034b50;
pub const ZIP_CENTRAL_DIRECTORY_SIGNATURE: u32 = 0x02014b50;
pub const ZIP_END_OF_CENTRAL_DIRECTORY_SIGNATURE: u32 = 0x06054b50;

pub const ZIP_METHOD_STORED: u16 = 0;
pub const ZIP_METHOD_DEFLATE: u16 = 8;

// Generous parser limits for ordinary DOCX packages, enforced before copying input data.
const MAX_ARCHIVE_SIZE: usize = 256 * 1024 * 1024;
const MAX_ENTRY_COUNT: usize = 16_384;
const MAX_ENTRY_SIZE: usize = 64 * 1024 * 1024;
const MAX_TOTAL_UNCOMPRESSED_SIZE: usize = 256 * 1024 * 1024;
const MAX_ENTRY_NAME_SIZE: usize = 4 * 1024;
const MAX_EOCD_SEARCH_SIZE: usize = u16::MAX as usize + 22;
const ZIP_UTF8_FLAG: u16 = 1 << 11;
const ZIP_ALLOWED_FLAGS: u16 = ZIP_UTF8_FLAG;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ZipEntry {
    pub name: String,
    pub content_type: String,
    pub data: FlatOpcData,
    pub compression: ZipCompression,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ZipCompression {
    Stored,
    Deflated,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ZipPackage {
    pub entries: Vec<ZipEntry>,
}

pub type DocxPackage = ZipPackage;

fn crc32_table() -> [u32; 256] {
    let mut table = [0u32; 256];
    for i in 0..256u32 {
        let mut crc = i;
        for _ in 0..8 {
            if crc & 1 != 0 {
                crc = (crc >> 1) ^ 0xEDB88320;
            } else {
                crc >>= 1;
            }
        }
        table[i as usize] = crc;
    }
    table
}

fn crc32(data: &[u8]) -> u32 {
    let table = crc32_table();
    let mut crc: u32 = 0xFFFF_FFFF;
    for &byte in data {
        let index = ((crc ^ u32::from(byte)) & 0xFF) as usize;
        crc = (crc >> 8) ^ table[index];
    }
    crc ^ 0xFFFF_FFFF
}

pub fn write_u16_le(buf: &mut Vec<u8>, value: u16) {
    buf.push(value as u8);
    buf.push((value >> 8) as u8);
}

pub fn write_u32_le(buf: &mut Vec<u8>, value: u32) {
    buf.push(value as u8);
    buf.push((value >> 8) as u8);
    buf.push((value >> 16) as u8);
    buf.push((value >> 24) as u8);
}

pub fn write_crc32(buf: &mut Vec<u8>, value: u32) {
    write_u32_le(buf, value);
}

fn write_local_file_header(
    buf: &mut Vec<u8>,
    name: &str,
    compressed_size: u32,
    uncompressed_size: u32,
    crc: u32,
    method: u16,
) {
    write_u32_le(buf, ZIP_LOCAL_FILE_SIGNATURE);
    write_u16_le(buf, 20); // version needed
    write_u16_le(buf, 0); // general purpose bit flag
    write_u16_le(buf, method);
    write_u16_le(buf, 0); // last mod time
    write_u16_le(buf, 0); // last mod date
    write_crc32(buf, crc);
    write_u32_le(buf, compressed_size);
    write_u32_le(buf, uncompressed_size);
    write_u16_le(
        buf,
        u16::try_from(name.len()).expect("entry name length fits in u16"),
    );
    write_u16_le(buf, 0); // extra field length
    buf.extend_from_slice(name.as_bytes());
}

fn write_central_directory_entry(
    buf: &mut Vec<u8>,
    name: &str,
    compressed_size: u32,
    uncompressed_size: u32,
    crc: u32,
    method: u16,
    local_header_offset: u32,
) {
    write_u32_le(buf, ZIP_CENTRAL_DIRECTORY_SIGNATURE);
    write_u16_le(buf, 20); // version made by
    write_u16_le(buf, 20); // version needed
    write_u16_le(buf, 0); // general purpose bit flag
    write_u16_le(buf, method);
    write_u16_le(buf, 0); // last mod time
    write_u16_le(buf, 0); // last mod date
    write_crc32(buf, crc);
    write_u32_le(buf, compressed_size);
    write_u32_le(buf, uncompressed_size);
    write_u16_le(
        buf,
        u16::try_from(name.len()).expect("entry name length fits in u16"),
    );
    write_u16_le(buf, 0); // extra field length
    write_u16_le(buf, 0); // file comment length
    write_u16_le(buf, 0); // disk number start
    write_u16_le(buf, 0); // internal file attributes
    write_u32_le(buf, 0); // external file attributes
    write_u32_le(buf, local_header_offset);
    buf.extend_from_slice(name.as_bytes());
}

fn write_eocd(buf: &mut Vec<u8>, central_dir_size: u32, central_dir_offset: u32, entry_count: u16) {
    write_u32_le(buf, ZIP_END_OF_CENTRAL_DIRECTORY_SIGNATURE);
    write_u16_le(buf, 0); // disk number
    write_u16_le(buf, 0); // disk with central dir
    write_u16_le(buf, entry_count);
    write_u16_le(buf, entry_count);
    write_u32_le(buf, central_dir_size);
    write_u32_le(buf, central_dir_offset);
    write_u16_le(buf, 0); // comment length
}

/// Serializer-side limits, defaulting to the same constants the parser enforces.
/// Module-private: only `serialize_zip` is part of the public API.
#[derive(Clone, Copy, Debug)]
struct SerializeLimits {
    max_archive_size: usize,
    max_entry_size: usize,
    max_entry_count: usize,
}

impl Default for SerializeLimits {
    fn default() -> Self {
        Self {
            max_archive_size: MAX_ARCHIVE_SIZE,
            max_entry_size: MAX_ENTRY_SIZE,
            max_entry_count: MAX_ENTRY_COUNT,
        }
    }
}

fn stored_payload(data: &FlatOpcData) -> &[u8] {
    match data {
        FlatOpcData::Xml(xml) => xml.as_bytes(),
        FlatOpcData::Binary(bytes) => bytes.as_slice(),
    }
}

pub fn serialize_zip(package: &DocxPackage) -> Result<Vec<u8>, DocxError> {
    serialize_zip_with_limits(package, &SerializeLimits::default())
}

fn serialize_zip_with_limits(
    package: &DocxPackage,
    limits: &SerializeLimits,
) -> Result<Vec<u8>, DocxError> {
    let entry_count = package.entries.len();
    if entry_count > limits.max_entry_count {
        return Err(DocxError::new(
            DocxErrorKind::InvalidPackage,
            "ZIP entry count exceeds the supported limit",
        ));
    }
    let entry_count_u16 = u16::try_from(entry_count).map_err(|_| {
        DocxError::new(
            DocxErrorKind::InvalidPackage,
            "entry count exceeds u16 maximum",
        )
    })?;

    // Preflight: validate entries and compute the exact output size (local
    // headers + payloads + central directory + EOCD) before any allocation.
    let mut local_offsets = Vec::with_capacity(entry_count);
    let mut central_dir_size: usize = 0;
    let mut cursor: usize = 0;
    for entry in &package.entries {
        let name = &entry.name;
        if name.is_empty() {
            return Err(DocxError::new(
                DocxErrorKind::InvalidPackage,
                "ZIP entry name is empty",
            ));
        }
        if name.len() > MAX_ENTRY_NAME_SIZE {
            return Err(DocxError::new(
                DocxErrorKind::InvalidPackage,
                "ZIP entry name exceeds the supported size limit",
            ));
        }
        if !zip_name_is_safe(name) {
            return Err(DocxError::new(
                DocxErrorKind::InvalidPackage,
                "unsafe ZIP entry name",
            ));
        }
        if entry.compression == ZipCompression::Deflated {
            return Err(DocxError::new(
                DocxErrorKind::UnsupportedPart,
                "deflate compression is not supported",
            ));
        }

        let data_len = stored_payload(&entry.data).len();
        if data_len > limits.max_entry_size {
            return Err(DocxError::new(
                DocxErrorKind::InvalidPackage,
                "ZIP entry exceeds the supported size limit",
            ));
        }

        local_offsets.push(u32::try_from(cursor).map_err(|_| {
            DocxError::new(
                DocxErrorKind::InvalidPackage,
                "local header offset exceeds u32",
            )
        })?);
        cursor = cursor
            .checked_add(30 + name.len())
            .and_then(|size| size.checked_add(data_len))
            .ok_or_else(|| {
                DocxError::new(DocxErrorKind::InvalidPackage, "ZIP output size overflow")
            })?;
        central_dir_size = central_dir_size
            .checked_add(46 + name.len())
            .ok_or_else(|| {
                DocxError::new(
                    DocxErrorKind::InvalidPackage,
                    "ZIP central directory size overflow",
                )
            })?;
    }

    let central_dir_offset = cursor;
    let total_size = cursor
        .checked_add(central_dir_size)
        .and_then(|size| size.checked_add(22))
        .ok_or_else(|| DocxError::new(DocxErrorKind::InvalidPackage, "ZIP output size overflow"))?;
    if total_size > limits.max_archive_size {
        return Err(DocxError::new(
            DocxErrorKind::InvalidPackage,
            "ZIP archive exceeds the maximum supported size",
        ));
    }
    let central_dir_offset_u32 = u32::try_from(central_dir_offset).map_err(|_| {
        DocxError::new(
            DocxErrorKind::InvalidPackage,
            "central directory offset exceeds u32",
        )
    })?;
    let central_dir_size_u32 = u32::try_from(central_dir_size).map_err(|_| {
        DocxError::new(
            DocxErrorKind::InvalidPackage,
            "central directory size exceeds u32",
        )
    })?;

    // Second pass: write local headers and payloads, computing CRC-32 once per entry.
    let mut output = Vec::with_capacity(total_size);
    let mut crcs = Vec::with_capacity(entry_count);
    for entry in &package.entries {
        let data = stored_payload(&entry.data);
        let size_u32 = u32::try_from(data.len())
            .map_err(|_| DocxError::new(DocxErrorKind::InvalidPackage, "entry size exceeds u32"))?;
        let crc = crc32(data);
        write_local_file_header(
            &mut output,
            &entry.name,
            size_u32,
            size_u32,
            crc,
            ZIP_METHOD_STORED,
        );
        output.extend_from_slice(data);
        crcs.push(crc);
    }

    // Third pass: central directory entries, reusing local offsets and CRCs.
    for (index, entry) in package.entries.iter().enumerate() {
        let data = stored_payload(&entry.data);
        let size_u32 = u32::try_from(data.len())
            .map_err(|_| DocxError::new(DocxErrorKind::InvalidPackage, "entry size exceeds u32"))?;
        write_central_directory_entry(
            &mut output,
            &entry.name,
            size_u32,
            size_u32,
            crcs[index],
            ZIP_METHOD_STORED,
            local_offsets[index],
        );
    }

    write_eocd(
        &mut output,
        central_dir_size_u32,
        central_dir_offset_u32,
        entry_count_u16,
    );

    debug_assert_eq!(output.len(), total_size);
    Ok(output)
}

pub fn parse_zip(bytes: &[u8]) -> Result<DocxPackage, DocxError> {
    if bytes.len() > MAX_ARCHIVE_SIZE {
        return Err(DocxError::new(
            DocxErrorKind::InvalidPackage,
            "ZIP archive exceeds the maximum supported size",
        ));
    }
    if bytes.len() < 22 {
        return Err(DocxError::new(
            DocxErrorKind::InvalidPackage,
            "input too short for ZIP",
        ));
    }

    // The EOCD may be followed only by its at-most-64KiB ZIP comment.
    let eocd_search_start = bytes.len().saturating_sub(MAX_EOCD_SEARCH_SIZE);
    let eocd_search_limit = bytes.len() - 22;
    let eocd_start = (eocd_search_start..=eocd_search_limit)
        .rev()
        .find_map(|pos| {
            let signature = bytes.get(pos..pos + 4)?;
            if signature != ZIP_END_OF_CENTRAL_DIRECTORY_SIGNATURE.to_le_bytes() {
                return None;
            }

            let eocd = bytes.get(pos..)?;
            if eocd.len() < 22 {
                return None;
            }

            let comment_length = u16::from_le_bytes([eocd[20], eocd[21]]);
            let eocd_end = pos.checked_add(22)?.checked_add(comment_length as usize)?;
            if eocd_end != bytes.len() {
                return None;
            }

            Some(pos)
        })
        .ok_or_else(|| DocxError::new(DocxErrorKind::InvalidPackage, "ZIP EOCD not found"))?;

    let eocd = &bytes[eocd_start..];
    if eocd.len() < 22 {
        return Err(DocxError::new(
            DocxErrorKind::InvalidPackage,
            "truncated EOCD",
        ));
    }

    let entry_count = u16::from_le_bytes([eocd[10], eocd[11]]);
    let central_dir_size = u32::from_le_bytes([eocd[12], eocd[13], eocd[14], eocd[15]]);
    let central_dir_offset = u32::from_le_bytes([eocd[16], eocd[17], eocd[18], eocd[19]]);
    let comment_length = u16::from_le_bytes([eocd[20], eocd[21]]);

    // The EOCD comment must be exactly the trailing bytes; arbitrary ZIP trailers are invalid.
    let eocd_end = eocd_start
        .checked_add(22)
        .and_then(|offset| offset.checked_add(comment_length as usize))
        .ok_or_else(|| DocxError::new(DocxErrorKind::InvalidPackage, "EOCD extent overflow"))?;
    if eocd_end != bytes.len() {
        return Err(DocxError::new(
            DocxErrorKind::InvalidPackage,
            "EOCD comment does not end at EOF",
        ));
    }

    let central_dir_start = usize::try_from(central_dir_offset).map_err(|_| {
        DocxError::new(
            DocxErrorKind::InvalidPackage,
            "central directory offset too large",
        )
    })?;
    let central_dir_limit = central_dir_start
        .checked_add(usize::try_from(central_dir_size).map_err(|_| {
            DocxError::new(
                DocxErrorKind::InvalidPackage,
                "central directory size too large",
            )
        })?)
        .ok_or_else(|| {
            DocxError::new(
                DocxErrorKind::InvalidPackage,
                "central directory extent overflow",
            )
        })?;
    if central_dir_limit != eocd_start {
        return Err(DocxError::new(
            DocxErrorKind::InvalidPackage,
            "central directory does not end at EOCD",
        ));
    }

    // Validate central dir size is non-zero if there are entries
    if entry_count > 0 && central_dir_size == 0 {
        return Err(DocxError::new(
            DocxErrorKind::InvalidPackage,
            "central directory size is zero but entry count is non-zero",
        ));
    }

    // ZIP64-ish overflow check: entry count must match actual entries found
    if entry_count == 0 && central_dir_size > 0 {
        return Err(DocxError::new(
            DocxErrorKind::InvalidPackage,
            "central directory has data but entry count is zero",
        ));
    }

    if usize::from(entry_count) > MAX_ENTRY_COUNT {
        return Err(DocxError::new(
            DocxErrorKind::InvalidPackage,
            "ZIP entry count exceeds the supported limit",
        ));
    }

    let mut entries = Vec::with_capacity(usize::from(entry_count));
    let mut entry_names: std::collections::HashSet<String> =
        std::collections::HashSet::with_capacity(usize::from(entry_count));
    let mut local_spans: Vec<std::ops::Range<usize>> = Vec::with_capacity(usize::from(entry_count));
    let mut total_uncompressed_size = 0usize;
    let mut cursor = central_dir_start;

    for _ in 0..entry_count {
        let central_header_end = cursor.checked_add(46).ok_or_else(|| {
            DocxError::new(
                DocxErrorKind::InvalidPackage,
                "central directory header extent overflow",
            )
        })?;
        if central_header_end > central_dir_limit {
            return Err(DocxError::new(
                DocxErrorKind::InvalidPackage,
                "truncated central directory entry",
            ));
        }
        let sig = u32::from_le_bytes([
            bytes[cursor],
            bytes[cursor + 1],
            bytes[cursor + 2],
            bytes[cursor + 3],
        ]);
        if sig != ZIP_CENTRAL_DIRECTORY_SIGNATURE {
            return Err(DocxError::new(
                DocxErrorKind::InvalidPackage,
                "expected central directory signature",
            ));
        }
        let central_flags = u16::from_le_bytes([bytes[cursor + 8], bytes[cursor + 9]]);
        if central_flags & !ZIP_ALLOWED_FLAGS != 0 {
            return Err(DocxError::new(
                DocxErrorKind::InvalidPackage,
                "unsupported central directory general-purpose flags",
            ));
        }
        let method = u16::from_le_bytes([bytes[cursor + 10], bytes[cursor + 11]]);
        let crc_stored = u32::from_le_bytes([
            bytes[cursor + 16],
            bytes[cursor + 17],
            bytes[cursor + 18],
            bytes[cursor + 19],
        ]);
        let central_compressed_size = u32::from_le_bytes([
            bytes[cursor + 20],
            bytes[cursor + 21],
            bytes[cursor + 22],
            bytes[cursor + 23],
        ]);
        let central_uncompressed_size = u32::from_le_bytes([
            bytes[cursor + 24],
            bytes[cursor + 25],
            bytes[cursor + 26],
            bytes[cursor + 27],
        ]);
        let name_length = u16::from_le_bytes([bytes[cursor + 28], bytes[cursor + 29]]);
        let extra_length = u16::from_le_bytes([bytes[cursor + 30], bytes[cursor + 31]]);
        let comment_length_cd = u16::from_le_bytes([bytes[cursor + 32], bytes[cursor + 33]]);
        let local_header_offset = u32::from_le_bytes([
            bytes[cursor + 42],
            bytes[cursor + 43],
            bytes[cursor + 44],
            bytes[cursor + 45],
        ]);

        // Check for ZIP64-ish overflow: sizes/offsets must fit in u32
        // (already parsed as u32, but validate they are not ZIP64 extended)
        if central_compressed_size == u32::MAX || central_uncompressed_size == u32::MAX {
            return Err(DocxError::new(
                DocxErrorKind::InvalidPackage,
                "ZIP64 extended sizes are not supported",
            ));
        }

        if usize::from(name_length) > MAX_ENTRY_NAME_SIZE {
            return Err(DocxError::new(
                DocxErrorKind::InvalidPackage,
                "ZIP entry name exceeds the supported size limit",
            ));
        }
        let name_start = cursor.checked_add(46).ok_or_else(|| {
            DocxError::new(DocxErrorKind::InvalidPackage, "entry name offset overflow")
        })?;
        let name_end = name_start
            .checked_add(usize::from(name_length))
            .ok_or_else(|| {
                DocxError::new(DocxErrorKind::InvalidPackage, "entry name extent overflow")
            })?;
        let name_bytes = bytes
            .get(name_start..name_end)
            .ok_or_else(|| DocxError::new(DocxErrorKind::InvalidPackage, "truncated entry name"))?;
        let name = std::str::from_utf8(name_bytes)
            .map_err(|_| {
                DocxError::new(
                    DocxErrorKind::InvalidPackage,
                    "entry name is not valid UTF-8",
                )
            })?
            .to_owned();
        if name.is_empty() {
            return Err(DocxError::new(
                DocxErrorKind::InvalidPackage,
                "entry name is empty",
            ));
        }
        if !zip_name_is_safe(&name) {
            return Err(DocxError::new(
                DocxErrorKind::InvalidPackage,
                "unsafe ZIP entry name",
            ));
        }
        if !entry_names.insert(name.clone()) {
            return Err(DocxError::new(
                DocxErrorKind::InvalidPackage,
                "duplicate ZIP entry name",
            ));
        }

        let central_entry_size = 46usize
            .checked_add(usize::from(name_length))
            .and_then(|size| size.checked_add(usize::from(extra_length)))
            .and_then(|size| size.checked_add(usize::from(comment_length_cd)))
            .ok_or_else(|| {
                DocxError::new(
                    DocxErrorKind::InvalidPackage,
                    "central directory entry extent overflow",
                )
            })?;
        cursor = cursor.checked_add(central_entry_size).ok_or_else(|| {
            DocxError::new(
                DocxErrorKind::InvalidPackage,
                "central directory cursor overflow",
            )
        })?;
        if cursor > central_dir_limit {
            return Err(DocxError::new(
                DocxErrorKind::InvalidPackage,
                "central directory entry exceeds central directory bounds",
            ));
        }

        if method == ZIP_METHOD_DEFLATE {
            return Err(DocxError::new(
                DocxErrorKind::UnsupportedPart,
                "deflate compression is not supported",
            ));
        }
        if method != ZIP_METHOD_STORED {
            return Err(DocxError::new(
                DocxErrorKind::UnsupportedPart,
                "unsupported compression method",
            ));
        }

        let compressed_size = usize::try_from(central_compressed_size).map_err(|_| {
            DocxError::new(DocxErrorKind::InvalidPackage, "compressed size too large")
        })?;
        let uncompressed_size = usize::try_from(central_uncompressed_size).map_err(|_| {
            DocxError::new(DocxErrorKind::InvalidPackage, "uncompressed size too large")
        })?;
        if compressed_size > MAX_ENTRY_SIZE || uncompressed_size > MAX_ENTRY_SIZE {
            return Err(DocxError::new(
                DocxErrorKind::InvalidPackage,
                "ZIP entry exceeds the supported size limit",
            ));
        }
        total_uncompressed_size = total_uncompressed_size
            .checked_add(uncompressed_size)
            .ok_or_else(|| {
                DocxError::new(DocxErrorKind::InvalidPackage, "aggregate size overflow")
            })?;
        if total_uncompressed_size > MAX_TOTAL_UNCOMPRESSED_SIZE {
            return Err(DocxError::new(
                DocxErrorKind::InvalidPackage,
                "ZIP entries exceed the aggregate size limit",
            ));
        }

        // Parse local file header.
        let data_start = usize::try_from(local_header_offset).map_err(|_| {
            DocxError::new(
                DocxErrorKind::InvalidPackage,
                "local header offset too large",
            )
        })?;
        let local_header_end = data_start.checked_add(30).ok_or_else(|| {
            DocxError::new(
                DocxErrorKind::InvalidPackage,
                "local header extent overflow",
            )
        })?;
        if local_header_end > bytes.len() {
            return Err(DocxError::new(
                DocxErrorKind::InvalidPackage,
                "truncated local file header",
            ));
        }
        let local_sig = u32::from_le_bytes([
            bytes[data_start],
            bytes[data_start + 1],
            bytes[data_start + 2],
            bytes[data_start + 3],
        ]);
        if local_sig != ZIP_LOCAL_FILE_SIGNATURE {
            return Err(DocxError::new(
                DocxErrorKind::InvalidPackage,
                "expected local file signature",
            ));
        }
        let local_flags = u16::from_le_bytes([bytes[data_start + 6], bytes[data_start + 7]]);
        let local_method = u16::from_le_bytes([bytes[data_start + 8], bytes[data_start + 9]]);

        if local_method != method {
            return Err(DocxError::new(
                DocxErrorKind::InvalidPackage,
                "local header method does not match central directory",
            ));
        }
        if local_flags != central_flags {
            return Err(DocxError::new(
                DocxErrorKind::InvalidPackage,
                "local header flags do not match central directory",
            ));
        }
        if local_flags & !ZIP_ALLOWED_FLAGS != 0 {
            return Err(DocxError::new(
                DocxErrorKind::InvalidPackage,
                "unsupported local file header flags",
            ));
        }

        let local_name_length =
            u16::from_le_bytes([bytes[data_start + 26], bytes[data_start + 27]]);
        let local_extra_length =
            u16::from_le_bytes([bytes[data_start + 28], bytes[data_start + 29]]);

        if local_name_length != name_length {
            return Err(DocxError::new(
                DocxErrorKind::InvalidPackage,
                "local header name length does not match central directory",
            ));
        }
        let local_name_start = local_header_end;
        let local_name_end = local_name_start
            .checked_add(usize::from(local_name_length))
            .ok_or_else(|| {
                DocxError::new(DocxErrorKind::InvalidPackage, "local name extent overflow")
            })?;
        let local_name = bytes.get(local_name_start..local_name_end).ok_or_else(|| {
            DocxError::new(DocxErrorKind::InvalidPackage, "truncated local entry name")
        })?;
        if local_name != name_bytes {
            return Err(DocxError::new(
                DocxErrorKind::InvalidPackage,
                "local entry name does not match central directory",
            ));
        }
        let local_crc = u32::from_le_bytes([
            bytes[data_start + 14],
            bytes[data_start + 15],
            bytes[data_start + 16],
            bytes[data_start + 17],
        ]);
        if local_crc != crc_stored {
            return Err(DocxError::new(
                DocxErrorKind::InvalidPackage,
                "local header CRC does not match central directory",
            ));
        }

        let data_offset = local_name_end
            .checked_add(usize::from(local_extra_length))
            .ok_or_else(|| {
                DocxError::new(DocxErrorKind::InvalidPackage, "local data offset overflow")
            })?;
        let local_data_end = data_offset.checked_add(compressed_size).ok_or_else(|| {
            DocxError::new(DocxErrorKind::InvalidPackage, "local data extent overflow")
        })?;
        if local_data_end > central_dir_start {
            return Err(DocxError::new(
                DocxErrorKind::InvalidPackage,
                "local file data overlaps with central directory",
            ));
        }
        let raw_data = bytes
            .get(data_offset..local_data_end)
            .ok_or_else(|| DocxError::new(DocxErrorKind::InvalidPackage, "truncated entry data"))?;
        let local_span = data_start..local_data_end;
        if local_spans.iter().any(|existing: &std::ops::Range<usize>| {
            local_span.start < existing.end && existing.start < local_span.end
        }) {
            return Err(DocxError::new(
                DocxErrorKind::InvalidPackage,
                "local file header or data overlaps another entry",
            ));
        }
        local_spans.push(local_span);

        // Stored entries must have identical compressed and uncompressed sizes.
        if central_compressed_size != central_uncompressed_size {
            return Err(DocxError::new(
                DocxErrorKind::InvalidPackage,
                "stored entry has mismatched compressed and uncompressed sizes",
            ));
        }

        // Validate local header sizes match central directory sizes
        let local_compressed_size = u32::from_le_bytes([
            bytes[data_start + 18],
            bytes[data_start + 19],
            bytes[data_start + 20],
            bytes[data_start + 21],
        ]);
        let local_uncompressed_size = u32::from_le_bytes([
            bytes[data_start + 22],
            bytes[data_start + 23],
            bytes[data_start + 24],
            bytes[data_start + 25],
        ]);
        if local_compressed_size != central_compressed_size {
            return Err(DocxError::new(
                DocxErrorKind::InvalidPackage,
                "local header compressed size does not match central directory",
            ));
        }
        if local_uncompressed_size != central_uncompressed_size {
            return Err(DocxError::new(
                DocxErrorKind::InvalidPackage,
                "local header uncompressed size does not match central directory",
            ));
        }

        // Validate CRC
        let computed_crc = crc32(raw_data);
        if computed_crc != crc_stored {
            return Err(DocxError::new(
                DocxErrorKind::InvalidPackage,
                "CRC mismatch for entry",
            ));
        }

        let data = if name.ends_with(".xml") || name.ends_with(".rels") {
            let xml = std::str::from_utf8(raw_data).map_err(|_| {
                DocxError::new(DocxErrorKind::XmlDecode, "XML entry is not valid UTF-8")
            })?;
            FlatOpcData::Xml(xml.to_owned())
        } else {
            FlatOpcData::Binary(raw_data.to_vec())
        };

        entries.push(ZipEntry {
            name,
            content_type: String::new(), // resolved after all parts are parsed
            data,
            compression: ZipCompression::Stored,
        });
    }

    // Validate that the central directory cursor reached the expected end.
    if cursor != central_dir_limit {
        return Err(DocxError::new(
            DocxErrorKind::InvalidPackage,
            "central directory size does not match entry count",
        ));
    }

    let content_types_xml = entries
        .iter()
        .find(|entry| entry.name == "[Content_Types].xml")
        .and_then(|entry| match &entry.data {
            FlatOpcData::Xml(xml) => Some(xml.clone()),
            FlatOpcData::Binary(_) => None,
        });
    for entry in &mut entries {
        entry.content_type = content_types_xml
            .as_deref()
            .and_then(|xml| content_type_for_part(xml, &entry.name))
            .unwrap_or_default();
    }

    Ok(ZipPackage { entries })
}

fn content_type_for_part(content_types_xml: &str, part_name: &str) -> Option<String> {
    let target = part_name;
    let mut offset = 0;
    while let Some(start) = content_types_xml[offset..].find("<Override") {
        let tag_start = offset + start;
        let after_start = tag_start + "<Override".len();
        let first = content_types_xml.as_bytes().get(after_start).copied();
        if !matches!(
            first,
            Some(b' ') | Some(b'\t') | Some(b'\n') | Some(b'\r') | Some(b'/') | Some(b'>')
        ) {
            offset = after_start;
            continue;
        }
        let tag_end = content_types_xml[after_start..].find('>')? + after_start;
        let raw_tag = &content_types_xml[after_start..tag_end];
        let tag = raw_tag.strip_suffix('/').unwrap_or(raw_tag);
        if let Some((found_part, found_type)) = parse_override_attributes(tag)
            && found_part == format!("/{target}")
        {
            return Some(found_type.to_owned());
        }
        offset = tag_end + 1;
    }
    None
}

fn parse_override_attributes(tag: &str) -> Option<(&str, &str)> {
    let bytes = tag.as_bytes();
    let mut i = 0;
    let mut part_name: Option<&str> = None;
    let mut content_type: Option<&str> = None;
    while i < bytes.len() {
        while i < bytes.len() && bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        if i >= bytes.len() {
            break;
        }
        let name_start = i;
        while i < bytes.len() && !bytes[i].is_ascii_whitespace() && bytes[i] != b'=' {
            i += 1;
        }
        let name = &tag[name_start..i];
        while i < bytes.len() && bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        if i >= bytes.len() || bytes[i] != b'=' {
            return None;
        }
        i += 1;
        while i < bytes.len() && bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        if i >= bytes.len() {
            return None;
        }
        let quote = bytes[i];
        if quote != b'"' && quote != b'\'' {
            return None;
        }
        i += 1;
        let value_start = i;
        while i < bytes.len() && bytes[i] != quote {
            i += 1;
        }
        if i >= bytes.len() {
            return None;
        }
        let value = &tag[value_start..i];
        i += 1;
        match name {
            "PartName" => part_name = Some(value),
            "ContentType" => content_type = Some(value),
            _ => {}
        }
    }
    let part_name = part_name?;
    let content_type = content_type?;
    Some((part_name, content_type))
}

/// C1 component safety predicate: rejects null bytes, backslashes, leading slash, and `..` segments.
/// Empty-name validity remains the caller's responsibility.
pub(crate) fn zip_name_is_safe(name: &str) -> bool {
    if name.contains('\0') || name.contains('\\') || name.starts_with('/') {
        return false;
    }
    for segment in name.split('/') {
        if segment == ".." {
            return false;
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zip_roundtrip_stored() {
        let package = ZipPackage {
            entries: vec![
                ZipEntry {
                    name: "[Content_Types].xml".to_string(),
                    content_type: "application/xml".to_string(),
                    data: FlatOpcData::Xml("<root/>".to_string()),
                    compression: ZipCompression::Stored,
                },
                ZipEntry {
                    name: "word/document.xml".to_string(),
                    content_type: "application/xml".to_string(),
                    data: FlatOpcData::Xml("<w:document/>".to_string()),
                    compression: ZipCompression::Stored,
                },
            ],
        };
        let bytes = serialize_zip(&package).unwrap();
        assert!(bytes.starts_with(&ZIP_LOCAL_FILE_SIGNATURE.to_le_bytes()));
        let parsed = parse_zip(&bytes).unwrap();
        assert_eq!(parsed.entries.len(), 2);
        assert_eq!(parsed.entries[0].name, "[Content_Types].xml");
    }

    #[test]
    fn zip_rejects_deflate() {
        // Build a valid stored ZIP, then mutate the method to deflate
        let package = ZipPackage {
            entries: vec![ZipEntry {
                name: "test.xml".to_string(),
                content_type: "application/xml".to_string(),
                data: FlatOpcData::Xml("<root/>".to_string()),
                compression: ZipCompression::Stored,
            }],
        };
        let mut bytes = serialize_zip(&package).unwrap();
        // Find the central directory entry method field (offset 10 within central dir entry)
        let cd_offset = bytes
            .windows(4)
            .position(|w| {
                u32::from_le_bytes([w[0], w[1], w[2], w[3]]) == ZIP_CENTRAL_DIRECTORY_SIGNATURE
            })
            .expect("central directory signature not found");
        // Method is at offset 10 within central directory entry
        bytes[cd_offset + 10] = ZIP_METHOD_DEFLATE as u8;
        // Also mutate the local header method (offset 8 within local file header)
        let local_offset = bytes
            .windows(4)
            .position(|w| u32::from_le_bytes([w[0], w[1], w[2], w[3]]) == ZIP_LOCAL_FILE_SIGNATURE)
            .expect("local file signature not found");
        bytes[local_offset + 8] = ZIP_METHOD_DEFLATE as u8;

        let result = parse_zip(&bytes);
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().kind, DocxErrorKind::UnsupportedPart);
    }

    #[test]
    fn parse_rejects_duplicate_entry_names() {
        let package = ZipPackage {
            entries: vec![
                ZipEntry {
                    name: "test.xml".to_string(),
                    content_type: "application/xml".to_string(),
                    data: FlatOpcData::Xml("<one/>".to_string()),
                    compression: ZipCompression::Stored,
                },
                ZipEntry {
                    name: "test.xml".to_string(),
                    content_type: "application/xml".to_string(),
                    data: FlatOpcData::Xml("<two/>".to_string()),
                    compression: ZipCompression::Stored,
                },
            ],
        };

        let error = parse_zip(&serialize_zip(&package).unwrap()).unwrap_err();
        assert_eq!(error.kind, DocxErrorKind::InvalidPackage);
    }

    #[test]
    fn parse_rejects_mismatched_local_entry_name() {
        let package = ZipPackage {
            entries: vec![ZipEntry {
                name: "test.xml".to_string(),
                content_type: "application/xml".to_string(),
                data: FlatOpcData::Xml("<root/>".to_string()),
                compression: ZipCompression::Stored,
            }],
        };
        let mut bytes = serialize_zip(&package).unwrap();
        let local_offset = bytes
            .windows(4)
            .position(|window| {
                u32::from_le_bytes([window[0], window[1], window[2], window[3]])
                    == ZIP_LOCAL_FILE_SIGNATURE
            })
            .unwrap();
        bytes[local_offset + 30] = b'b';

        let error = parse_zip(&bytes).unwrap_err();
        assert_eq!(error.kind, DocxErrorKind::InvalidPackage);
    }

    #[test]
    fn parse_rejects_invalid_utf8_entry_name() {
        let package = ZipPackage {
            entries: vec![ZipEntry {
                name: "test.xml".to_string(),
                content_type: "application/xml".to_string(),
                data: FlatOpcData::Xml("<root/>".to_string()),
                compression: ZipCompression::Stored,
            }],
        };
        let mut bytes = serialize_zip(&package).unwrap();
        let central_offset = bytes
            .windows(4)
            .position(|window| {
                u32::from_le_bytes([window[0], window[1], window[2], window[3]])
                    == ZIP_CENTRAL_DIRECTORY_SIGNATURE
            })
            .unwrap();
        bytes[central_offset + 46] = 0xff;

        let error = parse_zip(&bytes).unwrap_err();
        assert_eq!(error.kind, DocxErrorKind::InvalidPackage);
    }

    #[test]
    fn parse_rejects_reused_local_entry_span() {
        let package = ZipPackage {
            entries: vec![
                ZipEntry {
                    name: "one.xml".to_string(),
                    content_type: "application/xml".to_string(),
                    data: FlatOpcData::Xml("<one/>".to_string()),
                    compression: ZipCompression::Stored,
                },
                ZipEntry {
                    name: "two.xml".to_string(),
                    content_type: "application/xml".to_string(),
                    data: FlatOpcData::Xml("<two/>".to_string()),
                    compression: ZipCompression::Stored,
                },
            ],
        };
        let mut bytes = serialize_zip(&package).unwrap();
        let central_offsets = bytes
            .windows(4)
            .enumerate()
            .filter_map(|(index, window)| {
                (u32::from_le_bytes([window[0], window[1], window[2], window[3]])
                    == ZIP_CENTRAL_DIRECTORY_SIGNATURE)
                    .then_some(index)
            })
            .collect::<Vec<_>>();
        let first_local_offset = bytes[central_offsets[0] + 42..central_offsets[0] + 46].to_vec();
        bytes[central_offsets[1] + 42..central_offsets[1] + 46]
            .copy_from_slice(&first_local_offset);

        let error = parse_zip(&bytes).unwrap_err();
        assert_eq!(error.kind, DocxErrorKind::InvalidPackage);
    }

    #[test]
    fn parse_rejects_disallowed_central_directory_flags() {
        let package = ZipPackage {
            entries: vec![ZipEntry {
                name: "test.xml".to_string(),
                content_type: "application/xml".to_string(),
                data: FlatOpcData::Xml("<root/>".to_string()),
                compression: ZipCompression::Stored,
            }],
        };
        let mut bytes = serialize_zip(&package).unwrap();
        let central_offset = bytes
            .windows(4)
            .position(|window| {
                u32::from_le_bytes([window[0], window[1], window[2], window[3]])
                    == ZIP_CENTRAL_DIRECTORY_SIGNATURE
            })
            .unwrap();
        bytes[central_offset + 8] = 0x08; // data descriptor bit

        let error = parse_zip(&bytes).unwrap_err();
        assert_eq!(error.kind, DocxErrorKind::InvalidPackage);
    }

    #[test]
    fn serialize_rejects_deflate() {
        let package = ZipPackage {
            entries: vec![ZipEntry {
                name: "test.xml".to_string(),
                content_type: "application/xml".to_string(),
                data: FlatOpcData::Xml("<root/>".to_string()),
                compression: ZipCompression::Deflated,
            }],
        };
        let result = serialize_zip(&package);
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().kind, DocxErrorKind::UnsupportedPart);
    }

    #[test]
    fn serialize_rejects_too_many_entries() {
        let mut entries = Vec::new();
        for i in 0..u16::MAX as usize + 1 {
            entries.push(ZipEntry {
                name: format!("file_{}.xml", i),
                content_type: "application/xml".to_string(),
                data: FlatOpcData::Xml("<root/>".to_string()),
                compression: ZipCompression::Stored,
            });
        }
        let package = ZipPackage { entries };
        let result = serialize_zip(&package);
        assert!(result.is_err());
    }

    #[test]
    fn serialize_limits_defaults_match_parser_constants() {
        let limits = SerializeLimits::default();
        assert_eq!(limits.max_archive_size, MAX_ARCHIVE_SIZE);
        assert_eq!(limits.max_entry_size, MAX_ENTRY_SIZE);
        assert_eq!(limits.max_entry_count, MAX_ENTRY_COUNT);
    }

    #[test]
    fn serialize_rejects_unsafe_entry_names() {
        let package = ZipPackage {
            entries: vec![ZipEntry {
                name: "[Content_Types].xml".to_string(),
                content_type: "application/xml".to_string(),
                data: FlatOpcData::Xml("<root/>".to_string()),
                compression: ZipCompression::Stored,
            }],
        };
        let cases = [
            "",
            "word/document\0.xml",
            "/[Content_Types].xml",
            "word\\document.xml",
            "..",
        ];
        for bad_name in cases {
            let mut local = package.clone();
            local.entries[0].name = bad_name.to_string();
            let error = serialize_zip(&local).unwrap_err();
            assert_eq!(
                error.kind,
                DocxErrorKind::InvalidPackage,
                "expected InvalidPackage for {bad_name:?}"
            );
        }
    }

    #[test]
    fn serialize_rejects_entry_name_too_long() {
        let package = ZipPackage {
            entries: vec![ZipEntry {
                name: "a".repeat(MAX_ENTRY_NAME_SIZE + 1),
                content_type: "application/xml".to_string(),
                data: FlatOpcData::Xml("<root/>".to_string()),
                compression: ZipCompression::Stored,
            }],
        };
        let error = serialize_zip(&package).unwrap_err();
        assert_eq!(error.kind, DocxErrorKind::InvalidPackage);
    }

    #[test]
    fn serialize_name_validation_precedes_deflate_check() {
        let package = ZipPackage {
            entries: vec![ZipEntry {
                name: "word\\document.xml".to_string(),
                content_type: "application/xml".to_string(),
                data: FlatOpcData::Xml("<root/>".to_string()),
                compression: ZipCompression::Deflated,
            }],
        };
        let error = serialize_zip(&package).unwrap_err();
        assert_eq!(error.kind, DocxErrorKind::InvalidPackage);
    }

    #[test]
    fn serialize_respects_custom_entry_size_limit() {
        let package = ZipPackage {
            entries: vec![ZipEntry {
                name: "test.xml".to_string(),
                content_type: "application/xml".to_string(),
                data: FlatOpcData::Binary(vec![0xAB; 8]),
                compression: ZipCompression::Stored,
            }],
        };
        let at_limit = SerializeLimits {
            max_entry_size: 8,
            ..Default::default()
        };
        assert!(serialize_zip_with_limits(&package, &at_limit).is_ok());

        let below_limit = SerializeLimits {
            max_entry_size: 7,
            ..Default::default()
        };
        let error = serialize_zip_with_limits(&package, &below_limit).unwrap_err();
        assert_eq!(error.kind, DocxErrorKind::InvalidPackage);
    }

    #[test]
    fn serialize_respects_custom_entry_count_limit() {
        let package = ZipPackage {
            entries: vec![
                ZipEntry {
                    name: "a.xml".to_string(),
                    content_type: "application/xml".to_string(),
                    data: FlatOpcData::Xml("<a/>".to_string()),
                    compression: ZipCompression::Stored,
                },
                ZipEntry {
                    name: "b.xml".to_string(),
                    content_type: "application/xml".to_string(),
                    data: FlatOpcData::Xml("<b/>".to_string()),
                    compression: ZipCompression::Stored,
                },
            ],
        };
        let at_limit = SerializeLimits {
            max_entry_count: 2,
            ..Default::default()
        };
        assert!(serialize_zip_with_limits(&package, &at_limit).is_ok());

        let below_limit = SerializeLimits {
            max_entry_count: 1,
            ..Default::default()
        };
        let error = serialize_zip_with_limits(&package, &below_limit).unwrap_err();
        assert_eq!(error.kind, DocxErrorKind::InvalidPackage);
    }

    #[test]
    fn serialize_respects_custom_archive_size_limit() {
        // Two stored entries: local 2*(30 header + 5 name + 4 data),
        // central 2*(46 + 5 name), EOCD 22 -> 202 bytes total.
        let package = ZipPackage {
            entries: vec![
                ZipEntry {
                    name: "a.xml".to_string(),
                    content_type: "application/xml".to_string(),
                    data: FlatOpcData::Xml("<a/>".to_string()),
                    compression: ZipCompression::Stored,
                },
                ZipEntry {
                    name: "b.xml".to_string(),
                    content_type: "application/xml".to_string(),
                    data: FlatOpcData::Xml("<b/>".to_string()),
                    compression: ZipCompression::Stored,
                },
            ],
        };
        let at_limit = SerializeLimits {
            max_archive_size: 202,
            ..Default::default()
        };
        assert!(serialize_zip_with_limits(&package, &at_limit).is_ok());

        let below_limit = SerializeLimits {
            max_archive_size: 201,
            ..Default::default()
        };
        let error = serialize_zip_with_limits(&package, &below_limit).unwrap_err();
        assert_eq!(error.kind, DocxErrorKind::InvalidPackage);
    }

    #[test]
    fn parse_rejects_bad_central_offset() {
        // Build a minimal valid ZIP, then corrupt the central dir offset in EOCD
        let package = ZipPackage {
            entries: vec![ZipEntry {
                name: "test.xml".to_string(),
                content_type: "application/xml".to_string(),
                data: FlatOpcData::Xml("<root/>".to_string()),
                compression: ZipCompression::Stored,
            }],
        };
        let mut bytes = serialize_zip(&package).unwrap();
        // Find EOCD signature
        let eocd_pos = bytes
            .windows(4)
            .position(|w| {
                u32::from_le_bytes([w[0], w[1], w[2], w[3]])
                    == ZIP_END_OF_CENTRAL_DIRECTORY_SIGNATURE
            })
            .expect("EOCD not found");
        // Corrupt central dir offset (bytes 16-19 of EOCD)
        bytes[eocd_pos + 16] = 0xFF;
        bytes[eocd_pos + 17] = 0xFF;
        bytes[eocd_pos + 18] = 0xFF;
        bytes[eocd_pos + 19] = 0xFF;

        let result = parse_zip(&bytes);
        assert!(result.is_err());
    }

    #[test]
    fn parse_rejects_bad_crc() {
        // Build a valid ZIP, then corrupt the data
        let package = ZipPackage {
            entries: vec![ZipEntry {
                name: "test.xml".to_string(),
                content_type: "application/xml".to_string(),
                data: FlatOpcData::Xml("<root/>".to_string()),
                compression: ZipCompression::Stored,
            }],
        };
        let mut bytes = serialize_zip(&package).unwrap();
        // Find the local file header signature
        let local_pos = bytes
            .windows(4)
            .position(|w| u32::from_le_bytes([w[0], w[1], w[2], w[3]]) == ZIP_LOCAL_FILE_SIGNATURE)
            .expect("local file signature not found");
        // Data starts at local_pos + 30 + name_length
        let name_len = u16::from_le_bytes([bytes[local_pos + 26], bytes[local_pos + 27]]) as usize;
        let data_start = local_pos + 30 + name_len;
        // Corrupt first byte of data
        bytes[data_start] = bytes[data_start].wrapping_add(1);

        let result = parse_zip(&bytes);
        assert!(result.is_err());
    }

    #[test]
    fn parse_rejects_truncated_eocd() {
        let bytes = vec![0u8; 20]; // Too short for EOCD
        let result = parse_zip(&bytes);
        assert!(result.is_err());
    }

    #[test]
    fn parse_rejects_eocd_comment_extent() {
        // Build a valid ZIP, then set a comment length that extends past EOF
        let package = ZipPackage {
            entries: vec![ZipEntry {
                name: "test.xml".to_string(),
                content_type: "application/xml".to_string(),
                data: FlatOpcData::Xml("<root/>".to_string()),
                compression: ZipCompression::Stored,
            }],
        };
        let mut bytes = serialize_zip(&package).unwrap();
        // Find EOCD signature
        let eocd_pos = bytes
            .windows(4)
            .position(|w| {
                u32::from_le_bytes([w[0], w[1], w[2], w[3]])
                    == ZIP_END_OF_CENTRAL_DIRECTORY_SIGNATURE
            })
            .expect("EOCD not found");
        // Set comment length to something that extends past EOF
        bytes[eocd_pos + 20] = 0xFF;
        bytes[eocd_pos + 21] = 0xFF;

        let result = parse_zip(&bytes);
        assert!(result.is_err());
    }

    #[test]
    fn parse_rejects_central_dir_overlap() {
        // Build a valid ZIP, then set central dir offset so it overlaps with EOCD
        let package = ZipPackage {
            entries: vec![ZipEntry {
                name: "test.xml".to_string(),
                content_type: "application/xml".to_string(),
                data: FlatOpcData::Xml("<root/>".to_string()),
                compression: ZipCompression::Stored,
            }],
        };
        let mut bytes = serialize_zip(&package).unwrap();
        // Find EOCD signature
        let eocd_pos = bytes
            .windows(4)
            .position(|w| {
                u32::from_le_bytes([w[0], w[1], w[2], w[3]])
                    == ZIP_END_OF_CENTRAL_DIRECTORY_SIGNATURE
            })
            .expect("EOCD not found");
        // Set central dir offset to be very close to EOCD (overlapping)
        // EOCD is at eocd_pos, central dir size is non-zero, so offset = eocd_pos - 2 would overlap
        bytes[eocd_pos + 16] = (eocd_pos as u32 - 2) as u8;
        bytes[eocd_pos + 17] = ((eocd_pos as u32 - 2) >> 8) as u8;
        bytes[eocd_pos + 18] = ((eocd_pos as u32 - 2) >> 16) as u8;
        bytes[eocd_pos + 19] = ((eocd_pos as u32 - 2) >> 24) as u8;

        let result = parse_zip(&bytes);
        assert!(result.is_err());
    }

    #[test]
    fn content_type_for_part_resolves_override() {
        let xml = r#"<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/></Types>"#;
        assert_eq!(
            content_type_for_part(xml, "word/document.xml"),
            Some(
                "application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"
                    .to_string()
            )
        );
    }

    #[test]
    fn content_type_for_part_returns_none_when_missing() {
        let xml = r#"<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/></Types>"#;
        assert_eq!(content_type_for_part(xml, "word/styles.xml"), None);
    }

    #[test]
    fn content_type_for_part_does_not_confuse_sibling_names() {
        let xml = r#"<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/><Override PartName="/_rels/.rels" ContentType="application/vnd.openxmlformats-officedocument.relationships+xml"/></Types>"#;
        assert_eq!(
            content_type_for_part(xml, "_rels/.rels"),
            Some("application/vnd.openxmlformats-officedocument.relationships+xml".to_string())
        );
        assert_eq!(
            content_type_for_part(xml, "word/document.xml"),
            Some(
                "application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"
                    .to_string()
            )
        );
    }

    #[test]
    fn content_type_for_part_supports_attribute_order() {
        let xml = r#"<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Override ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml" PartName="/word/document.xml"/></Types>"#;
        assert_eq!(
            content_type_for_part(xml, "word/document.xml"),
            Some(
                "application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"
                    .to_string()
            )
        );
    }

    #[test]
    fn content_type_for_part_supports_whitespace_and_single_quotes() {
        let xml = r#"<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Override PartName = '/word/document.xml' ContentType='application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml' /></Types>"#;
        assert_eq!(
            content_type_for_part(xml, "word/document.xml"),
            Some(
                "application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"
                    .to_string()
            )
        );
    }

    #[test]
    fn content_type_for_part_ignores_override_other() {
        let xml = r#"<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><OverrideOther PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/></Types>"#;
        assert_eq!(
            content_type_for_part(xml, "word/document.xml"),
            Some(
                "application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"
                    .to_string()
            )
        );
    }

    #[test]
    fn content_type_for_part_skips_malformed_override_and_continues() {
        let xml = r#"<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Override PartName="/word/document.xml"/><Override PartName="/word/styles.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.styles+xml"/></Types>"#;
        assert_eq!(
            content_type_for_part(xml, "word/styles.xml"),
            Some(
                "application/vnd.openxmlformats-officedocument.wordprocessingml.styles+xml"
                    .to_string()
            )
        );
    }

    #[test]
    fn content_type_for_part_matches_exact_part_name_raw() {
        let xml = r#"<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/><Override PartName="/word/document.xml&amp;extra" ContentType="text/plain"/></Types>"#;
        assert_eq!(content_type_for_part(xml, "word/document.xml&extra"), None);
    }

    #[test]
    fn zip_name_is_safe_permits_ordinary_opc_names() {
        assert!(zip_name_is_safe("[Content_Types].xml"));
        assert!(zip_name_is_safe("word/document.xml"));
        assert!(zip_name_is_safe("docProps/core.xml"));
        assert!(zip_name_is_safe("_rels/.rels"));
        assert!(zip_name_is_safe("a//b"));
        assert!(zip_name_is_safe("relative/path/.segment"));
        assert!(zip_name_is_safe(""));
    }

    #[test]
    fn zip_name_is_safe_rejects_forbidden_names() {
        assert!(!zip_name_is_safe("foo\0bar"));
        assert!(!zip_name_is_safe("foo\\bar"));
        assert!(!zip_name_is_safe("/absolute"));
        assert!(!zip_name_is_safe("foo/../bar"));
        assert!(!zip_name_is_safe(".."));
        assert!(!zip_name_is_safe("foo/.."));
        assert!(!zip_name_is_safe("foo/bar/.."));
        assert!(!zip_name_is_safe("foo\\..\\bar"));
    }

    #[test]
    fn zip_name_is_safe_ignores_non_targeted_restrictions() {
        assert!(zip_name_is_safe("CON"));
        assert!(zip_name_is_safe("foo:bar"));
    }

    #[test]
    fn parse_rejects_unsafe_zip_names() {
        let package = ZipPackage {
            entries: vec![ZipEntry {
                name: "[Content_Types].xml".to_string(),
                content_type: "application/xml".to_string(),
                data: FlatOpcData::Xml("<root/>".to_string()),
                compression: ZipCompression::Stored,
            }],
        };
        let cases = [
            "/[Content_Types].xml",
            "[Content_Types].xml/../rels/.rels",
            "word\\document.xml",
            "word/document\0.xml",
        ];
        for bad_name in cases {
            // The serializer rejects unsafe names outright, so serialize a
            // same-length safe placeholder, then patch the raw central-directory
            // name bytes so the parser sees the unsafe name.
            let mut local = package.clone();
            local.entries[0].name = "a".repeat(bad_name.len());
            let mut bytes = serialize_zip(&local).unwrap();
            let central_offset = bytes
                .windows(4)
                .position(|window| {
                    u32::from_le_bytes([window[0], window[1], window[2], window[3]])
                        == ZIP_CENTRAL_DIRECTORY_SIGNATURE
                })
                .unwrap();
            let name_len =
                u16::from_le_bytes([bytes[central_offset + 28], bytes[central_offset + 29]])
                    as usize;
            let name_start = central_offset + 46;
            bytes[name_start..name_start + name_len].copy_from_slice(bad_name.as_bytes());
            let result = parse_zip(&bytes);
            assert!(
                result.is_err(),
                "expected unsafe name rejection for {bad_name:?}"
            );
            assert_eq!(result.unwrap_err().kind, DocxErrorKind::InvalidPackage);
        }
    }

    #[test]
    fn parse_rejects_local_header_crc_mismatch() {
        let package = ZipPackage {
            entries: vec![ZipEntry {
                name: "test.xml".to_string(),
                content_type: "application/xml".to_string(),
                data: FlatOpcData::Xml("<root/>".to_string()),
                compression: ZipCompression::Stored,
            }],
        };
        let mut bytes = serialize_zip(&package).unwrap();
        let local_pos = bytes
            .windows(4)
            .position(|w| u32::from_le_bytes([w[0], w[1], w[2], w[3]]) == ZIP_LOCAL_FILE_SIGNATURE)
            .expect("local file signature not found");
        bytes[local_pos + 14] = bytes[local_pos + 14].wrapping_add(1);
        let result = parse_zip(&bytes);
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().kind, DocxErrorKind::InvalidPackage);
    }

    #[test]
    fn parse_rejects_stored_mismatched_sizes() {
        // Build a valid ZIP, then set compressed_size != uncompressed_size in local header
        let package = ZipPackage {
            entries: vec![ZipEntry {
                name: "test.xml".to_string(),
                content_type: "application/xml".to_string(),
                data: FlatOpcData::Xml("<root/>".to_string()),
                compression: ZipCompression::Stored,
            }],
        };
        let mut bytes = serialize_zip(&package).unwrap();
        // Find local file header
        let local_pos = bytes
            .windows(4)
            .position(|w| u32::from_le_bytes([w[0], w[1], w[2], w[3]]) == ZIP_LOCAL_FILE_SIGNATURE)
            .expect("local file signature not found");
        // Compressed size is at offset 18-21 in local header
        // Set it to a different value than uncompressed size (offset 22-25)
        bytes[local_pos + 18] = 0xFF;
        bytes[local_pos + 19] = 0xFF;
        bytes[local_pos + 20] = 0xFF;
        bytes[local_pos + 21] = 0xFF;

        let result = parse_zip(&bytes);
        assert!(result.is_err());
    }

    fn patch_eocd_comment(bytes: &mut Vec<u8>, comment: &[u8]) {
        let eocd_pos = bytes
            .windows(4)
            .position(|w| {
                u32::from_le_bytes([w[0], w[1], w[2], w[3]])
                    == ZIP_END_OF_CENTRAL_DIRECTORY_SIGNATURE
            })
            .expect("EOCD not found");
        let comment_len = comment.len().min(u16::MAX as usize);
        bytes.truncate(eocd_pos + 22);
        bytes.extend_from_slice(&comment[..comment_len]);
        bytes[eocd_pos + 20] = comment_len as u8;
        bytes[eocd_pos + 21] = (comment_len >> 8) as u8;
    }

    #[test]
    fn parse_zip_accepts_ordinary_eocd_comment() {
        let package = ZipPackage {
            entries: vec![ZipEntry {
                name: "test.xml".to_string(),
                content_type: "application/xml".to_string(),
                data: FlatOpcData::Xml("<root/>".to_string()),
                compression: ZipCompression::Stored,
            }],
        };
        let mut bytes = serialize_zip(&package).unwrap();
        patch_eocd_comment(&mut bytes, b"ordinary comment");
        let result = parse_zip(&bytes);
        assert!(result.is_ok());
        assert_eq!(result.unwrap().entries.len(), 1);
    }

    #[test]
    fn parse_zip_accepts_eocd_comment_with_signature_at_end() {
        let package = ZipPackage {
            entries: vec![ZipEntry {
                name: "test.xml".to_string(),
                content_type: "application/xml".to_string(),
                data: FlatOpcData::Xml("<root/>".to_string()),
                compression: ZipCompression::Stored,
            }],
        };
        let mut bytes = serialize_zip(&package).unwrap();
        patch_eocd_comment(
            &mut bytes,
            b"comment with EOCD bytes at end \x50\x4b\x05\x06",
        );
        let result = parse_zip(&bytes);
        assert!(result.is_ok());
        assert_eq!(result.unwrap().entries.len(), 1);
    }

    #[test]
    fn parse_zip_accepts_eocd_comment_with_signature_in_middle() {
        let package = ZipPackage {
            entries: vec![ZipEntry {
                name: "test.xml".to_string(),
                content_type: "application/xml".to_string(),
                data: FlatOpcData::Xml("<root/>".to_string()),
                compression: ZipCompression::Stored,
            }],
        };
        let mut bytes = serialize_zip(&package).unwrap();
        patch_eocd_comment(
            &mut bytes,
            b"comment \x50\x4b\x05\x06 inside trailing comment",
        );
        let result = parse_zip(&bytes);
        assert!(result.is_ok());
        assert_eq!(result.unwrap().entries.len(), 1);
    }

    #[test]
    fn parse_zip_rejects_eocd_comment_with_trailing_data() {
        let package = ZipPackage {
            entries: vec![ZipEntry {
                name: "test.xml".to_string(),
                content_type: "application/xml".to_string(),
                data: FlatOpcData::Xml("<root/>".to_string()),
                compression: ZipCompression::Stored,
            }],
        };
        let mut bytes = serialize_zip(&package).unwrap();
        patch_eocd_comment(&mut bytes, b"comment with fake EOCD before end");
        bytes.extend_from_slice(b"trailing bytes after real EOCD comment");
        let result = parse_zip(&bytes);
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().kind, DocxErrorKind::InvalidPackage);
    }
}
