---
presto: patch
---

Fixed browser opening logic to use a plain thread instead of `tokio::task::spawn_blocking` when waiting for user input before opening the URL, preventing the process from hanging after auth completes.
