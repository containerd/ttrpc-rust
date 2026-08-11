// Copyright 2026 Alibaba Cloud. All rights reserved.
//
// SPDX-License-Identifier: Apache-2.0
//
//! Shared test utilities for hook integration tests.

use std::sync::atomic::{AtomicU64, Ordering};

use ttrpc::security_extension::PayloadTransform;

// ── XOR Transform constants ─────────────────────────────────────────────────

pub const XOR_KEY: u16 = 0xA5A5;
pub const XOR_ALGO_ID: u16 = 0x0001;
/// algo_id(2) + aad_tag(2) + payload_len(4)
pub const XOR_HEADER_LEN: usize = 8;

// ── XOR PayloadTransform ────────────────────────────────────────────────────

/// Symmetric XOR-based payload transform for testing.
///
/// Wire format: `[algo_id:u16][aad_tag:u16][payload_len:u32][encrypted_data...]`
/// Encryption: XOR each u16 word with an AAD-derived effective key.
///
/// The effective key is `XOR_KEY ^ aad_fold(aad)`, so any tampering with
/// the header fields (`stream_id`, `type_`, `flags`) is detected via the
/// `aad_tag` check on the inbound path.
#[derive(Debug)]
pub struct XorPayloadTransform;

/// Fold the 6-byte AAD (`stream_id || type_ || flags`) into a u16 by
/// XOR-ing adjacent byte pairs.
pub fn aad_to_u16(aad: &[u8]) -> u16 {
    let mut k: u16 = 0;
    for (i, &b) in aad.iter().enumerate() {
        if i % 2 == 0 {
            k ^= (b as u16) << 8;
        } else {
            k ^= b as u16;
        }
    }
    k
}

fn effective_key(aad: &[u8]) -> u16 {
    XOR_KEY ^ aad_to_u16(aad)
}

impl PayloadTransform for XorPayloadTransform {
    fn transform_outbound(&self, data: Vec<u8>, aad: &[u8]) -> Result<Vec<u8>, String> {
        let key = effective_key(aad);
        let payload_len = data.len();
        let mut padded = data;
        if padded.len() % 2 != 0 {
            padded.push(0x00);
        }
        let mut encrypted = Vec::with_capacity(XOR_HEADER_LEN + padded.len());
        encrypted.extend_from_slice(&XOR_ALGO_ID.to_be_bytes());
        encrypted.extend_from_slice(&aad_to_u16(aad).to_be_bytes()); // aad_tag
        encrypted.extend_from_slice(&(payload_len as u32).to_be_bytes());
        for chunk in padded.chunks(2) {
            let val = u16::from_be_bytes([chunk[0], chunk[1]]);
            let xored = val ^ key;
            encrypted.extend_from_slice(&xored.to_be_bytes());
        }
        Ok(encrypted)
    }

    fn transform_inbound(&self, data: Vec<u8>, aad: &[u8]) -> Result<Vec<u8>, String> {
        if data.len() < XOR_HEADER_LEN {
            return Err(format!(
                "xor: packet too short ({} < {})",
                data.len(),
                XOR_HEADER_LEN
            ));
        }
        let algo_id = u16::from_be_bytes([data[0], data[1]]);
        if algo_id != XOR_ALGO_ID {
            return Err(format!("xor: unknown algo 0x{:04X}", algo_id));
        }
        // Verify AAD integrity tag
        let stored_tag = u16::from_be_bytes([data[2], data[3]]);
        let expected_tag = aad_to_u16(aad);
        if stored_tag != expected_tag {
            return Err(format!(
                "xor: AAD mismatch (stored=0x{:04X}, expected=0x{:04X}) — header tampered",
                stored_tag, expected_tag
            ));
        }
        let payload_len = u32::from_be_bytes([data[4], data[5], data[6], data[7]]) as usize;
        let encrypted = &data[XOR_HEADER_LEN..];
        if encrypted.len() % 2 != 0 {
            return Err("xor: encrypted data has odd length".into());
        }
        let key = effective_key(aad);
        let mut decrypted = Vec::with_capacity(encrypted.len());
        for chunk in encrypted.chunks(2) {
            let val = u16::from_be_bytes([chunk[0], chunk[1]]);
            let xored = val ^ key;
            decrypted.extend_from_slice(&xored.to_be_bytes());
        }
        decrypted.truncate(payload_len);
        Ok(decrypted)
    }
}

// ── Socket path helper ──────────────────────────────────────────────────────

