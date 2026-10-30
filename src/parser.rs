use crate::error::{Error, Result};
use crate::*;

/// Validate the container header; return it and the offset just past it.
fn parse_header(buf: &[u8]) -> Result<(ContainerHeader, usize)> {
    if buf.len() < HEADER_SIZE {
        return Err(Error::fmt_msg("mnfx: truncated header"));
    }
    if buf[..4] != MAGIC {
        return Err(Error::fmt_msg("mnfx: magic"));
    }
    let version = u16::from_le_bytes([buf[4], buf[5]]);
    if version != VERSION {
        return Err(Error::fmt_msg(format!(
            "mnfx: unsupported version {version}"
        )));
    }
    let pixel_format = PixelFormat::try_from(buf[6])
        .map_err(|_| Error::fmt_msg(format!("mnfx: unknown pixel format {}", buf[6])))?;
    let orientation = buf[7];
    if !(1..=8).contains(&orientation) {
        return Err(Error::fmt_msg(format!(
            "mnfx: orientation {orientation} outside 1..=8"
        )));
    }
    let chunk_count = u16::from_le_bytes([buf[8], buf[9]]);
    let width = u32::from_le_bytes([buf[10], buf[11], buf[12], buf[13]]);
    let height = u32::from_le_bytes([buf[14], buf[15], buf[16], buf[17]]);
    if width == 0 || height == 0 {
        return Err(Error::fmt_msg("mnfx: zero dims"));
    }
    // The pixel-area bound: overflow rejects here rather than downstream
    // (v1 RGB8 = 3 bytes/sample; later formats re-derive from stride).
    const SAMPLE_STRIDE: u64 = 3;
    let area = u64::from(width) * u64::from(height);
    let _expected_px_bytes = area
        .checked_mul(SAMPLE_STRIDE)
        .ok_or_else(|| Error::fmt_msg("mnfx: dims overflow the pixel accounting"))?;
    if usize::from(chunk_count) > buf.len() / CHUNK_HEADER_SIZE {
        return Err(Error::fmt_msg("mnfx: chunk count exceeds the file length"));
    }
    Ok((
        ContainerHeader {
            pixel_format,
            orientation,
            chunk_count,
            width,
            height,
        },
        HEADER_SIZE,
    ))
}

/// A decoded chunk (payload CRC-validated; still bytes at W0).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Chunk {
    /// The parsed header.
    pub header: ChunkHeader,
    /// The stored payload bytes (CRC-validated).
    pub stored: Vec<u8>,
}

