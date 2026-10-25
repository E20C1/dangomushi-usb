use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::error::Error;
use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Component, Path, PathBuf};

pub const OFFLINE_DOC_PACK_ID: &str = "dangomushi-core-docs";
pub const OFFLINE_DOC_PACK_FORMAT: u16 = 1;
pub const OFFLINE_DOC_MARKER: &str = ".dangomushi-offline-docs";
pub const MAX_OFFLINE_DOC_BYTES: usize = 4 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OfflineDocument {
    pub path: &'static str,
    pub content: &'static str,
}

const DOCUMENTS: &[OfflineDocument] = &[
    OfflineDocument { path: "README.md", content: include_str!("../README.md") },
    OfflineDocument { path: "docs/ARCHITECTURE.md", content: include_str!("../docs/ARCHITECTURE.md") },
    OfflineDocument { path: "docs/CLIPBOARD_HYGIENE.md", content: include_str!("../docs/CLIPBOARD_HYGIENE.md") },
    OfflineDocument { path: "docs/CREDENTIAL_MODEL.md", content: include_str!("../docs/CREDENTIAL_MODEL.md") },
    OfflineDocument { path: "docs/DEVICE_PAIRING.md", content: include_str!("../docs/DEVICE_PAIRING.md") },
    OfflineDocument { path: "docs/DEVICE_RECONNECTION.md", content: include_str!("../docs/DEVICE_RECONNECTION.md") },
    OfflineDocument { path: "docs/DEVICE_REVOCATION.md", content: include_str!("../docs/DEVICE_REVOCATION.md") },
    OfflineDocument { path: "docs/DIRECT_LAN_PEER_PATH.md", content: include_str!("../docs/DIRECT_LAN_PEER_PATH.md") },
    OfflineDocument { path: "docs/DOCUMENT_SEARCH.md", content: include_str!("../docs/DOCUMENT_SEARCH.md") },
    OfflineDocument { path: "docs/ENCRYPTED_PEER_TRANSFER.md", content: include_str!("../docs/ENCRYPTED_PEER_TRANSFER.md") },
    OfflineDocument { path: "docs/EXISTING_CREDENTIAL_INPUT.md", content: include_str!("../docs/EXISTING_CREDENTIAL_INPUT.md") },
    OfflineDocument { path: "docs/FILE_COMPARISON.md", content: include_str!("../docs/FILE_COMPARISON.md") },
    OfflineDocument { path: "docs/GENERATED_CREDENTIAL_INTEGRATION.md", content: include_str!("../docs/GENERATED_CREDENTIAL_INTEGRATION.md") },
    OfflineDocument { path: "docs/HASH_VERIFY.md", content: include_str!("../docs/HASH_VERIFY.md") },
    OfflineDocument { path: "docs/INTERNET_P2P_THREAT_REVIEW.md", content: include_str!("../docs/INTERNET_P2P_THREAT_REVIEW.md") },
    OfflineDocument { path: "docs/LAN_ENABLE_CONTROL.md", content: include_str!("../docs/LAN_ENABLE_CONTROL.md") },
    OfflineDocument { path: "docs/MOBILE_EDITOR_SESSION_HANDOFF.md", content: include_str!("../docs/MOBILE_EDITOR_SESSION_HANDOFF.md") },
    OfflineDocument { path: "docs/OFFLINE_DOCUMENTATION_PACKS.md", content: include_str!("../docs/OFFLINE_DOCUMENTATION_PACKS.md") },
    OfflineDocument { path: "docs/OFFLINE_RECONSTRUCTION.md", content: include_str!("../docs/OFFLINE_RECONSTRUCTION.md") },
    OfflineDocument { path: "docs/PASSWORD_GENERATOR_CORE.md", content: include_str!("../docs/PASSWORD_GENERATOR_CORE.md") },
    OfflineDocument { path: "docs/PERMISSION_SCOPES.md", content: include_str!("../docs/PERMISSION_SCOPES.md") },
    OfflineDocument { path: "docs/PORTABLE_RELEASE_BUNDLES.md", content: include_str!("../docs/PORTABLE_RELEASE_BUNDLES.md") },
    OfflineDocument { path: "docs/PUBLIC_NETWORK_SAFE_DEFAULT.md", content: include_str!("../docs/PUBLIC_NETWORK_SAFE_DEFAULT.md") },
    OfflineDocument { path: "docs/RECEIVED_FILE_INTEGRITY.md", content: include_str!("../docs/RECEIVED_FILE_INTEGRITY.md") },
    OfflineDocument { path: "docs/REPRODUCIBLE_DEPENDENCY_CACHE.md", content: include_str!("../docs/REPRODUCIBLE_DEPENDENCY_CACHE.md") },
    OfflineDocument { path: "docs/RECOVERY_SNAPSHOTS.md", content: include_str!("../docs/RECOVERY_SNAPSHOTS.md") },
    OfflineDocument { path: "docs/RESTORE_VERIFICATION.md", content: include_str!("../docs/RESTORE_VERIFICATION.md") },
    OfflineDocument { path: "docs/RECOVERY_CODES.md", content: include_str!("../docs/RECOVERY_CODES.md") },
    OfflineDocument { path: "docs/REPOSITORY_GIT.md", content: include_str!("../docs/REPOSITORY_GIT.md") },
    OfflineDocument { path: "docs/ROADMAP.md", content: include_str!("../docs/ROADMAP.md") },
    OfflineDocument { path: "docs/SECRET_BACKUP_RESTORE.md", content: include_str!("../docs/SECRET_BACKUP_RESTORE.md") },
    OfflineDocument { path: "docs/SECRET_DATA_BOUNDARY.md", content: include_str!("../docs/SECRET_DATA_BOUNDARY.md") },
    OfflineDocument { path: "docs/TEXT_JSON_UTILITIES.md", content: include_str!("../docs/TEXT_JSON_UTILITIES.md") },
    OfflineDocument { path: "docs/TRUSTED_DEVICES.md", content: include_str!("../docs/TRUSTED_DEVICES.md") },
    OfflineDocument { path: "docs/TRUSTED_PEER_IDENTITY.md", content: include_str!("../docs/TRUSTED_PEER_IDENTITY.md") },
    OfflineDocument { path: "docs/UI_LAYER_DECISION.md", content: include_str!("../docs/UI_LAYER_DECISION.md") },
    OfflineDocument { path: "docs/UI_SYSTEM.md", content: include_str!("../docs/UI_SYSTEM.md") },
    OfflineDocument { path: "docs/VAULT_STORAGE.md", content: include_str!("../docs/VAULT_STORAGE.md") },
    OfflineDocument { path: "docs/VAULT_UNLOCK.md", content: include_str!("../docs/VAULT_UNLOCK.md") },
    OfflineDocument { path: "docs/WORKSPACE_BACKUP_RESTORE.md", content: include_str!("../docs/WORKSPACE_BACKUP_RESTORE.md") },
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OfflineDocPackSummary {
    pub pack_id: &'static str,
    pub format_version: u16,
    pub app_version: &'static str,
    pub document_count: usize,
    pub total_bytes: u64,
    pub sha256: [u8; 32],
}

impl OfflineDocPackSummary {
    pub fn sha256_hex(&self) -> String {
        hex_lower(&self.sha256)
    }
}

#[derive(Debug)]
pub enum OfflineDocsError {
    Io(io::Error),
    InvalidEmbeddedPack(&'static str),
    DestinationExists,
    InvalidExport(&'static str),
    UnknownDocument,
}

impl fmt::Display for OfflineDocsError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "{error}"),
            Self::InvalidEmbeddedPack(reason) => {
                write!(formatter, "embedded documentation pack is invalid: {reason}")
            }
            Self::DestinationExists => {
                formatter.write_str("offline documentation destination already exists")
            }
            Self::InvalidExport(reason) => {
                write!(formatter, "offline documentation export is invalid: {reason}")
            }
            Self::UnknownDocument => formatter.write_str("offline document does not exist"),
        }
    }
}

