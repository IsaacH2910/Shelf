# Security

Shelf is a local-first desktop app. Files stay on the Mac that indexes them. Optional cloud access exposes a loopback HTTP origin through a Cloudflare named tunnel so signed-in devices can read that library from any network.

## Report a vulnerability

Please **do not** open a public GitHub issue for a security problem.

Email the maintainer privately, or open a private GitHub security advisory if the repository has that feature enabled. Include:

- A description of the issue and its impact
- Steps to reproduce, or a proof of concept that does not include live credentials
- Affected version or commit

You should hear back within a few days. Please give a reasonable window to fix and release before any public disclosure.

## What is in scope

- Authentication and session handling (passwords, cookies, lockout, grants)
- Authorization gaps that let one household member see another member’s library or progress
- Exposure of the loopback origin beyond `127.0.0.1` without an explicit nearby session
- Path traversal or file-read bugs that escape granted library roots
- Secrets committed to the repository (tunnel tokens, passwords, keychain dumps)

## What is out of scope

- Physical access to an unlocked Mac that already has the library mounted
- A Cloudflare account or DNS zone that you do not control
- Social engineering of household passwords
- Issues that only appear after you disable cloud access and still expect remote clients to work

## Security model (short)

- **Origin.** The HTTP service binds to `127.0.0.1:7834` unless the owner starts **Connect iPhone / iPad**, which listens on the LAN and advertises `_shelf._tcp` for that session only.
- **Passwords.** Owner and household passwords are hashed with Argon2id. The minimum length is 8 characters. Remote sign-in is disabled until the owner password is set.
- **Sessions.** HttpOnly cookies, idle expiry (14 days), absolute expiry (90 days), periodic rotation, and per-device revoke.
- **Lockout.** Repeated failures lock an account briefly; a global limiter also applies.
- **Grants.** Members only see series you grant them, unless they have whole-library access. Progress is per user.
- **Tunnel.** `cloudflared` makes an outbound connection. No inbound port-forward or VPN is required. The public hostname must match the configured host; other Host headers are rejected.
- **Translation.** OCR and translation caches live in the app database. Source PDFs and videos are not modified.

## Credentials

Never commit a Cloudflare tunnel token, owner password, or session cookie. If a token was ever checked in, revoke it in the Cloudflare dashboard and issue a new one before the repository is public.

Store the public hostname in **Settings → Cloud** (local SQLite). Store the Cloudflare tunnel token in the macOS keychain, or in `SHELF_TUNNEL_TOKEN` for a local process. Never put either value in source.

The macOS keychain entry is:

- Service: `com.isaach.shelf`
- Account: `cloudflare-tunnel-token`
