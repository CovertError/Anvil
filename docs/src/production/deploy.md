# Deploying

Anvilforge compiles to a single static binary. Deployment is dramatically simpler than Laravel — no PHP-FPM, no opcache, no separate worker reboots.

## Build the release binary

```bash
cargo build --release --bin myapp
# → target/release/myapp
```

The binary is fully statically linked (against musl if you cross-compile) and includes the Forge-compiled templates baked in at build time.

## Run it

```bash
./myapp serve              # the HTTP server
./myapp queue:work         # in a separate process
./myapp schedule:run       # call from cron once a minute
```

A typical SystemD unit:

```ini
[Unit]
Description=Anvilforge app
After=network.target

[Service]
Type=simple
User=anvilforge
WorkingDirectory=/var/www/myapp
EnvironmentFile=/var/www/myapp/.env
ExecStart=/var/www/myapp/myapp serve
Restart=on-failure
RestartSec=5s

[Install]
WantedBy=multi-user.target
```

## Behind a reverse proxy

Anvilforge listens on a plain TCP socket — terminate TLS upstream:

```nginx
server {
    listen 443 ssl;
    server_name example.com;
    ssl_certificate     /etc/letsencrypt/live/example.com/fullchain.pem;
    ssl_certificate_key /etc/letsencrypt/live/example.com/privkey.pem;

    location / {
        proxy_pass http://127.0.0.1:8080;
        proxy_set_header Host $host;
        proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;
        proxy_set_header X-Forwarded-Proto $scheme;
    }

    location /static/ {
        alias /var/www/myapp/public/build/;
        expires 1y;
    }
}
```

## Docker

```dockerfile
FROM rust:1.85 AS build
WORKDIR /src
COPY . .
RUN cargo build --release --bin myapp

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates && rm -rf /var/lib/apt/lists/*
COPY --from=build /src/target/release/myapp /app/myapp
COPY --from=build /src/public /app/public
COPY --from=build /src/resources /app/resources
WORKDIR /app
EXPOSE 8080
CMD ["./myapp", "serve"]
```

For musl-static builds (smaller image, simpler distroless deploy), use `clux/muslrust` or `cross`.

## Graceful shutdown

Anvilforge handles `SIGTERM` and `SIGINT` correctly: it stops accepting new connections, drains in-flight requests with a 30-second timeout, flushes queue worker mid-job state, then exits. SystemD's default `SIGTERM` + `RestartSec=5s` will give you safe rolling restarts.

[Next: observability →](observability.md)
