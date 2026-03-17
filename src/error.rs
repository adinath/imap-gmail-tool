use thiserror::Error;

#[derive(Error, Debug)]
pub enum GmailError {
    // IMAP errors — connection, auth, protocol
    #[error("IMAP error: {0}")]
    Imap(#[from] imap::Error),

    // TLS handshake failures
    #[error("TLS error: {0}")]
    Tls(#[from] native_tls::Error),

    // SMTP send failures
    #[error("SMTP error: {0}")]
    Smtp(#[from] lettre::transport::smtp::Error),

    // Invalid email addresses passed to lettre
    #[error("Invalid email address: {0}")]
    Address(#[from] lettre::address::AddressError),

    // lettre message builder errors (e.g. missing required fields)
    #[error("Failed to build email message: {0}")]
    MessageBuild(#[from] lettre::error::Error),

    // mailparse can't decode the raw RFC822 bytes
    #[error("Failed to parse email: {0}")]
    MailParse(#[from] mailparse::MailParseError),

    // Caller passed a UID that doesn't exist in the mailbox
    #[error("Email not found: uid={0} in folder '{1}'")]
    NotFound(u32, String),

    // The requested IMAP folder doesn't exist
    #[error("Folder not found: '{0}'")]
    FolderNotFound(String),
}

// Convenience alias used throughout the codebase
pub type Result<T> = std::result::Result<T, GmailError>;
