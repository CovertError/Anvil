# Coming from Laravel?

This page is the one-screen translation between Laravel idioms and their
Anvilforge equivalents. Keep it open in a tab while you build your first
feature; almost everything you'd reach for in Laravel maps directly to
something here.

The rule of thumb: **same shape, different language**. Anvilforge
deliberately copies Laravel's directory layout, command names, and
mental model. The differences are where Rust's type system disagreed
with PHP's — explicit error returns, struct-shaped requests, derive
macros instead of base-class magic.

## Installation & quickstart

| Laravel | Anvilforge |
|---|---|
| `laravel new my-app` | `anvil new my-app` |
| `php artisan serve` | `anvil serve` |
| `php artisan migrate` | `anvil migrate` |
| `php artisan migrate:fresh --seed` | `anvil migrate:fresh --seed` |
| `php artisan db:seed` | `anvil db:seed` |
| `php artisan queue:work` | `anvil queue:work` |
| `php artisan schedule:run` | `anvil schedule:run` |
| `php artisan test` | `anvil test` |
| `php artisan tinker` | `anvil repl` |
| `php artisan route:list` | `anvil routes` |

## Scaffolding (`make:*`)

Every command you'd run is the same name, just `anvil` instead of
`php artisan`.

| Laravel | Anvilforge |
|---|---|
| `make:model Post --migration` | `make:model Post --with-migration` |
| `make:migration add_published_to_posts` | `make:migration add_published_to_posts` |
| `make:controller PostController --resource` | `make:controller PostController --resource` |
| `make:request StorePostRequest` | `make:request StorePostRequest` |
| `make:job SendWelcomeEmail` | `make:job SendWelcomeEmail` |
| `make:event UserRegistered` | `make:event UserRegistered` |
| `make:listener SendWelcomeEmail --event=UserRegistered` | `make:listener SendWelcomeEmail --event=UserRegistered` |
| `make:test PostCreationTest` | `make:test post_creation` |
| `make:seeder UserSeeder` | `make:seeder UserSeeder` |
| `make:factory UserFactory --model=User` | `make:factory UserFactory --model=User` |
| (no equivalent in Laravel) | `make:component Counter` — Spark component |
| `make:auth` (Breeze) | `make:auth` — login + register + logout scaffold |

## Routes

| Laravel (`routes/web.php`) | Anvilforge (`routes/web.rs`) |
|---|---|
| `Route::get('/', fn() => view('welcome'));` | `r.get("/", HomeController::index)` |
| `Route::resource('posts', PostController::class);` | The seven RESTful routes registered explicitly: `r.get("/posts", PostController::index).post("/posts", PostController::store).get("/posts/:id", PostController::show)…`. Generate the stubs with `anvil make:controller PostController --resource`. |
| `Route::middleware('auth')->group(...)` | `.layer(auth())` on a sub-router |
| `Route::prefix('admin')->group(...)` | `.nest("/admin", ...)` |
| Named routes: `Route::get(...)->name('posts.show')` | Plain function references — name them in Rust |

## ORM — Eloquent → Cast

| Laravel | Anvilforge |
|---|---|
| `class Post extends Model {}` | `#[derive(Model)] #[table("posts")] struct Post { id: i64, ... }` |
| `Post::all()` | `Post::query().get(&c).await?` |
| `Post::find($id)` | `Post::find(&c, id).await?` |
| `Post::findOrFail($id)` | `Post::find_or_fail(&c, id).await?` |
| `Post::where('published', true)->get()` | `Post::query().where_eq("published", true).get(&c).await?` |
| `Post::create([...])` | `Post::create(c.pool(), Post { ... }).await?` (or `instance.insert(c.pool()).await?`) |
| `$post->update([...])` | `post.update(&c, attrs).await?` |
| `$post->delete()` | `post.delete(&c).await?` |
| `$post->save()` | `post.save(&c).await?` |
| `$user->posts` (`hasMany`) | `#[has_many(Post)]` on `User` |
| `$post->user` (`belongsTo`) | `#[belongs_to(User)]` on `Post` |
| Soft deletes via `SoftDeletes` trait | `#[soft_deletes]` on the model |

## Migrations