/// Parse and validate an MNFX container. Payloads stay bytes (W0);
/// pixel-chunk interpretation lands at W1 via manifold-codec.
pub fn parse(buf: &[u8]) -> Result<(ContainerHeader, Vec<Chunk>)> {
    let (hdr, mut pos) = parse_header(buf)?;
    let mut chunks = Vec::with_capacity(usize::from(hdr.chunk_count).min(4096));
    let mut pixel_seen = false;
    for i in 0..u32::from(hdr.chunk_count) {
        if buf.len() < pos + CHUNK_HEADER_SIZE {
            return Err(Error::fmt_msg(format!("mnfx: truncated chunk header {i}")));
        }
        let h = &buf[pos..pos + CHUNK_HEADER_SIZE];
        let type_raw = u16::from_le_bytes([h[0], h[1]]);
        let codec = Codec::try_from(h[2])
            .map_err(|_| Error::fmt_msg(format!("mnfx: unknown chunk codec {}", h[2])))?;
        let flags = h[3];
        if flags != 0 {
            return Err(Error::fmt_msg(format!(
                "mnfx: chunk flags {flags} reserved"
            )));
        }
        let raw_len = u32::from_le_bytes([h[4], h[5], h[6], h[7]]);
        let stored_len = u32::from_le_bytes([h[8], h[9], h[10], h[11]]);
        let crc = u32::from_le_bytes([h[12], h[13], h[14], h[15]]);
        let chunk_type = ChunkType::from_raw(type_raw);
        // Unknown chunk types: critical (0x8000 bit) rejects; optional
        // skips (lossless rewrite). Known types route by value below.
        if matches!(chunk_type, ChunkType::Unknown(t) if t & 0x8000 != 0) {
            return Err(Error::fmt_msg(format!(
                "mnfx: unknown CRITICAL chunk type {:#06x}",
                type_raw
            )));
        }
        pos += CHUNK_HEADER_SIZE;
        let stored_len = stored_len as usize;
        if buf.len() < pos + stored_len {
            return Err(Error::fmt_msg(format!(
                "mnfx: chunk {i} stored_len {stored_len} exceeds the file"
            )));
        }
        // CRC BEFORE parse (normative).
        let payload = &buf[pos..pos + stored_len];
        pos += stored_len;
        let got = crc32c(payload);
        if got != crc {
            return Err(Error::fmt_msg(format!(
                "mnfx: chunk {i} crc mismatch (got {got:#010x}, want {crc:#010x})"
            )));
        }
        // raw_len bounds by codec.
        match codec {
            Codec::Raw => {
                if raw_len as usize != payload.len() {
                    return Err(Error::fmt_msg(format!(
                        "mnfx: raw chunk {i} raw_len != stored_len"
                    )));
                }
            }
            Codec::MnF8 | Codec::MnF9 => {
                if raw_len != 0 {
                    return Err(Error::fmt_msg(format!(
                        "mnfx: embed chunk {i} raw_len must be 0"
                    )));
                }
            }
            Codec::Zstd => { /* the decompression bound enforces at W1 */ }
        }
        if chunk_type == ChunkType::Pixel {
            if pixel_seen {
                return Err(Error::fmt_msg("mnfx: multiple pixel chunks"));
            }
            pixel_seen = true;
        } else if codec == Codec::MnF9 {
            return Err(Error::fmt_msg(format!(
                "mnfx: chunk {i} uses codec 5 outside a pixel layer"
            )));
            // Codec 4 on non-pixel chunks stays legal: W3 plane chunks
            // (the grammar-frozen MNF8/zstd/raw vocabulary) use it.
        }
        chunks.push(Chunk {
            header: ChunkHeader {
                chunk_type,
                codec,
                stored_len: stored_len as u32,
                raw_len,
                crc,
                payload_offset: pos - stored_len,
            },
            stored: payload.to_vec(),
        });
    }
    if !pixel_seen {
        return Err(Error::fmt_msg("mnfx: no pixel chunk"));
    }
    Ok((hdr, chunks))
}

