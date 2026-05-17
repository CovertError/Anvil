//! Notifications. A notifiable can receive a notification via one or more channels.

use async_trait::async_trait;

use crate::container::Container;
use crate::mail::OutgoingMessage;
use crate::Error;

#[derive(Debug, Clone)]
pub enum Channel {
    Mail,
    Database,
    Slack,
}

#[async_trait]
pub trait Notification: Send + Sync {
    fn channels(&self) -> Vec<Channel>;

    async fn to_mail(&self, _container: &Container) -> Result<Option<OutgoingMessage>, Error> {
        Ok(None)
    }

    async fn to_database(&self, _container: &Container) -> Result<Option<serde_json::Value>, Error> {
        Ok(None)
    }

    async fn to_slack(&self, _container: &Container) -> Result<Option<SlackMessage>, Error> {
        Ok(None)
    }
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct SlackMessage {
    pub webhook: String,
    pub text: String,
}

pub async fn notify<N: Notification>(
    container: &Container,
    notification: &N,
) -> Result<(), Error> {
    for channel in notification.channels() {
        match channel {
            Channel::Mail => {
                if let Some(msg) = notification.to_mail(container).await? {
                    container.mailer().send(msg).await?;
                }
            }
            Channel::Database => {
                if let Some(payload) = notification.to_database(container).await? {
                    tracing::info!(?payload, "notification stored in database channel (POC stub)");
                }
            }
            Channel::Slack => {
                if let Some(slack) = notification.to_slack(container).await? {
                    tracing::info!(?slack, "notification posted to slack channel (POC stub)");
                }
            }
        }
    }
    Ok(())
}
