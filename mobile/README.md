# pftui Apple Clients

This folder contains the native Apple-platform clients for local testing.

## Structure

- `app/` — SwiftUI app targets for iPhone and macOS that talk to the pftui mobile API

## Local test flow

1. Enable the mobile API once:
   - `pftui system mobile enable --bind 0.0.0.0`
2. Generate a read token for the phone:
   - `pftui system mobile token generate --permission read --name ios`
3. Start the server:
   - `pftui system mobile serve`
4. Copy the printed TLS fingerprint and the generated API token.
5. Open `mobile/app/PftuiMobile.xcodeproj` in Xcode.
6. Open either the `PftuiMobile` or `PftuiDesktop` scheme in Xcode.
7. Run on an iPhone, simulator, or macOS and enter `hostname` or `hostname:port`, the API token, and the fingerprint.

The app uses pinned TLS plus a scoped bearer API token before it requests portfolio or analytics data.

## Release builds

Tagged releases currently build the iOS artifact from `mobile/app/PftuiMobile.xcodeproj`. The project now also includes a native macOS target for local desktop builds.
