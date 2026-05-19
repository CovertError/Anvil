# Queues

Anvilforge queues are async, durable, retryable background jobs. The default driver is Postgres-backed (using `SELECT … FOR UPDATE SKIP LOCKED` for safe concurrent consumers).

## Define a job

```bash
smith make:job SendWelcomeEmail
```

```rust
use anvilforge::prelude::*;

#[derive(Debug, Serialize, Deserialize, Job)]
pub struct SendWelcomeEmail {
    pub user_id: i64,
}

impl SendWelcomeEmail {
    pub async fn handle(&self, c: &Container) -> Result<()> {
        let user = User::find(c.pool(), self.user_id).await?.ok_or(Error::NotFound)?;
        let msg = anvilforge::mail::OutgoingMessage {
            from: String::new(),  // uses MAIL_FROM_ADDRESS default
            to: vec![user.email],
            cc: vec![],
            bcc: vec![],
            subject: "Welcome to the app".into(),
            html_body: Some(format!("<h1>Welcome, {}!</h1>", user.name)),
            text_body: None,
        };
        c.mailer().send(msg).await?;
        Ok(())
    }
}
```

The `#[derive(Job)]` macro:
- Generates `SendWelcomeEmail::dispatch(self).await?` — the public API for pushing the job.
- Registers a runner with the framework's job registry via `inventory::submit!`.

## Dispatch

```rust
async fn register(
    State(c): State<Container>,
    payload: RegisterRequest,
) -> Result<Redirect> {
    // ... create the user ...

    SendWelcomeEmail { user_id: new_user.id }
        .dispatch()
        .await?;

    Ok(Redirect::to("/welcome"))
}
```

## Run the worker

In a separate process:

```bash
smith queue:work
```

The worker:
- Polls the configured queue every second.
- Pops one job at a time (via `SKIP LOCKED`).
- Calls the registered runner.
- On error: exponential backoff retry up to `max_attempts` (default 3), then writes to `failed_jobs`.
- Responds to `SIGTERM` by finishing the in-flight job before exiting.

For production: run multiple workers in parallel, each in its own container/process.

## Drivers

| Driver       | Configure with                            | Notes                                  |
| ------------ | ----------------------------------------- | -------------------------------------- |
| `database`   | `QUEUE_DRIVER=database` (default)         | Durable, ACID, simple ops              |
| `redis`      | `QUEUE_DRIVER=redis` + `REDIS_URL=...`    | Faster, no DB write per dispatch       |
| `in-memory`  | constructed manually in tests             | Lost on restart; useful for unit tests |
| `fake`       | `QueueHandle::fake()` returns the outbox  | For assertion-style queue tests        |

## Testing

```rust
use anvilforge::queue::QueueHandle;

#[tokio::test]
async fn dispatching_pushes_to_queue() {
    let (queue, outbox) = QueueHandle::fake();
    // ... build container with this queue ...

    SendWelcomeEmail { user_id: 1 }.dispatch_with(&container).await.unwrap();

    let pushed = outbox.lock();
    assert_eq!(pushed.len(), 1);
    assert_eq!(pushed[0].job_type, "SendWelcomeEmail");
}
```

[Next: mail & notifications →](mail.md)
