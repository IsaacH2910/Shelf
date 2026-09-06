# Cloud access

Shelf can publish the library to phones and tablets over HTTPS without opening a port on your router. Files stay on the Mac. The desktop app keeps working if the tunnel is off.

```text
iPhone / iPad
    HTTPS
        Cloudflare named tunnel
            cloudflared on the Mac (outbound only)
                Axum on 127.0.0.1:7834
                    SQLite + local files
```

There is no pairing PIN, no LAN listener, and no VPN. The public hostname stays the same across app restarts.

## What you need

1. [`cloudflared`](https://developers.cloudflare.com/cloudflare-one/connections/connect-networks/downloads/) on the Mac:

   ```bash
   brew install cloudflare/cloudflare/cloudflared
   ```

2. A hostname you control (for example `shelf.example.com`) and a [Cloudflare named tunnel](https://developers.cloudflare.com/cloudflare-one/connections/connect-networks/get-started/create-remote-tunnel/) whose public hostname points at:

   ```text
   http://127.0.0.1:7834
   ```

3. The tunnel token from Cloudflare, and an owner password of at least 8 characters.

If you are running from source, `npm run build` first so the tunnel can serve the web client from `dist/`.

## Enable it in the app

Hostname and token are **local only**. They are never compiled into the app.

1. In Cloudflare Zero Trust, create a named tunnel and copy its token.
2. Point that tunnel’s public hostname at `http://127.0.0.1:7834`.
3. In Shelf: **Settings → Cloud**
   - Set the owner password
   - Enter the public hostname
   - Paste the tunnel token and save
4. Enable cloud access
5. On the phone, open the HTTPS URL shown in Settings (or scan the QR code)
6. Sign in as `owner` with that password

The hostname is stored in the local SQLite settings. The token is stored in the macOS keychain (`com.isaach.shelf` / `cloudflare-tunnel-token`). Neither belongs in git.

For a source checkout you can also set environment variables instead of (or as a fallback for) the Settings fields:

```bash
export SHELF_TUNNEL_HOSTNAME=shelf.example.com
export SHELF_TUNNEL_TOKEN=...
```

Do not commit those values. A template lives in `.env.example`.

The origin always binds to loopback. Cloudflare terminates TLS. The HTTP service rejects `Host` headers that are not the configured hostname (with or without `www.`).

## Household

**Settings → Household** creates extra accounts. Each person signs in with their own username and password.

- Usernames are 2–32 characters: lowercase letters, digits, and underscores.
- Grant the whole library, or pick specific series.
- Progress is stored per user.
- Disable an account to cut off sign-in without deleting it.

The phone and tablet UI is a thin reader. Library roots, OCR engines, and household admin stay on the Mac.

## Devices

**Settings → Devices** lists active sessions. Revoke a lost phone there. Sessions idle out after 14 days, expire after 90 days, and rotate periodically. Eight failed logins lock that account for 15 minutes.

## Uploads

Signed-in clients can upload into a granted series (chunked, up to 8 GB per file). Completed files land in the series folder on the Mac and are indexed like any other local file.

## When something fails

| Symptom | What to check |
| --- | --- |
| “Install cloudflared to enable.” | `cloudflared` is not on `PATH` or in `/opt/homebrew/bin` / `/usr/local/bin` |
| Enable stays disabled | Hostname, tunnel token, and owner password are all set |
| Tunnel status stuck on connecting | Token, Cloudflare hostname, and the public-hostname → `127.0.0.1:7834` mapping |
| Login rejected | Owner password is set; account is not disabled or locked |
| Phone shows no series | That user needs a grant, or whole-library access |
| Blank page from source | Run `npm run build` so `dist/` exists |

Turn cloud access off at any time. Existing desktop use is unchanged.