static SOCKET_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Generate a unique temporary Unix socket path and clean up any stale file.
pub fn temp_unix_socket_path() -> String {
    let id = SOCKET_COUNTER.fetch_add(1, Ordering::SeqCst);
    let path = format!("/tmp/ttrpc_test_{}_{}.sock", std::process::id(), id);
    let _ = std::fs::remove_file(&path);
    path
}

/// Remove a socket file (ignores errors).
pub fn cleanup_socket_file(path: &str) {
    let _ = std::fs::remove_file(path);
}

// ── AAD integrity tests ──────────────────────────────────────────────────────

/// Build a standard 6-byte AAD from `stream_id`, `type_`, `flags`.
pub fn make_aad(stream_id: u32, type_: u8, flags: u8) -> [u8; 6] {
    let mut aad = [0u8; 6];
    aad[0..4].copy_from_slice(&stream_id.to_be_bytes());
    aad[4] = type_;
    aad[5] = flags;
    aad
}

#[cfg(test)]
mod aad_tests {
    use super::*;

    #[test]
    fn roundtrip_with_matching_aad() {
        let xform = XorPayloadTransform;
        let aad = make_aad(1, 0x01, 0x00); // stream_id=1, REQUEST, no flags
        let plaintext = b"hello world".to_vec();

        let encrypted = xform.transform_outbound(plaintext.clone(), &aad).unwrap();
        let decrypted = xform.transform_inbound(encrypted, &aad).unwrap();
        assert_eq!(decrypted, plaintext);
    }

    #[test]
    fn detect_stream_id_tampering() {
        let xform = XorPayloadTransform;
        let aad_orig = make_aad(1, 0x01, 0x00);
        let plaintext = b"secret data".to_vec();

        let encrypted = xform.transform_outbound(plaintext, &aad_orig).unwrap();

        // Attacker changes stream_id in the header
        let aad_tampered = make_aad(99, 0x01, 0x00);
        let err = xform
            .transform_inbound(encrypted, &aad_tampered)
            .unwrap_err();
        assert!(
            err.contains("AAD mismatch"),
            "expected AAD error, got: {}",
            err
        );
    }

    #[test]
    fn detect_message_type_tampering() {
        let xform = XorPayloadTransform;
        let aad_orig = make_aad(3, 0x01, 0x00); // REQUEST
        let plaintext = b"request payload".to_vec();

        let encrypted = xform.transform_outbound(plaintext, &aad_orig).unwrap();

        // Attacker changes type_ from REQUEST (0x01) to RESPONSE (0x02)
        let aad_tampered = make_aad(3, 0x02, 0x00);
        let err = xform
            .transform_inbound(encrypted, &aad_tampered)
            .unwrap_err();
        assert!(
            err.contains("AAD mismatch"),
            "expected AAD error, got: {}",
            err
        );
    }

    #[test]
    fn detect_flags_tampering() {
        let xform = XorPayloadTransform;
        let aad_orig = make_aad(5, 0x03, 0x00); // DATA, no flags
        let plaintext = b"streaming chunk".to_vec();

        let encrypted = xform.transform_outbound(plaintext, &aad_orig).unwrap();

        // Attacker sets flags to 0xFF
        let aad_tampered = make_aad(5, 0x03, 0xFF);
        let err = xform
            .transform_inbound(encrypted, &aad_tampered)
            .unwrap_err();
        assert!(
            err.contains("AAD mismatch"),
            "expected AAD error, got: {}",
            err
        );
    }

    #[test]
    fn detect_close_frame_aad() {
        // Close messages are DATA frames with FLAG_REMOTE_CLOSED | FLAG_NO_DATA.
        // AAD = stream_id || type_(DATA=0x03) || flags(0x05).
        let xform = XorPayloadTransform;
        let aad_close = make_aad(7, 0x03, 0x05); // DATA type, close flags
        let empty_payload = vec![]; // close has empty payload

        let encrypted = xform
            .transform_outbound(empty_payload.clone(), &aad_close)
            .unwrap();

        // Legitimate close decrypts fine
        let decrypted = xform
            .transform_inbound(encrypted.clone(), &aad_close)
            .unwrap();
        assert_eq!(decrypted, empty_payload);

        // But if stream_id is changed, the close is rejected
        let aad_wrong = make_aad(99, 0x03, 0x05);
        let err = xform.transform_inbound(encrypted, &aad_wrong).unwrap_err();
        assert!(
            err.contains("AAD mismatch"),
            "expected AAD error, got: {}",
            err
        );
    }
}
