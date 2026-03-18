---
tempo-request: patch
---

Tighten charge payment provisioning retry to only fire on auth/payment (401–403) and server error (5xx) status codes, avoiding wasteful retries on unrelated API errors like 400 body validation. Show full server response body in payment rejection errors instead of extracting a single JSON field. Ensure all retry paths surface the original error on retry failure.
