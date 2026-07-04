use std::io::Read;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MetafileKind {
    Emf,
    Wmf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct MetafileRaster {
    pub(crate) rgba: Vec<u8>,
    pub(crate) width_px: u32,
    pub(crate) height_px: u32,
}

pub(crate) fn format_for_part(name: &str) -> Option<(MetafileKind, bool)> {
    let ext = name.rsplit('.').next()?.to_ascii_lowercase();
    match ext.as_str() {
        "emf" => Some((MetafileKind::Emf, false)),
        "wmf" => Some((MetafileKind::Wmf, false)),
        "emz" => Some((MetafileKind::Emf, true)),
        "wmz" => Some((MetafileKind::Wmf, true)),
        _ => None,
    }
}

pub(crate) fn is_gzip_payload(bytes: &[u8]) -> bool {
    bytes.len() >= 2 && bytes[0] == 0x1f && bytes[1] == 0x8b
}

pub(crate) fn dimensions(
    kind: MetafileKind,
    bytes: &[u8],
    compressed: bool,
) -> (Option<u32>, Option<u32>) {
    let inflated = if compressed && is_gzip_payload(bytes) {
        inflate_gzip_metafile(bytes)
    } else {
        None
    };
    let payload = match (compressed, inflated.as_deref()) {
        (true, Some(payload)) => payload,
        (true, None) => return (None, None),
        (false, _) => bytes,
    };
    dimensions_uncompressed(kind, payload).unzip()
}

pub(crate) fn extract_raster(
    kind: MetafileKind,
    bytes: &[u8],
    compressed: bool,
) -> Option<MetafileRaster> {
    let inflated = if compressed && is_gzip_payload(bytes) {
        inflate_gzip_metafile(bytes)
    } else {
        None
    };
    let payload = match (compressed, inflated.as_deref()) {
        (true, Some(payload)) => payload,
        (true, None) => return None,
        (false, _) => bytes,
    };
    let (frame_w, frame_h) = dimensions_uncompressed(kind, payload)?;
    let raster = match kind {
        MetafileKind::Emf => extract_emf_stretchdibits(payload)?,
        MetafileKind::Wmf => extract_wmf_stretchdib(payload)?,
    };
    (raster.width_px == frame_w && raster.height_px == frame_h).then_some(raster)
}

fn inflate_gzip_metafile(bytes: &[u8]) -> Option<Vec<u8>> {
    const MAX_METAFILE_INFLATE: u64 = 1 << 20;
    let decoder = flate2::read::GzDecoder::new(bytes);
    let mut limited = decoder.take(MAX_METAFILE_INFLATE);
    let mut out = Vec::new();
    limited.read_to_end(&mut out).ok()?;
    (!out.is_empty()).then_some(out)
}

fn dimensions_uncompressed(kind: MetafileKind, bytes: &[u8]) -> Option<(u32, u32)> {
    match kind {
        MetafileKind::Emf => emf_dimensions(bytes),
        MetafileKind::Wmf => wmf_dimensions(bytes),
    }
}

fn emf_dimensions(bytes: &[u8]) -> Option<(u32, u32)> {
    if bytes.len() < 44 || read_u32le(bytes, 0)? != 1 {
        return None;
    }
    let header_size = read_u32le(bytes, 4)? as usize;
    if header_size < 44 || header_size > bytes.len() || bytes.get(40..44)? != b" EMF" {
        return None;
    }
    rect_dimensions(
        read_i32le(bytes, 8)?,
        read_i32le(bytes, 12)?,
        read_i32le(bytes, 16)?,
        read_i32le(bytes, 20)?,
    )
}

fn wmf_dimensions(bytes: &[u8]) -> Option<(u32, u32)> {
    if bytes.len() < 22 || read_u32le(bytes, 0)? != 0x9AC6CDD7 {
        return None;
    }
    let units_per_inch = read_u16le(bytes, 14)? as u32;
    if units_per_inch == 0 {
        return None;
    }
    let width_units = (read_i16le(bytes, 10)? as i32) - (read_i16le(bytes, 6)? as i32);
    let height_units = (read_i16le(bytes, 12)? as i32) - (read_i16le(bytes, 8)? as i32);
    let width_px = scale_wmf_units(width_units, units_per_inch)?;
    let height_px = scale_wmf_units(height_units, units_per_inch)?;
    Some((width_px, height_px))
}

fn extract_emf_stretchdibits(bytes: &[u8]) -> Option<MetafileRaster> {
    if bytes.len() < 44 || read_u32le(bytes, 0)? != 1 {
        return None;
    }
    let mut offset = 0usize;
    let mut stretchdibits = 0usize;
    let mut raster = None;
    while offset.checked_add(8)? <= bytes.len() {
        let record_type = read_u32le(bytes, offset)?;
        let record_size = read_u32le(bytes, offset + 4)? as usize;
        if record_size < 8 || record_size % 4 != 0 {
            return None;
        }
        let end = offset.checked_add(record_size)?;
        if end > bytes.len() {
            return None;
        }
        if record_type == 81 {
            stretchdibits += 1;
            if stretchdibits > 1 {
                return None;
            }
            raster = extract_emf_stretchdibits_record(&bytes[offset..end]);
        }
        offset = end;
    }
    (stretchdibits == 1).then_some(raster?)
}

fn extract_emf_stretchdibits_record(record: &[u8]) -> Option<MetafileRaster> {
    if record.len() < 64 {
        return None;
    }
    let off_bmi = read_u32le(record, 48)? as usize;
    let cb_bmi = read_u32le(record, 52)? as usize;
    let off_bits = read_u32le(record, 56)? as usize;
    let cb_bits = read_u32le(record, 60)? as usize;
    let bmi_end = off_bmi.checked_add(cb_bmi)?;
    let bits_end = off_bits.checked_add(cb_bits)?;
    let bmi = record.get(off_bmi..bmi_end)?;
    let bits = record.get(off_bits..bits_end)?;
    decode_dib(bmi, bits)
}

fn extract_wmf_stretchdib(bytes: &[u8]) -> Option<MetafileRaster> {
    let mut offset = if bytes.len() >= 22 && read_u32le(bytes, 0)? == 0x9AC6CDD7 {
        22usize
    } else {
        0usize
    };
    if bytes.len() < offset.checked_add(18)? {
        return None;
    }
    offset += 18;
    let mut stretchdib = 0usize;
    let mut raster = None;
    while offset.checked_add(6)? <= bytes.len() {
        let record_words = read_u32le(bytes, offset)? as usize;
        if record_words < 3 {
            return None;
        }
        let record_size = record_words.checked_mul(2)?;
        let end = offset.checked_add(record_size)?;
        if end > bytes.len() {
            return None;
        }
        let function = read_u16le(bytes, offset + 4)?;
        if function == 0 {
            break;
        }
        if function == 0x0F43 {
            stretchdib += 1;
            if stretchdib > 1 {
                return None;
            }
            let dib_start = offset.checked_add(6 + 22)?;
            let dib = bytes.get(dib_start..end)?;
            raster = decode_packed_dib(dib);
        }
        offset = end;
    }
    (stretchdib == 1).then_some(raster?)
}

fn decode_packed_dib(dib: &[u8]) -> Option<MetafileRaster> {
    let header_len = bitmap_info_header_len(dib)?;
    decode_dib(dib.get(..header_len)?, dib.get(header_len..)?)
}

fn bitmap_info_header_len(dib: &[u8]) -> Option<usize> {
    if dib.len() < 40 || read_u32le(dib, 0)? != 40 {
        return None;
    }
    let bit_count = read_u16le(dib, 14)?;
    let compression = read_u32le(dib, 16)?;
    let colors_used = read_u32le(dib, 32)?;
    if compression != 0 || !matches!(bit_count, 24 | 32) || colors_used != 0 {
        return None;
    }
    Some(40)
}

fn decode_dib(bmi: &[u8], bits: &[u8]) -> Option<MetafileRaster> {
    if bmi.len() < 40 || read_u32le(bmi, 0)? != 40 {
        return None;
    }
    let width = read_i32le(bmi, 4)?;
    let height = read_i32le(bmi, 8)?;
    let planes = read_u16le(bmi, 12)?;
    let bit_count = read_u16le(bmi, 14)?;
    let compression = read_u32le(bmi, 16)?;
    let colors_used = read_u32le(bmi, 32)?;
    if width <= 0
        || height == 0
        || planes != 1
        || compression != 0
        || colors_used != 0
        || !matches!(bit_count, 24 | 32)
    {
        return None;
    }
    let width = width as u32;
    let height_abs = height.unsigned_abs();
    let bytes_per_pixel = (bit_count / 8) as usize;
    let row_stride = dib_row_stride(width, bit_count)?;
    let needed = row_stride.checked_mul(height_abs as usize)?;
    if bits.len() < needed {
        return None;
    }
    let pixel_count = (width as usize).checked_mul(height_abs as usize)?;
    let mut rgba = vec![0u8; pixel_count.checked_mul(4)?];
    for y in 0..height_abs as usize {
        let src_y = if height < 0 {
            y
        } else {
            height_abs as usize - 1 - y
        };
        let row = bits.get(src_y * row_stride..src_y * row_stride + row_stride)?;
        for x in 0..width as usize {
            let src = x.checked_mul(bytes_per_pixel)?;
            let dst = (y.checked_mul(width as usize)?.checked_add(x)?).checked_mul(4)?;
            rgba[dst] = *row.get(src + 2)?;
            rgba[dst + 1] = *row.get(src + 1)?;
            rgba[dst + 2] = *row.get(src)?;
            rgba[dst + 3] = if bit_count == 32 {
                *row.get(src + 3)?
            } else {
                255
            };
        }
    }
    Some(MetafileRaster {
        rgba,
        width_px: width,
        height_px: height_abs,
    })
}

fn dib_row_stride(width: u32, bit_count: u16) -> Option<usize> {
    let bits = (width as usize).checked_mul(bit_count as usize)?;
    bits.checked_add(31)?.checked_div(32)?.checked_mul(4)
}

fn rect_dimensions(left: i32, top: i32, right: i32, bottom: i32) -> Option<(u32, u32)> {
    let width = right.checked_sub(left)?;
    let height = bottom.checked_sub(top)?;
    (width > 0 && height > 0).then_some((width as u32, height as u32))
}

fn scale_wmf_units(value: i32, units_per_inch: u32) -> Option<u32> {
    if value <= 0 {
        return None;
    }
    let value = value as u64;
    let units = units_per_inch as u64;
    let px = (value * 96 + units / 2) / units;
    (px > 0 && px <= u32::MAX as u64).then_some(px as u32)
}

fn read_u16le(bytes: &[u8], offset: usize) -> Option<u16> {
    let end = offset.checked_add(2)?;
    Some(u16::from_le_bytes(bytes.get(offset..end)?.try_into().ok()?))
}

fn read_i16le(bytes: &[u8], offset: usize) -> Option<i16> {
    let end = offset.checked_add(2)?;
    Some(i16::from_le_bytes(bytes.get(offset..end)?.try_into().ok()?))
}

fn read_u32le(bytes: &[u8], offset: usize) -> Option<u32> {
    let end = offset.checked_add(4)?;
    Some(u32::from_le_bytes(bytes.get(offset..end)?.try_into().ok()?))
}

fn read_i32le(bytes: &[u8], offset: usize) -> Option<i32> {
    let end = offset.checked_add(4)?;
    Some(i32::from_le_bytes(bytes.get(offset..end)?.try_into().ok()?))
}