impl Error for OfflineDocsError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            _ => None,
        }
    }
}

impl From<io::Error> for OfflineDocsError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

pub fn documents() -> &'static [OfflineDocument] {
    DOCUMENTS
}

pub fn document(path: &str) -> Result<&'static OfflineDocument, OfflineDocsError> {
    DOCUMENTS
        .iter()
        .find(|document| document.path == path)
        .ok_or(OfflineDocsError::UnknownDocument)
}

pub fn summary() -> Result<OfflineDocPackSummary, OfflineDocsError> {
    validate_embedded_pack()?;

    let mut hasher = Sha256::new();
    hasher.update(b"DANGOMUSHI-OFFLINE-DOC-PACK-V1\0");
    hasher.update(OFFLINE_DOC_PACK_FORMAT.to_be_bytes());

    let mut total_bytes = 0_u64;
    for document in DOCUMENTS {
        let path = document.path.as_bytes();
        let content = document.content.as_bytes();
        total_bytes = total_bytes
            .checked_add(content.len() as u64)
            .ok_or(OfflineDocsError::InvalidEmbeddedPack("total size overflow"))?;

        hasher.update((path.len() as u32).to_be_bytes());
        hasher.update(path);
        hasher.update((content.len() as u64).to_be_bytes());
        hasher.update(Sha256::digest(content));
    }

    let sha256: [u8; 32] = hasher.finalize().into();
    Ok(OfflineDocPackSummary {
        pack_id: OFFLINE_DOC_PACK_ID,
        format_version: OFFLINE_DOC_PACK_FORMAT,
        app_version: env!("CARGO_PKG_VERSION"),
        document_count: DOCUMENTS.len(),
        total_bytes,
        sha256,
    })
}