| Laravel | Anvilforge |
|---|---|
| `Schema::create('posts', fn (Blueprint $t) => ...)` | `s.create("posts", \|t\| { ... })` inside `up()` (or use the `migration!` macro for the whole struct in 6 lines) |
| `$t->id()` | `t.id()` |
| `$t->string('title')` | `t.string("title")` |
| `$t->text('body')->nullable()` | `t.text("body").nullable()` |
| `$t->timestamps()` | `t.timestamps()` |
| `$t->softDeletes()` | `t.soft_deletes()` |
| `$t->foreignId('user_id')->constrained()` | `t.foreign("user_id").references("id").on("users")` |
| `Schema::dropIfExists('posts')` | `s.drop_if_exists("posts")` (in `down()`) |

## Validation — Form Requests

| Laravel | Anvilforge |
|---|---|
| `class StorePostRequest extends FormRequest { public function rules() { return ['title' => 'required\|string\|max:255']; } }` | `#[derive(FormRequest)] struct StorePostRequest { #[garde(length(min=1, max=255))] title: String, }` |
| `$req->validated()` | Comes pre-deserialized as a struct extractor |
| `abort(422)` | `Err(Error::Validation(...))?` (auto-maps to 422) |

## Templates — Blade → Forge

Forge is Blade for Rust. The directive set is intentionally the same:

| Blade | Forge |
|---|---|
| `@extends('layouts.app')` | `@extends("layouts.app")` |
| `@section('content') ... @endsection` | `@section("content") ... @endsection` |
| `@yield('content')` | `@yield("content")` |
| `@if($x) ... @endif` | `@if(x) ... @endif` |
| `@foreach($items as $i)` | `@foreach(items as i)` |
| `{{ $post->title }}` | `{{ post.title }}` |
| `{!! $html !!}` | `{!! html !!}` |
| `@csrf` | `@csrf` |
| `@vite(['resources/css/app.css'])` | `@vite(["resources/css/app.css"])` |

## Reactive components — Livewire → Spark

| Livewire | Spark |
|---|---|
| `class Counter extends Component { public $count = 0; }` | `#[spark_component(template = "spark/counter")] pub struct Counter { pub count: i32 }` |
| `public function increment()` | `#[spark_actions] impl Counter { async fn increment(&mut self) -> Result<()> { ... } }` |
| `wire:click="increment"` | `spark:click="increment"` |
| `wire:model="title"` | `spark:model="title"` (mark field with `#[spark(model)]`) |
| `@livewire('counter')` | `@spark("counter")` |
| `protected $listeners = ['posts.created' => 'refresh']` | `#[spark_on("posts.created")]` |

Architecture difference worth knowing: Spark snapshots live in the DOM
as HMAC-signed envelopes, not on the server. No session affinity, no
per-component memory between requests. Full write-up in
[Spark — reactive components](../subsystems/spark.md).

## Broadcasting — Pusher/Reverb → Bellows

| Laravel | Anvilforge |
|---|---|
| `broadcast(new OrderShipped($order))` | `bellows::broadcast(&c.bellows(), OrderShipped { order })` |
| Echo client | Laravel Echo works unchanged — Bellows speaks the Pusher protocol |
| `php artisan reverb:start` | `anvil serve` already exposes `/bellows/connect` |

## Mail, notifications, events, jobs

| Laravel | Anvilforge |
|---|---|
| `Mail::to($u)->send(new OrderShipped($order))` | `c.mailer().send(OutgoingMessage::new().to(&u.email).subject("…").view("mail.order_shipped", json!({...}))).await?` |
| `Notification::send($users, new Invoice($inv))` | `anvilforge::notification::notify(&c, &Invoice { inv }).await?` (one user; loop for many) |
| `event(new OrderShipped($order))` | `c.events().dispatch(OrderShipped { order })` |
| `OrderShipped::dispatch($order)` (queued job) | `anvilforge::queue::dispatch_payload(&c, "OrderShipped", json!({ "order_id": order.id })).await?` |

## Errors & responses

| Laravel | Anvilforge |
|---|---|
| `abort(404)` | `Err(Error::NotFound)?` |
| `abort(403)` | `Err(Error::Forbidden("...".into()))?` |
| `throw ValidationException::withMessages([...])` | `Err(Error::Validation(errs))?` |
| `return response()->json($data)` | `Ok(Json(data))` |
| `return view('posts.show', ['post' => $p])` | `Ok(view("posts.show", json!({"post": p})))` |
| `redirect('/posts')` | `Ok(redirect("/posts"))` |
| `back()->withInput()->withErrors($e)` | `Ok(redirect_back(req).with_errors(e))` |

