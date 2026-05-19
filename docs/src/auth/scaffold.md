# `smith make:auth`

Generates a complete login/register/logout scaffold — Anvilforge's equivalent of Laravel Breeze.

```bash
smith make:auth
```

This creates:

```
src/app/controllers_auth.rs    ← AuthController with login/register/logout
src/app/requests_auth.rs       ← LoginRequest, RegisterRequest
src/routes/auth.rs             ← /login, /register, /logout routes
resources/views/auth/login.forge.html
resources/views/auth/register.forge.html
database/migrations/<ts>_add_auth_columns_to_users.rs
```

Then a few manual steps (these become automatic in v0.2):

1. Add `mod controllers_auth;` and `mod requests_auth;` to `src/app/mod.rs`.
2. Add `pub mod auth;` to `src/routes/mod.rs`.
3. Wire `routes::auth::register` into `bootstrap/app.rs`:

   ```rust
   .web(|r| {
       routes::web::register(r);
       routes::auth::register(r)
   })
   ```

4. Run `smith migrate` to add the auth columns.

Then visit `/login` or `/register`.

## What you get

- `POST /login`: verifies via `auth::attempt::<User>`, persists in session via `auth::login`.
- `POST /register`: hashes the password via `auth::hash_password`, inserts the user, logs them in.
- `POST /logout`: clears the session.

Forms are styled minimally — change `resources/views/auth/*.forge.html` to match your design system.

[Next: queues →](../subsystems/queues.md)
