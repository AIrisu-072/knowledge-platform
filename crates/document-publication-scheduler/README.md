# Document publication scheduler

Build the Linux artifact with `docker build --target publication-scheduler -t knowledge-platform-publication-scheduler .`. The existing default Docker target remains `bootstrap-tools`.

Required runtime environment:

- `DOCUMENT_DATABASE_URL`: PostgreSQL connection URL for a database with the Versioning migrations applied. Keep credentials in deployment secrets.
- `DOCUMENT_STORAGE_ROOT`: absolute path to the mounted authoritative FileStorage root.
- `DSI_WORKER_EXECUTABLE`: absolute path to the approved DSI worker. The image sets this to its bundled worker.

Optional: `DSI_PDFIUM_RUNTIME_DIR` points to a mounted approved/pinned Pdfium runtime; `DOCUMENT_PUBLICATION_POLL_SECONDS` is 1–60 seconds (default 5). Startup runs a real sandboxed DSI TXT probe and exits if the worker or mandatory Linux sandbox cannot execute. The scheduler uses the database clock, bounded retries, and SIGINT/SIGTERM-aware shutdown through the container runtime.