## Container / service container

| Laravel | Anvilforge |
|---|---|
| `app(SomeService::class)` | `c.resolve::<SomeService>()` |
| `App::bind(...)` | `c.bind(SomeService::new())` |
| `$this->app->make(...)` | `c.resolve::<T>()` |
| Constructor injection | `State<Container>` extractor + `c.resolve::<T>()` |

## Config & env

| Laravel | Anvilforge |
|---|---|
| `.env` | `.env` (same file, parsed via [`dotenvy`](https://docs.rs/dotenvy)) |
| `config('mail.from.address')` | `c.app().mail_from_address` (typed structs in `config/`) |
| `config:cache` | Not needed — typed config is compiled into the binary |

## Caching

| Laravel | Anvilforge |
|---|---|
| `Cache::get('key')` | `c.cache().get("key").await` |
| `Cache::put('key', $v, $ttl)` | `c.cache().put("key", v, ttl).await` |
| `Cache::remember('key', $ttl, fn() => ...)` | `c.cache().remember("key", ttl, \|\| async { ... }).await` |
| `Cache::forget('key')` | `c.cache().forget("key").await` |
| Redis driver | Set `CACHE_DRIVER=redis` + `REDIS_URL` |
| File driver | Set `CACHE_DRIVER=moka` (in-process; for multi-process use Redis) |

## Auth

| Laravel | Anvilforge |
|---|---|
| `Auth::user()` | Use the `Auth<User>` extractor on a handler — `async fn show(Auth(user): Auth<User>) { ... }`. Optional form: `OptionalAuth<User>`. |
| `Auth::check()` | The presence of `Auth<User>` in the extractor list implies the user is logged in; `OptionalAuth<User>` distinguishes signed-in from anon. |
| `Auth::attempt($credentials)` | `anvilforge::auth::attempt::<User>(&c, &session, &email, &password).await?` |
| `Auth::login($user)` | `anvilforge::auth::login(&session, &user).await?` |
| `Auth::logout()` | `anvilforge::auth::logout(&session).await?` |
| Policies | A plain `struct PostPolicy;` with static `fn view/create/update/delete(user, resource) -> bool`. Call the predicate from the controller: `if !PostPolicy::update(&user, &post) { return Err(Error::forbidden("not yours")); }`. Scaffold with `anvil make:policy PostPolicy --model=Post`. |
| Gates | Plain functions or a struct with named predicates — there's no central `Gate` registry yet; call the predicate where you need it. |

## Testing — PHPUnit/Pest → Assay

```rust
use anvilforge::assay::*;

#[tokio::test]
async fn root_returns_welcome() {
    let client = TestClient::new(app).await;
    client.get("/").await
        .assert_ok()
        .assert_see("Welcome");
}

expect(2 + 2).to_be(4);
expect("hello world").to_contain("world");

dataset!(squares, [
    one => (1, 1),
    two => (2, 4),
], |(n, sq)| { expect(n * n).to_be(sq); });
```

Pest's `expect()`, `dataset!`, fluent HTTP assertions — same shape,
Rust types.

## What's different on purpose

A short list of places where Anvilforge intentionally diverges from
Laravel and the rationale:

- **Explicit `Result<_, Error>` in handlers.** No magic exception
  bubbling — every fallible call needs a `?`. The framework provides a
  single `Error` enum that maps to the right HTTP status, so it's not
  *more* code than Laravel's `abort()` calls, just more visible.
- **Cast `Post::create(&c, ...)` takes the container.** No facade-style
  ambient state — the DB pool is passed explicitly. Optional
  facade-style helpers (`db()`, `cache()`) exist for handlers that want
  the Laravel feel.
- **`#[derive(FormRequest)]` instead of `extends FormRequest`.**
  Validation is compile-time; rules live as attributes on struct
  fields. Same coverage as Laravel's rules, just spelled differently.
- **Templates compile-checked.** Forge templates are validated against
  the controller's typed view data — a typo in `{{ post.titl }}` is a
  compile error, not a runtime null.

## Anything missing?

If a Laravel idiom you use every day isn't on this page, [open an
issue](https://github.com/anvilforge/anvilforge/issues) — this page is
meant to be exhaustive for the 90% of Laravel apps that don't reach for
obscure macros.