pub fn export(destination: &Path) -> Result<OfflineDocPackSummary, OfflineDocsError> {
    let summary = summary()?;

    match fs::create_dir(destination) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
            return Err(OfflineDocsError::DestinationExists)
        }
        Err(error) => return Err(OfflineDocsError::Io(error)),
    }

    let result = export_into_new_directory(destination, &summary);
    if result.is_err() {
        let _ = fs::remove_dir_all(destination);
    }
    result?;
    verify_export(destination)?;
    Ok(summary)
}

pub fn verify_export(destination: &Path) -> Result<OfflineDocPackSummary, OfflineDocsError> {
    let root_metadata = fs::symlink_metadata(destination)?;
    if !root_metadata.is_dir() || root_metadata.file_type().is_symlink() {
        return Err(OfflineDocsError::InvalidExport(
            "pack root must be a real directory",
        ));
    }

    let expected_summary = summary()?;
    let expected_marker = marker_text(&expected_summary);
    let marker_path = destination.join(OFFLINE_DOC_MARKER);
    let marker_metadata = fs::symlink_metadata(&marker_path)
        .map_err(|_| OfflineDocsError::InvalidExport("marker is missing"))?;
    if !marker_metadata.is_file() || marker_metadata.file_type().is_symlink() {
        return Err(OfflineDocsError::InvalidExport(
            "marker must be a regular file",
        ));
    }
    let marker = read_bounded(&marker_path, expected_marker.len())?;
    if marker != expected_marker.as_bytes() {
        return Err(OfflineDocsError::InvalidExport("marker mismatch"));
    }

    for document in DOCUMENTS {
        let path = destination.join(document.path);
        let metadata = fs::symlink_metadata(&path)
            .map_err(|_| OfflineDocsError::InvalidExport("document is missing"))?;
        if !metadata.is_file() || metadata.file_type().is_symlink() {
            return Err(OfflineDocsError::InvalidExport(
                "document must be a regular file",
            ));
        }
        let bytes = read_bounded(&path, document.content.len())?;
        if bytes != document.content.as_bytes() {
            return Err(OfflineDocsError::InvalidExport(
                "document content mismatch",
            ));
        }
    }

    let actual_files = collect_export_files(destination)?;
    let mut expected_files: BTreeSet<String> =
        DOCUMENTS.iter().map(|document| document.path.to_string()).collect();
    expected_files.insert(OFFLINE_DOC_MARKER.to_string());

    if actual_files != expected_files {
        return Err(OfflineDocsError::InvalidExport(
            "unexpected or missing export entries",
        ));
    }

    Ok(expected_summary)
}

fn validate_embedded_pack() -> Result<(), OfflineDocsError> {
    if DOCUMENTS.is_empty() {
        return Err(OfflineDocsError::InvalidEmbeddedPack("no documents"));
    }

    let mut paths = BTreeSet::new();
    for document in DOCUMENTS {
        validate_relative_document_path(document.path)?;
        if document.content.is_empty() {
            return Err(OfflineDocsError::InvalidEmbeddedPack(
                "empty document content",
            ));
        }
        if document.content.len() > MAX_OFFLINE_DOC_BYTES {
            return Err(OfflineDocsError::InvalidEmbeddedPack(
                "document exceeds size limit",
            ));
        }
        if !paths.insert(document.path) {
            return Err(OfflineDocsError::InvalidEmbeddedPack(
                "duplicate document path",
            ));
        }
    }

    Ok(())
}

fn validate_relative_document_path(path: &str) -> Result<(), OfflineDocsError> {
    if path.is_empty() || path.contains('\\') {
        return Err(OfflineDocsError::InvalidEmbeddedPack(
            "invalid document path",
        ));
    }

    let parsed = Path::new(path);
    if parsed.is_absolute()
        || !parsed
            .components()
            .all(|component| matches!(component, Component::Normal(_)))
    {
        return Err(OfflineDocsError::InvalidEmbeddedPack(
            "document path is not relative-normal",
        ));
    }

    Ok(())
}

