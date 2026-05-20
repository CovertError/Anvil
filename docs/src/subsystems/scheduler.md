# Scheduler

Same shape as Laravel's `app/Console/Kernel.php` `schedule()` method — a fluent Rust builder over cron expressions. Define scheduled tasks in `src/app/schedule.rs`; they run when you invoke `anvil schedule:run`, which is what you wire into system cron once a minute:

```cron
* * * * * cd /var/www/myapp && smith schedule:run >> /var/log/anvilforge.log 2>&1
```

## Define a task

```rust
use std::sync::Arc;
use anvilforge::async_trait::async_trait;
use anvilforge::schedule::{Schedule, ScheduledTask};

pub struct GenerateReports;

#[async_trait]
impl ScheduledTask for GenerateReports {
    async fn run(&self, c: &Container) -> Result<()> {
        // ... build the report ...
        Ok(())
    }

    fn description(&self) -> &str { "Generate daily reports" }
}

pub fn build() -> Schedule {
    let schedule = Schedule::new();
    schedule.daily_at("02:00", Arc::new(GenerateReports));
    schedule.hourly(Arc::new(PruneOldLogs));
    schedule.cron("*/15 * * * * *", Arc::new(SyncWithUpstream));  // every 15 sec
    schedule
}
```

## Cron syntax

The expression format is `sec min hour day month weekday`. The full reference is in the [`cron`](https://docs.rs/cron) crate.

| Shorthand                 | Equivalent          |
| ------------------------- | ------------------- |
| `daily_at("02:00", t)`    | `0 0 2 * * *`       |
| `hourly(t)`               | `0 0 * * * *`       |
| `cron("...", t)`          | (custom expression) |

[Next: broadcasting →](broadcasting.md)
