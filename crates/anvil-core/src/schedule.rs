//! Scheduler. Cron-style expressions matched per tick from `smith schedule:run`.

use std::str::FromStr;
use std::sync::Arc;

use async_trait::async_trait;
use chrono::Utc;
use cron::Schedule as Cron;
use parking_lot::Mutex;

use crate::container::Container;
use crate::Error;

#[async_trait]
pub trait ScheduledTask: Send + Sync {
    async fn run(&self, container: &Container) -> Result<(), Error>;
    fn description(&self) -> &str {
        "scheduled task"
    }
}

pub struct ScheduledEntry {
    pub expression: String,
    pub task: Arc<dyn ScheduledTask>,
}

#[derive(Default, Clone)]
pub struct Schedule {
    entries: Arc<Mutex<Vec<ScheduledEntry>>>,
}

impl Schedule {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push(&self, expression: impl Into<String>, task: Arc<dyn ScheduledTask>) {
        self.entries.lock().push(ScheduledEntry {
            expression: expression.into(),
            task,
        });
    }

    pub fn cron(&self, expression: &str, task: Arc<dyn ScheduledTask>) {
        self.push(expression, task);
    }

    pub fn daily_at(&self, hour_minute: &str, task: Arc<dyn ScheduledTask>) {
        let parts: Vec<&str> = hour_minute.split(':').collect();
        let (h, m) = match parts.as_slice() {
            [h, m] => (h.parse::<u32>().unwrap_or(0), m.parse::<u32>().unwrap_or(0)),
            _ => (0, 0),
        };
        self.push(format!("0 {m} {h} * * *"), task);
    }

    pub fn hourly(&self, task: Arc<dyn ScheduledTask>) {
        self.push("0 0 * * * *", task);
    }

    /// Run any tasks whose expression matches the current minute.
    pub async fn run_due(&self, container: &Container) -> Result<(), Error> {
        let entries: Vec<_> = self
            .entries
            .lock()
            .iter()
            .map(|e| (e.expression.clone(), e.task.clone()))
            .collect();
        let now = Utc::now();
        for (expr, task) in entries {
            let Ok(cron) = Cron::from_str(&expr) else {
                tracing::warn!(expr, "invalid cron expression, skipping");
                continue;
            };
            if let Some(next) = cron.upcoming(Utc).next() {
                // Run if the next upcoming time is within the next minute.
                if (next - now).num_seconds().abs() < 60 {
                    if let Err(e) = task.run(container).await {
                        tracing::error!(error = ?e, "scheduled task failed");
                    }
                }
            }
        }
        Ok(())
    }
}
