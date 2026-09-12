# Shelf for iPhone and iPad

Native SwiftUI shell around the existing Shelf web reader. This is the **Bonjour client**.

Connection order (fixed):

1. **Bonjour** — browse `_shelf._tcp` while the Mac is in **Connect iPhone / iPad** mode
2. **LAN** — remembered or typed `http://host:7834` health probe
3. **Cloudflare** — saved public hostname over HTTPS

Safari and the PWA cannot browse Bonjour. Windows, Android, and Linux use the LAN URL (or Cloudflare), not this app.

Linux CI cannot compile this target. Open it on a Mac with Xcode 15.4 or later.

## What it does

1. Asks for Local Network permission (`NSLocalNetworkUsageDescription` + `_shelf._tcp`).
2. **Find Mac** browses `_shelf._tcp` with `NWBrowser` and reads the `url` TXT record.
3. Resolves with `NetService` if TXT has no URL.
4. Probes `GET /api/health` (2s). First healthy Bonjour instance wins.
5. If Bonjour misses, probes the saved LAN URL.
6. If LAN misses, loads **Settings → Cloudflare hostname**.
7. The reader is `WKWebView` with the default cookie store (login in-page).

Household accounts match the Mac app. Cookies stay on one origin. Switching from `http://mac.local:7834` to Cloudflare HTTPS will ask you to sign in again on that origin.

## Open in Xcode

```bash
open ios/Shelf/Shelf.xcodeproj
```

1. Select the **Shelf** target.
2. Set your **Team** for signing (`com.isaach.shelf.ios`).
3. Choose an iPhone or iPad simulator, or your device.
4. Run.

On a real device: Settings → Privacy & Security → Local Network must allow Shelf after the first browse.

```bash
xcodebuild -project ios/Shelf/Shelf.xcodeproj -scheme Shelf -destination 'platform=iOS Simulator,name=iPhone 16' build
```

## Use it with the Mac app

1. Mac: set the owner password, then **Settings → Connect iPhone / iPad**.
2. Leave Shelf running and the Mac awake.
3. Phone: same Wi-Fi, open this app, grant Local Network, tap **Find Mac**.
4. Pick this Mac. Chrome says **Bonjour**.
5. In **Settings**, you can also save a LAN URL (step 2) and the Cloudflare hostname (step 3).
6. Away from home (or nearby session off, Cloud on): chrome says **Cloud**.

LAN is HTTP on purpose. It is still password-protected. Remote access stays HTTPS through Cloudflare.

## Simulator notes

The iOS Simulator can browse Bonjour on the Mac’s network in many setups. If it does not see `_shelf._tcp`, save the LAN URL or Cloudflare hostname. `127.0.0.1` inside the simulator is the simulator, not the Shelf Mac process.
