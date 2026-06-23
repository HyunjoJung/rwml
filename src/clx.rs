//! CLX / piece-table parsing.
//!
//! The CLX (in the table stream at `[fcClx, fcClx+lcbClx)`) is zero or more
//! `Prc` blocks (`0x01`) followed by one `Pcdt` (`0x02`) whose body is a
//! `PlcPcd` — the piece table. The piece table maps character positions to byte
//! offsets in the `WordDocument` stream and records, per piece, whether the
//! text is 1-byte ANSI (`fCompressed`) or 2-byte UTF-16LE.
//!
//! Reference: [MS-DOC] 2.8.35 (Pcdt), 2.8.34 (Prc), 2.9.177 (PlcPcd), 2.9.176 (Pcd).

use crate::error::{Error, Result};
use crate::util::{u16le, u32le};

/// Upper bound on the piece count — far beyond any real `.doc` (a piece per fast-save edit;
/// real documents have at most thousands), but it bounds a crafted PlcPcd that would
/// otherwise declare millions of overlapping pieces to amplify decoding (see `parse_plcpcd`).
const MAX_PIECES: usize = 1 << 20;

/// One text piece resolved from the piece table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Piece {
    /// Character count of this piece (`cp[i+1] - cp[i]`).
    pub cch: usize,
    /// Byte offset of the piece text in the `WordDocument` stream.
    pub fc: usize,
    /// `true` = 1-byte ANSI (cp1252), `false` = 2-byte UTF-16LE.
    pub compressed: bool,
}

/// Parse the CLX, returning the ordered piece list.
pub(crate) fn parse(clx: &[u8]) -> Result<Vec<Piece>> {
    let mut pos = 0usize;
    loop {
        let marker = *clx
            .get(pos)
            .ok_or_else(|| Error::PieceTable("CLX truncated before Pcdt".into()))?;
        match marker {
            0x01 => {
                // Prc: 1-byte clxt + 2-byte cbGrpprl + cbGrpprl bytes.
                let cb = u16le(clx, pos + 1)
                    .ok_or_else(|| Error::PieceTable("truncated Prc".into()))?
                    as usize;
                pos = pos
                    .checked_add(3 + cb)
                    .ok_or_else(|| Error::PieceTable("Prc length overflow".into()))?;
            }
            0x02 => {
                // Pcdt: 1-byte clxt + 4-byte lcb + PlcPcd[lcb].
                let lcb = u32le(clx, pos + 1)
                    .ok_or_else(|| Error::PieceTable("truncated Pcdt".into()))?
                    as usize;
                let start = pos + 5;
                let plc = clx
                    .get(start..start.saturating_add(lcb))
                    .ok_or_else(|| Error::PieceTable("PlcPcd out of CLX bounds".into()))?;
                return parse_plcpcd(plc);
            }
            other => {
                return Err(Error::PieceTable(format!(
                    "unexpected CLX marker 0x{other:02x}"
                )))
            }
        }
    }
}

/// PlcPcd = (n+1) CP entries (u32) followed by n PCD entries (8 bytes each).
fn parse_plcpcd(plc: &[u8]) -> Result<Vec<Piece>> {
    if plc.len() < 4 || (plc.len() - 4) % 12 != 0 {
        return Err(Error::PieceTable(format!(
            "bad PlcPcd length {}",
            plc.len()
        )));
    }
    // Cap the piece count: `n` scales with the (uncapped) table stream, and pieces may
    // overlap (all point at the same WordDocument bytes), so an unbounded count is one half
    // of an N×W decode-amplification DoS (the other half is bounded in `text`/`assemble`).
    // Mirrors the FKP/FFN/style caps elsewhere; far above any real document's run count.
    let n = ((plc.len() - 4) / 12).min(MAX_PIECES);
    let mut pieces = Vec::with_capacity(n);
    for i in 0..n {
        let cp0 = u32le(plc, i * 4).unwrap_or(0) as i64;
        let cp1 = u32le(plc, (i + 1) * 4).unwrap_or(0) as i64;
        let cch = (cp1 - cp0).max(0) as usize;

        // PCD: [0..2] flags, [2..6] FcCompressed, [6..8] prm.
        let pcd_off = (n + 1) * 4 + i * 8;
        let fc_compressed =
            u32le(plc, pcd_off + 2).ok_or_else(|| Error::PieceTable("truncated PCD".into()))?;
        let compressed = (fc_compressed & 0x4000_0000) != 0;
        let fc30 = (fc_compressed & 0x3FFF_FFFF) as usize;
        let fc = if compressed { fc30 / 2 } else { fc30 };
        pieces.push(Piece {
            cch,
            fc,
            compressed,
        });
    }
    Ok(pieces)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a one-piece Pcdt: CPs `[0, 5]` then one PCD with `fc=0x100`,
    /// uncompressed.
    fn one_piece_pcdt() -> Vec<u8> {
        let mut plc = Vec::new();
        plc.extend_from_slice(&0u32.to_le_bytes());
        plc.extend_from_slice(&5u32.to_le_bytes());
        plc.extend_from_slice(&0u16.to_le_bytes()); // PCD flags
        plc.extend_from_slice(&0x100u32.to_le_bytes()); // FcCompressed (uncompressed)
        plc.extend_from_slice(&0u16.to_le_bytes()); // prm
        let mut clx = vec![0x02u8];
        clx.extend_from_slice(&(plc.len() as u32).to_le_bytes());
        clx.extend_from_slice(&plc);
        clx
    }

    #[test]
    fn parses_single_uncompressed_piece() {
        let pieces = parse(&one_piece_pcdt()).unwrap();
        assert_eq!(
            pieces,
            vec![Piece {
                cch: 5,
                fc: 0x100,
                compressed: false
            }]
        );
    }

    #[test]
    fn skips_leading_prc_block() {
        // Prc: 0x01 + cbGrpprl(2) + 2 bytes, then the Pcdt.
        let mut clx = vec![0x01u8, 0x02, 0x00, 0xAA, 0xBB];
        clx.extend_from_slice(&one_piece_pcdt());
        let pieces = parse(&clx).unwrap();
        assert_eq!(pieces.len(), 1);
        assert_eq!(pieces[0].fc, 0x100);
    }

    #[test]
    fn rejects_unknown_marker() {
        assert!(parse(&[0x99]).is_err());
    }
}
