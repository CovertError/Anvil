# Laravel Octane (Swoole) reference image for same-box A/B benchmarking
# against the `anvilforge-bench` container. Implements one route — `/`
# returning a tiny JSON — and runs under Octane's Swoole worker mode.
#
# Build:
#   docker build -f tools/http-bench/octane/Dockerfile \
#                -t laravel-octane-bench tools/http-bench/octane
#
# Run alongside Anvil for an apples-to-apples wrk/oha shootout: use the
# compose file at tools/http-bench/octane/docker-compose.yml.

FROM php:8.3-cli-alpine

# Swoole needs build deps; once installed we strip the dev set to keep
# the image lean.
RUN apk add --no-cache --virtual .build-deps \
        $PHPIZE_DEPS \
        linux-headers \
        openssl-dev \
        curl-dev \
        libpng-dev \
        zlib-dev \
 && apk add --no-cache curl libpng zlib openssl \
 && pecl install swoole \
 && docker-php-ext-enable swoole \
 && docker-php-ext-install pdo pdo_mysql \
 && apk del .build-deps \
 && rm -rf /tmp/* /var/cache/apk/*

# Composer for one-shot install — we don't keep it around.
COPY --from=composer:2 /usr/bin/composer /usr/bin/composer

WORKDIR /app

# Minimal Laravel skeleton + Octane. Pinning to the LTS major to avoid
# Composer wandering off into a breaking version mid-CI.
RUN composer create-project --prefer-dist --no-interaction laravel/laravel . "^11" \
 && composer require --no-interaction laravel/octane spiral/roadrunner-cli=^2 \
 && rm -rf .git /root/.composer/cache

# Drop in our tiny route so the comparison is on actual Octane behaviour,
# not Laravel's default welcome screen which loads view templates.
RUN cat > routes/web.php <<'PHP'
<?php
use Illuminate\Support\Facades\Route;
Route::get('/', fn () => response()->json([
    'message' => 'hello from laravel',
    'items'   => [1, 2, 3, 4, 5],
    'ok'      => true,
]));
PHP

ENV APP_KEY=base64:00000000000000000000000000000000000000000000=
ENV APP_ENV=production
ENV APP_DEBUG=false
ENV LOG_CHANNEL=stderr

EXPOSE 8000
CMD ["php", "artisan", "octane:start", \
     "--server=swoole", \
     "--host=0.0.0.0", \
     "--port=8000", \
     "--workers=auto"]
