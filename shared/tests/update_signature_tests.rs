//! Tests for the signed update: the signature over SHA256SUMS.txt, its address, and where files may come from.

use ed25519_dalek::{Signer, SigningKey};
use juicebar_shared::update::{install_question, is_allowed_download_url, is_release_signed, is_signed_by, signature_url, RELEASE_PUBLIC_KEY};

const SUMS: &[u8] = b"ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad  JuiceBar-mac-arm64.zip\r\n";
const REPOSITORY: &str = "https://github.com/developedByAshu/juice-bar";

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// A throwaway key from a fixed seed, and its public half as hex.
fn test_key(seed: u8) -> (SigningKey, String) {
    let key = SigningKey::from_bytes(&[seed; 32]);
    let public = hex(key.verifying_key().as_bytes());
    (key, public)
}

#[test]
fn a_good_signature_over_the_exact_sums_passes() {
    let (key, public) = test_key(7);
    let signature = hex(&key.sign(SUMS).to_bytes());
    assert_eq!(signature.len(), 128);
    assert!(is_signed_by(&public, SUMS, &signature));
    assert!(is_signed_by(&public, SUMS, &format!("{signature}\n")), "the .sig file's line ending is fine");
    assert!(is_signed_by(&public, SUMS, &signature.to_uppercase()), "hex case does not matter");
}

#[test]
fn a_bad_signature_fails() {
    let (key, public) = test_key(7);
    let mut signature = key.sign(SUMS).to_bytes();
    signature[10] ^= 0x01;
    assert!(!is_signed_by(&public, SUMS, &hex(&signature)));
    assert!(!is_signed_by(&public, SUMS, &"0".repeat(128)));
    assert!(!is_signed_by(&public, SUMS, ""));
}

#[test]
fn a_signature_by_another_key_fails() {
    let (key, _) = test_key(7);
    let (_, other_public) = test_key(8);
    let signature = hex(&key.sign(SUMS).to_bytes());
    assert!(!is_signed_by(&other_public, SUMS, &signature));
    assert!(!is_release_signed(SUMS, &signature), "only the release key counts for a real update");
    assert_eq!(RELEASE_PUBLIC_KEY.len(), 64);
}

#[test]
fn truncated_or_broken_hex_fails() {
    let (key, public) = test_key(7);
    let signature = hex(&key.sign(SUMS).to_bytes());
    assert!(!is_signed_by(&public, SUMS, &signature[..126]));
    assert!(!is_signed_by(&public, SUMS, &format!("{signature}00")));
    assert!(!is_signed_by(&public, SUMS, &format!("zz{}", &signature[2..])));
    assert!(!is_signed_by(&public[..62], SUMS, &signature), "a short public key fails too");
}

#[test]
fn tampered_sums_fail() {
    let (key, public) = test_key(7);
    let signature = hex(&key.sign(SUMS).to_bytes());
    let mut tampered = SUMS.to_vec();
    tampered[0] = b'c';
    assert!(!is_signed_by(&public, &tampered, &signature));
    assert!(!is_signed_by(&public, &SUMS[..SUMS.len() - 2], &signature), "even a dropped line ending fails");
}

#[test]
fn the_signature_sits_on_main_under_signatures() {
    assert_eq!(signature_url(REPOSITORY, "v0.1.3").as_deref(), Some("https://raw.githubusercontent.com/developedByAshu/juice-bar/main/signatures/v0.1.3.sig"));
    assert_eq!(signature_url(REPOSITORY, "../../evil"), None, "a tag cannot leave the folder");
    assert_eq!(signature_url(REPOSITORY, "v1/../x"), None);
    assert_eq!(signature_url(REPOSITORY, ""), None);
    assert_eq!(signature_url("https://gitlab.com/a/b", "v1.0.0"), None);
}

#[test]
fn update_files_come_only_from_github() {
    assert!(is_allowed_download_url("https://github.com/developedByAshu/juice-bar/releases/download/v0.1.3/SHA256SUMS.txt"));
    assert!(is_allowed_download_url("https://objects.githubusercontent.com/github-production-release-asset/1"));
    assert!(is_allowed_download_url("https://release-assets.githubusercontent.com/github-production-release-asset/1"));
    assert!(is_allowed_download_url("https://raw.githubusercontent.com/developedByAshu/juice-bar/main/signatures/v0.1.3.sig"));
    assert!(!is_allowed_download_url("https://github.com/someoneElse/juice-bar/releases/download/v9/JuiceBar-x64.exe"));
    assert!(!is_allowed_download_url("http://github.com/developedByAshu/juice-bar/x"));
    assert!(!is_allowed_download_url("https://github.com.evil.example/developedByAshu/x"));
    assert!(!is_allowed_download_url("https://github.com/developedByAshu/../someoneElse/x"));
    assert!(!is_allowed_download_url("https://raw.githubusercontent.com/someoneElse/x/main/signatures/v1.sig"));
}

#[test]
fn the_install_question_names_the_version() {
    assert_eq!(install_question("Juice Bar", "v0.2.0"), "Juice Bar 0.2.0 is ready. Install now?");
}
