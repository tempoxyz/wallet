# Changelog

## 0.1.1 (2026-03-17)

### Patch Changes

- Simplified optimistic key provisioning by removing the `query_key_status` and `prepare_provisioning_retry` functions, instead retrying directly with `with_key_authorization()` on any error. Added support for merged `WWW-Authenticate` challenges (RFC 9110 §11.6.1) by splitting and selecting the first supported payment method. Fixed `list_channels` to exclude localhost origins and removed the realm-vs-origin validation check.

