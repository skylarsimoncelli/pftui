# pftui Mobile

This folder contains the native iOS client for local testing.

## Structure

- `app/` — SwiftUI iPhone app that talks to the pftui mobile API

## Local test flow

1. Enable the mobile API once:
   - `pftui system mobile enable --bind 0.0.0.0`
2. Generate a read token for the phone:
   - `pftui system mobile token generate --permission read --name ios`
3. Start the server:
   - `pftui system mobile serve`
4. Copy the printed TLS fingerprint and the generated API token.
5. Open `mobile/app/PftuiMobile.xcodeproj` in Xcode.
6. Run on an iPhone or simulator and enter `hostname` or `hostname:port`, the API token, and the fingerprint.

The app uses pinned TLS plus a scoped bearer API token before it requests portfolio or analytics data.

## Release builds

Tagged releases now build `mobile/app/PftuiMobile.xcodeproj` in the existing release workflow and upload a `pftui-ios-mobile-simulator.zip` artifact.
