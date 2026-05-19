# Your first app

## Scaffold

```bash
smith new blog
cd blog
```

This creates a complete project with all directories, a starter `User` model, a `users` migration, web/api routes, Forge templates, and a Vite config. Same shape as `laravel new`.

## Configure

```bash
cp .env.example .env
```

Edit `.env` to point at your Postgres:

```env
DATABASE_URL=postgres://postgres:postgres@localhost:5432/blog
```

If you don't have Postgres running, the workspace ships with a `docker-compose.yml`:

```bash
docker-compose up -d
```

## Migrate

```bash
smith migrate
```

You should see:

```
migrated: 2026_01_01_000001_create_users_table
migrated: 2026_01_01_000002_create_jobs_table
```

## Serve

```bash
smith serve
```

Visit <http://localhost:8080>. You'll see the Anvilforge welcome page.

For auto-reload during development:

```bash
smith serve --watch
```

This uses `cargo-watch` to rebuild on changes to `src/`, `routes/`, `resources/views/`, and `config/`. Install once with `cargo install cargo-watch`.

[Next: project structure →](structure.md)
