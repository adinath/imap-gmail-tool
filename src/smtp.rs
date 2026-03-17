use lettre::message::Mailbox;
use lettre::transport::smtp::authentication::Credentials;
use lettre::{Message, SmtpTransport, Transport};

use crate::error::Result;
use crate::imap::Config;

/// Send an email via Gmail's SMTP relay.
///
/// Uses STARTTLS on port 587 (the standard for Gmail SMTP with app passwords).
/// `cc` is optional — pass an empty slice if not needed.
pub fn send_email(
    config: &Config,
    to: &str,
    subject: &str,
    body: &str,
    cc: Option<&str>,
) -> Result<()> {
    // Parse the from address. lettre expects "Display Name <email>" or just "email".
    // We construct "address <address>" so Gmail shows the address as the sender name.
    let from: Mailbox = config.address.parse()?;
    let to_addr: Mailbox = to.parse()?;

    let mut builder = Message::builder()
        .from(from)
        .to(to_addr)
        .subject(subject);

    if let Some(cc_addr) = cc {
        let cc_mailbox: Mailbox = cc_addr.parse()?;
        builder = builder.cc(cc_mailbox);
    }

    let email = builder.body(body.to_string())?;

    // App password credentials — same password used for IMAP
    let creds = Credentials::new(config.address.clone(), config.app_password.clone());

    // STARTTLS on port 587 is Gmail's standard SMTP submission port
    let mailer = SmtpTransport::starttls_relay("smtp.gmail.com")?
        .credentials(creds)
        .build();

    mailer.send(&email)?;
    Ok(())
}