/// Serialize a container from validated parts (W0: raw-coded payloads).
pub fn serialize(
    header: &ContainerHeader,
    chunks: &[(ChunkType, Codec, &[u8])],
) -> Result<Vec<u8>> {
    if chunks.is_empty() {
        return Err(Error::fmt_msg("mnfx: no chunks"));
    }
    if usize::from(header.chunk_count) != chunks.len() {
        return Err(Error::fmt_msg("mnfx: chunk_count != chunks.len()"));
    }
    let mut out = Vec::new();
    out.extend_from_slice(&MAGIC);
    out.extend_from_slice(&VERSION.to_le_bytes());
    out.push(header.pixel_format as u8);
    out.push(header.orientation);
    out.extend_from_slice(&header.chunk_count.to_le_bytes());
    out.extend_from_slice(&header.width.to_le_bytes());
    out.extend_from_slice(&header.height.to_le_bytes());
    for (chunk_type, codec, payload) in chunks {
        let t: u16 = chunk_type.raw();
        out.extend_from_slice(&t.to_le_bytes());
        out.push(*codec as u8);
        out.push(0u8); // flags reserved
        let raw_len = match codec {
            Codec::Raw => payload.len() as u32,
            Codec::MnF8 | Codec::MnF9 => 0u32,
            Codec::Zstd => 0u32, // set by the zstd layer at W1
        };
        out.extend_from_slice(&raw_len.to_le_bytes());
        out.extend_from_slice(&(payload.len() as u32).to_le_bytes());
        out.extend_from_slice(&crc32c(payload).to_le_bytes());
        out.extend_from_slice(payload);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_header() -> ContainerHeader {
        ContainerHeader {
            pixel_format: PixelFormat::Rgb8,
            orientation: 1,
            chunk_count: 1,
            width: 8,
            height: 8,
        }
    }

    fn rgb_payload() -> Vec<u8> {
        vec![7u8; 8 * 8 * 3]
    }

    #[test]
    fn roundtrip_single_raw_pixel_chunk() {
        let hdr = sample_header();
        let payload = rgb_payload();
        let buf = serialize(&hdr, &[(ChunkType::Pixel, Codec::Raw, &payload)]).unwrap();
        let (h, chunks) = parse(&buf).unwrap();
        assert_eq!(h, hdr);
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].header.chunk_type, ChunkType::Pixel);
        assert_eq!(chunks[0].stored, payload);
    }

    #[test]
    fn rejects_bad_magic() {
        let mut buf = serialize(
            &sample_header(),
            &[(ChunkType::Pixel, Codec::Raw, &rgb_payload())],
        )
        .unwrap();
        buf[0] = b'X';
        assert!(parse(&buf).is_err());
    }

    #[test]
    fn rejects_truncated_header() {
        let buf = [0u8; 17];
        assert!(parse(&buf).is_err());
    }

    #[test]
    fn rejects_crc_mismatch() {
        let mut buf = serialize(
            &sample_header(),
            &[(ChunkType::Pixel, Codec::Raw, &rgb_payload())],
        )
        .unwrap();
        let last = buf.len() - 1;
        buf[last] ^= 0xFF; // flip a payload byte: CRC now wrong
        assert!(parse(&buf).is_err());
    }

    #[test]
    fn rejects_stored_len_overrun() {
        let mut buf = serialize(
            &sample_header(),
            &[(ChunkType::Pixel, Codec::Raw, &rgb_payload())],
        )
        .unwrap();
        // stored_len sits at header(18) + 8: bump past the file end
        let stored_field = HEADER_SIZE + 8;
        let cur = u32::from_le_bytes([
            buf[stored_field],
            buf[stored_field + 1],
            buf[stored_field + 2],
            buf[stored_field + 3],
        ]);
        buf[stored_field..stored_field + 4].copy_from_slice(&(cur + 100).to_le_bytes());
        assert!(parse(&buf).is_err());
    }

    #[test]
    fn rejects_unknown_critical_chunk() {
        let hdr = ContainerHeader {
            chunk_count: 2,
            ..sample_header()
        };
        let buf = serialize(
            &hdr,
            &[
                (ChunkType::Pixel, Codec::Raw, &rgb_payload()),
                (ChunkType::Unknown(0x8321), Codec::Raw, b"payload"),
            ],
        )
        .unwrap();
        assert!(parse(&buf).is_err(), "unknown critical chunk must reject");
    }

    #[test]
    fn skips_unknown_optional_chunk() {
        let hdr = ContainerHeader {
            chunk_count: 2,
            ..sample_header()
        };
        let buf = serialize(
            &hdr,
            &[
                (ChunkType::Pixel, Codec::Raw, &rgb_payload()),
                (ChunkType::Unknown(0x0042), Codec::Raw, b"payload"),
            ],
        )
        .unwrap();
        let (h, chunks) = parse(&buf).unwrap();
        assert_eq!(h.chunk_count, 2);
        assert_eq!(chunks.len(), 2);
        assert!(matches!(
            chunks[1].header.chunk_type,
            ChunkType::Unknown(0x0042)
        ));
    }

    #[test]
    fn rejects_second_pixel_chunk() {
        let hdr = ContainerHeader {
            chunk_count: 2,
            ..sample_header()
        };
        let buf = serialize(
            &hdr,
            &[
                (ChunkType::Pixel, Codec::Raw, &rgb_payload()),
                (ChunkType::Pixel, Codec::Raw, &rgb_payload()),
            ],
        )
        .unwrap();
        assert!(parse(&buf).is_err());
    }

    #[test]
    fn rejects_orientation_out_of_range() {
        let hdr = ContainerHeader {
            orientation: 9,
            ..sample_header()
        };
        let buf = serialize(&hdr, &[(ChunkType::Pixel, Codec::Raw, &rgb_payload())]).unwrap();
        assert!(parse(&buf).is_err());
    }

    #[test]
    fn rejects_zero_dims() {
        let hdr = ContainerHeader {
            width: 0,
            ..sample_header()
        };
        let buf = serialize(&hdr, &[(ChunkType::Pixel, Codec::Raw, &rgb_payload())]).unwrap();
        assert!(parse(&buf).is_err());
    }

    #[test]
    fn crc32c_matches_known_vectors() {
        // RFC 3720 test vectors for CRC-32C (Castagnoli)
        assert_eq!(crc32c(b""), 0x0000_0000);
        assert_eq!(crc32c(b"123456789"), 0xE306_9283);
        assert_eq!(crc32c(&[0u8; 32]), 0x8A91_36AA);
    }
}
