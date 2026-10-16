#![cfg(feature = "docx")]

//! Saved and freshly written packages record the same ZIP creating host on every
//! platform, so their bytes do not depend on where `rwml` was built.

use rwml::{write_docx, DocModel, Document};

/// The "version made by" host byte of every central-directory record
/// (APPNOTE 4.3.12 and 4.4.2), found by walking from the end-of-central-directory record.
fn central_directory_hosts(bytes: &[u8]) -> Vec<u8> {
    let eocd = bytes
        .windows(4)
        .rposition(|window| window == b"PK\x05\x06")
        .expect("end of central directory");
    let u16_at = |at: usize| u16::from_le_bytes([bytes[at], bytes[at + 1]]) as usize;
    let u32_at = |at: usize| {
        u32::from_le_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]]) as usize
    };
    let count = u16_at(eocd + 10);
    let mut at = u32_at(eocd + 16);
    let mut hosts = Vec::with_capacity(count);
    for _ in 0..count {
        assert_eq!(
            &bytes[at..at + 4],
            b"PK\x01\x02",
            "central header signature"
        );
        hosts.push(bytes[at + 5]);
        at += 46 + u16_at(at + 28) + u16_at(at + 30) + u16_at(at + 32);
    }
    hosts
}

#[test]
fn saved_and_fresh_packages_record_a_unix_host_on_every_platform() {
    const UNIX: u8 = 3;
    let fresh = write_docx(&DocModel::default());
    let saved = Document::open(&fresh).unwrap().save().unwrap();

    for (label, bytes) in [("fresh", &fresh), ("saved", &saved)] {
        let hosts = central_directory_hosts(bytes);
        assert!(!hosts.is_empty(), "{label} package has entries");
        assert!(
            hosts.iter().all(|&host| host == UNIX),
            "{label} package hosts: {hosts:?}"
        );
    }
}
