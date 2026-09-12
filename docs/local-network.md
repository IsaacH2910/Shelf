# Nearby access (Bonjour + LAN)

Shelf can serve the library on your home Wi-Fi without Cloudflare. That is a **nearby session**, not a permanent listener. You start it from the Mac when you want a phone or tablet to connect.

```text
1. Bonjour   iPhone / iPad Shelf app browses _shelf._tcp
2. LAN       same Wi-Fi URL/IP (Safari, Windows, Android, Linux)
3. Cloudflare  HTTPS named tunnel when local fails
```

Bonjour is discovery. LAN HTTP is the transport. Cloudflare is last.

## Connect from an iPhone or iPad (Bonjour)

1. On the Mac, set the owner password (same bar as cloud).
2. **Settings → Connect iPhone / iPad**.
3. Choose how long to stay discoverable (until you turn it off, 15 minutes, or 1 hour).
4. Tap **Connect iPhone / iPad**. Shelf binds `0.0.0.0:7834` and advertises `_shelf._tcp`.
5. Leave the Mac awake. Sleeping Macs do not serve files and do not advertise.
6. On the phone, join the same Wi-Fi and open the native Shelf app (`ios/Shelf`).
7. Grant Local Network access. Tap **Find Mac**. Pick this Mac.
8. Sign in with a household account. The chrome says **Bonjour**.

Safari and Add to Home Screen **cannot** browse Bonjour. Use the native app for step 1.

When you tap **Stop nearby connections**, or the timer ends, Bonjour unregisters and the origin returns to loopback unless Cloudflare is still on.

## LAN URL (no Bonjour)

Use this for Windows, Android, Linux, or Safari on the same Wi-Fi.

1. Start a nearby session on the Mac as above.
2. Copy the **LAN URL** (`http://<hostname>.local:7834` or `http://<lan-ip>:7834`) or scan the QR.
3. Open that HTTP URL in a browser. Sign in.

The PWA remembers that LAN origin and, on the next visit from HTTP, probes it before Cloudflare. A PWA installed from the Cloudflare HTTPS hostname cannot probe `http://` LAN URLs (mixed content). Open the LAN URL directly, or use the iOS app.

## Cloudflare (last)

Away from home, on cellular, or when nearby mode is off, clients use the named tunnel. See [Cloud access](cloud.md).

The iOS app tries Bonjour, then a saved LAN URL, then the Cloudflare hostname you typed in its Settings. The same household accounts work on every origin. Cookies are origin-scoped, so switching from `http://mac.local:7834` to `https://your.hostname` can ask you to sign in again.

## Security

- Nearby mode is **off by default**. It needs an owner password and an explicit Connect action.
- LAN is **HTTP**, local-network only, still password-protected. Do not treat it as remote HTTPS.
- Host allow-list accepts loopback, the configured Cloudflare hostname, and this Mac’s Bonjour name / LAN IPs while nearby mode is on.
- `cloudflared` still targets `http://127.0.0.1:7834`. Nearby mode listens on all interfaces so both localhost and LAN work.
- No Bluetooth. No serving while the Mac sleeps.

## Manual check

1. Mac: Connect iPhone / iPad. Phone app on Wi-Fi: Find Mac → **Bonjour**.
2. Turn nearby mode off, or leave Wi-Fi. If Cloud is on: chrome says **Cloud**.
3. On a laptop browser: with nearby on, open the LAN URL. With nearby off and Cloud on, use the HTTPS hostname.
