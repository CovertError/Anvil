//! Mail. SMTP via lettre, with an in-memory fake driver for tests.

use std::sync::Arc;

use async_trait::async_trait;
use lettre::message::header::ContentType;
use lettre::transport::smtp::authentication::Credentials;
use lettre::{AsyncSmtpTransport, AsyncTransport, Message, Tokio1Executor};
use parking_lot::Mutex;

use crate::config::MailConfig;
use crate::Error;

#[async_trait]
pub trait MailDriver: Send + Sync {
    async fn send(&self, message: OutgoingMessage) -> Result<(), Error>;
}

#[derive(Debug, Clone)]
pub struct OutgoingMessage {
    pub from: String,
    pub to: Vec<String>,
    pub cc: Vec<String>,
    pub bcc: Vec<String>,
    pub subject: String,
    pub html_body: Option<String>,
    pub text_body: Option<String>,
}

#[derive(Clone)]
pub struct MailerHandle {
    driver: Arc<dyn MailDriver>,
}

impl MailerHandle {
    pub fn new(driver: Arc<dyn MailDriver>) -> Self {
        Self { driver }
    }

    pub fn null() -> Self {
        Self {
            driver: Arc::new(NullMailDriver),
        }
    }

    pub fn fake() -> (Self, Arc<Mutex<Vec<OutgoingMessage>>>) {
        let outbox = Arc::new(Mutex::new(Vec::new()));
        let driver = FakeDriver {
            outbox: outbox.clone(),
        };
        (
            Self {
                driver: Arc::new(driver),
            },
            outbox,
        )
    }

    pub fn smtp(config: &MailConfig) -> Result<Self, Error> {
        let mut builder = AsyncSmtpTransport::<Tokio1Executor>::builder_dangerous(&config.host)
            .port(config.port);
        if !config.username.is_empty() {
            builder = builder.credentials(Credentials::new(
                config.username.clone(),
                config.password.clone(),
            ));
        }
        let transport = builder.build();
        Ok(Self {
            driver: Arc::new(SmtpDriver {
                transport,
                default_from: format!("{} <{}>", config.from_name, config.from_address),
            }),
        })
    }

    pub async fn send(&self, message: OutgoingMessage) -> Result<(), Error> {
        self.driver.send(message).await
    }
}

/// Trait for app-defined mailables (Laravel's `Mailable`).
#[async_trait]
pub trait Mailable: Send + Sync {
    async fn build(&self) -> Result<OutgoingMessage, Error>;
}

pub async fn to(addr: impl Into<String>, mailer: &MailerHandle, mailable: impl Mailable) -> Result<(), Error> {
    let mut msg = mailable.build().await?;
    msg.to.push(addr.into());
    mailer.send(msg).await
}

struct NullMailDriver;

#[async_trait]
impl MailDriver for NullMailDriver {
    async fn send(&self, message: OutgoingMessage) -> Result<(), Error> {
        tracing::debug!(?message, "null mail driver dropped message");
        Ok(())
    }
}

struct FakeDriver {
    outbox: Arc<Mutex<Vec<OutgoingMessage>>>,
}

#[async_trait]
impl MailDriver for FakeDriver {
    async fn send(&self, message: OutgoingMessage) -> Result<(), Error> {
        self.outbox.lock().push(message);
        Ok(())
    }
}

struct SmtpDriver {
    transport: AsyncSmtpTransport<Tokio1Executor>,
    default_from: String,
}

#[async_trait]
impl MailDriver for SmtpDriver {
    async fn send(&self, message: OutgoingMessage) -> Result<(), Error> {
        let from = if message.from.is_empty() {
            self.default_from.clone()
        } else {
            message.from.clone()
        };

        let mut builder = Message::builder()
            .from(from.parse().map_err(|e: lettre::address::AddressError| Error::Mail(e.to_string()))?);
        for to in &message.to {
            builder = builder.to(to.parse().map_err(|e: lettre::address::AddressError| Error::Mail(e.to_string()))?);
        }
        for cc in &message.cc {
            builder = builder.cc(cc.parse().map_err(|e: lettre::address::AddressError| Error::Mail(e.to_string()))?);
        }
        for bcc in &message.bcc {
            builder = builder.bcc(bcc.parse().map_err(|e: lettre::address::AddressError| Error::Mail(e.to_string()))?);
        }
        let builder = builder.subject(&message.subject);

        let msg = if let Some(html) = &message.html_body {
            builder
                .header(ContentType::TEXT_HTML)
                .body(html.clone())
                .map_err(|e| Error::Mail(e.to_string()))?
        } else if let Some(text) = &message.text_body {
            builder
                .header(ContentType::TEXT_PLAIN)
                .body(text.clone())
                .map_err(|e| Error::Mail(e.to_string()))?
        } else {
            return Err(Error::Mail("mail has no body".into()));
        };

        self.transport
            .send(msg)
            .await
            .map_err(|e| Error::Mail(e.to_string()))?;
        Ok(())
    }
}
