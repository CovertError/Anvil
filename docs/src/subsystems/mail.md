# Mail & notifications

Laravel's `Mail::to(...)->send(new Mailable())` translated to Rust. SMTP
via [`lettre`](https://docs.rs/lettre) with the same configuration shape
(`MAIL_MAILER`, `MAIL_HOST`, `MAIL_PORT`, `MAIL_FROM_ADDRESS` in `.env`).
The notification system maps Laravel's multi-channel notifications
(mail + database + broadcast) onto a `Notify` trait.

## Sending mail

```rust
use anvilforge::mail::{MailerHandle, OutgoingMessage};

async fn send_invoice(c: &Container, user: &User) -> Result<()> {
    let msg = OutgoingMessage {
        from: String::new(),  // uses MAIL_FROM_ADDRESS
        to: vec![user.email.clone()],
        cc: vec![],
        bcc: vec![],
        subject: "Your invoice".into(),
        html_body: Some("<h1>Invoice</h1>".into()),
        text_body: None,
    };
    c.mailer().send(msg).await
}
```

## Drivers

| Driver | Configure                                        |
| ------ | ------------------------------------------------ |
| SMTP   | `MAIL_MAILER=smtp` + `MAIL_HOST`/`MAIL_PORT` etc |
| Null   | drops mail silently (useful for dev)             |
| Fake   | `MailerHandle::fake()` → assertion-style outbox  |

SES / Postmark / Resend drivers ship in v0.2. For development, the bundled `docker-compose.yml` includes [MailHog](https://github.com/mailhog/MailHog) at `localhost:1025` — set `MAIL_HOST=localhost MAIL_PORT=1025` and view sent mail at <http://localhost:8025>.

## Notifications

Notifications dispatch the same payload across multiple channels (mail, database, Slack):

```rust
use anvilforge::notification::{Channel, Notification, SlackMessage};
use anvilforge::async_trait::async_trait;

pub struct InvoicePaid {
    pub amount: i64,
}

#[async_trait]
impl Notification for InvoicePaid {
    fn channels(&self) -> Vec<Channel> {
        vec![Channel::Mail, Channel::Database, Channel::Slack]
    }

    async fn to_mail(&self, _c: &Container) -> Result<Option<OutgoingMessage>> {
        Ok(Some(OutgoingMessage {
            subject: format!("Payment received: ${}", self.amount / 100),
            // ...
            ..Default::default()
        }))
    }

    async fn to_slack(&self, _c: &Container) -> Result<Option<SlackMessage>> {
        Ok(Some(SlackMessage {
            webhook: std::env::var("SLACK_WEBHOOK").unwrap(),
            text: format!(":moneybag: ${} received", self.amount / 100),
        }))
    }
}

// Dispatch:
anvilforge::notification::notify(&c, &InvoicePaid { amount: 9900 }).await?;
```

[Next: cache →](cache.md)
