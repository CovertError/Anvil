# Spark JS runtime sources

`dist/spark.min.js` is the source of truth. It is committed to the repo so that
`cargo build` works without Node or esbuild installed.

Edit `dist/spark.min.js` directly. The file is intentionally readable rather than
minified — bundle size for this MVP is dominated by morphdom-lite (~2 KB
inlined) and the dispatch logic (~4 KB), well under any latency budget.

A future improvement is to add a `build.rs` that runs `esbuild` over a
TypeScript source in this directory when `SPARK_REBUILD_JS=1` is set, falling
back to the committed bundle when not.