fn export_into_new_directory(
    destination: &Path,
    summary: &OfflineDocPackSummary,
) -> Result<(), OfflineDocsError> {
    for document in DOCUMENTS {
        let output = destination.join(document.path);
        if let Some(parent) = output.parent() {
            fs::create_dir_all(parent)?;
        }

        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&output)?;
        file.write_all(document.content.as_bytes())?;
        file.sync_all()?;
    }

    let marker_path = destination.join(OFFLINE_DOC_MARKER);
    let mut marker = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(marker_path)?;
    marker.write_all(marker_text(summary).as_bytes())?;
    marker.sync_all()?;
    Ok(())
}

fn marker_text(summary: &OfflineDocPackSummary) -> String {
    format!(
        "format={}\npack={}\napp_version={}\ndocument_count={}\ntotal_bytes={}\nsha256={}\n",
        summary.format_version,
        summary.pack_id,
        summary.app_version,
        summary.document_count,
        summary.total_bytes,
        summary.sha256_hex(),
    )
}

fn read_bounded(path: &Path, expected_len: usize) -> Result<Vec<u8>, OfflineDocsError> {
    let limit = expected_len
        .checked_add(1)
        .ok_or(OfflineDocsError::InvalidExport("size overflow"))?;
    let mut file = File::open(path)?;
    let mut bytes = Vec::with_capacity(expected_len);
    file.take(limit as u64).read_to_end(&mut bytes)?;
    if bytes.len() != expected_len {
        return Err(OfflineDocsError::InvalidExport("file length mismatch"));
    }
    Ok(bytes)
}

fn collect_export_files(root: &Path) -> Result<BTreeSet<String>, OfflineDocsError> {
    let mut files = BTreeSet::new();
    collect_export_files_recursive(root, root, &mut files)?;
    Ok(files)
}

fn collect_export_files_recursive(
    root: &Path,
    current: &Path,
    files: &mut BTreeSet<String>,
) -> Result<(), OfflineDocsError> {
    for entry in fs::read_dir(current)? {
        let entry = entry?;
        let path = entry.path();
        let metadata = fs::symlink_metadata(&path)?;
        if metadata.file_type().is_symlink() {
            return Err(OfflineDocsError::InvalidExport(
                "symlink entries are not allowed",
            ));
        }
        if metadata.is_dir() {
            collect_export_files_recursive(root, &path, files)?;
            continue;
        }
        if !metadata.is_file() {
            return Err(OfflineDocsError::InvalidExport(
                "special entries are not allowed",
            ));
        }

        let relative = path
            .strip_prefix(root)
            .map_err(|_| OfflineDocsError::InvalidExport("entry escaped pack root"))?;
        let mut components = Vec::new();
        for component in relative.components() {
            let Component::Normal(component) = component else {
                return Err(OfflineDocsError::InvalidExport("invalid export path"));
            };
            let text = component
                .to_str()
                .ok_or(OfflineDocsError::InvalidExport("non-UTF-8 export path"))?;
            components.push(text);
        }
        files.insert(components.join("/"));
    }
    Ok(())
}

fn hex_lower(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(HEX[(byte >> 4) as usize] as char);
        output.push(HEX[(byte & 0x0f) as usize] as char);
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temporary_path(label: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        std::env::temp_dir().join(format!(
            "dangomushi-offline-docs-{label}-{}-{unique}",
            std::process::id()
        ))
    }

    #[test]
    fn embedded_pack_is_valid_and_contains_core_docs() {
        let summary = summary().expect("summary");
        assert!(summary.document_count >= 30);
        assert!(summary.total_bytes > 100_000);
        assert_eq!(summary.sha256_hex().len(), 64);
        assert!(document("README.md").is_ok());
        assert!(document("docs/ARCHITECTURE.md").is_ok());
        assert!(document("docs/ROADMAP.md").is_ok());
    }

    #[test]
    fn export_is_create_new_and_round_trips_exactly() {
        let destination = temporary_path("roundtrip");
        let exported = export(&destination).expect("export");
        let verified = verify_export(&destination).expect("verify");
        assert_eq!(exported, verified);
        assert!(matches!(
            export(&destination),
            Err(OfflineDocsError::DestinationExists)
        ));
        fs::remove_dir_all(destination).expect("cleanup");
    }

    #[test]
    fn modified_export_fails_verification() {
        let destination = temporary_path("tamper");
        export(&destination).expect("export");
        fs::write(destination.join("README.md"), b"modified").expect("modify");
        assert!(matches!(
            verify_export(&destination),
            Err(OfflineDocsError::InvalidExport(_))
        ));
        fs::remove_dir_all(destination).expect("cleanup");
    }
}
