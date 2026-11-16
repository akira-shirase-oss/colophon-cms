//! The asset's lifecycle and the bounds an upload intent is checked against.

/// The most an upload may weigh (07-media.md: approved images up to 20 MiB in v0.1).
pub const MAX_BYTES: u64 = 20 * 1024 * 1024;

/// The declared types v0.1 accepts: still images only. The signature check decides what the
/// bytes really are; a declaration outside this list is refused before the object exists.
pub const DECLARED_TYPES: [&str; 3] = ["image/png", "image/jpeg", "image/webp"];

/// Where an asset stands: quarantined while the worker decides, then exactly one end state —
/// `usable`, `rejected`, or, after a delete request, `deleted` by way of `deleting` (media M3,
/// D-098).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    /// Uploaded; the verify job is due or running. Never referenced by published content.
    Quarantined,
    /// Signature checked, decode bounded, metadata stripped; the public key may be served.
    Usable,
    /// Refused by verification; `rejected_reason` says why in words safe to show.
    Rejected,
    /// A delete job committed its mark and is removing the objects outside any transaction.
    Deleting,
    /// Every object is gone. The row is a tombstone: it is never removed and never serves.
    Deleted,
}

/// A checked upload intent: a name to show, a type the client claims, a size it promises.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Intent {
    /// The original file name, for lists and dialogs; never a storage key.
    pub file_name: String,
    /// The claimed MIME type — a hint for the picker, never trusted for safety (M2).
    pub declared_type: String,
    /// The promised byte count.
    pub size: u64,
}

/// Why an intent was refused.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum IntentError {
    /// The file name was empty or past the record's bound.
    #[error("a file name is 1 to 200 characters")]
    FileName,
    /// The declared type is not one v0.1 accepts.
    #[error("v0.1 accepts PNG, JPEG and WebP images")]
    FileType,
    /// The promised size is zero or past the 20 MiB bound.
    #[error("an upload is 1 byte to 20 MiB")]
    FileSize,
}

impl Intent {
    /// Checks an intent's three fields against their bounds; refuses without an object existing.
    pub fn new(file_name: &str, declared_type: &str, size: u64) -> Result<Self, IntentError> {
        let file_name = file_name.trim();
        if file_name.is_empty() || file_name.chars().count() > 200 {
            return Err(IntentError::FileName);
        }
        if !DECLARED_TYPES.contains(&declared_type) {
            return Err(IntentError::FileType);
        }
        if size == 0 || size > MAX_BYTES {
            return Err(IntentError::FileSize);
        }
        Ok(Self {
            file_name: file_name.to_owned(),
            declared_type: declared_type.to_owned(),
            size,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn intent_bounds() {
        assert!(Intent::new("logo.png", "image/png", 12).is_ok());
        assert_eq!(Intent::new("", "image/png", 12), Err(IntentError::FileName));
        assert_eq!(
            Intent::new(&"x".repeat(201), "image/png", 12),
            Err(IntentError::FileName)
        );
        assert_eq!(
            Intent::new("x.svg", "image/svg+xml", 12),
            Err(IntentError::FileType)
        );
        assert_eq!(
            Intent::new("x.png", "image/png", 0),
            Err(IntentError::FileSize)
        );
        assert_eq!(
            Intent::new("x.png", "image/png", MAX_BYTES + 1),
            Err(IntentError::FileSize)
        );
    }
}
