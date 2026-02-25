---
presto: patch
---

Fixed multiple security vulnerabilities: prevented payment credential capture via malicious HTTP redirects by using the final URL after redirects for payment retries, added terminal escape sequence sanitization to prevent ANSI injection from server-controlled data, clamped voucher amounts to the known channel deposit to prevent coercion by malicious servers, and added network constraint validation in session requests.
