# @sidebandchat/tunes

`sideband-tunes` is a native Node.js and Electron library for discovering and observing media sessions exposed by the operating system. It is generic infrastructure: it does not contain Spotify-specific behavior or presence logic.

## Platforms

| Platform | Integration                     | Session enumeration | Changes       |
| -------- | ------------------------------- | ------------------- | ------------- |
| Windows  | GSMTC through `windows-rs`      | Yes                 | Native events |
| Linux    | MPRIS over D-Bus through `zbus` | Yes                 | D-Bus signals |

macOS is intentionally unsupported. Apple provides no public API for observing other applications' Now Playing sessions, and macOS 15.4 introduced private-entitlement enforcement around `MediaRemote.framework`. The library does not invoke AppleScript, entitlement-borrowing adapters, inject code, or poll individual applications. A macOS no-op binary is still built so cross-platform consumers can import the package safely; it reports `availability: "unsupported"`.

## Usage

```sh
npm install @sidebandchat/tunes
```

```ts
import {
  getPlatformCapabilities,
  getSessions,
  MediaSessionObserver,
} from "@sidebandchat/tunes";

console.log(getPlatformCapabilities());
console.log(await getSessions());

const observer = new MediaSessionObserver();
observer.on("added", (session) => console.log("added", session));
observer.on("changed", (session) => console.log("changed", session));
observer.on("removed", (id) => console.log("removed", id));
observer.on("error", (error) => console.error(error));

await observer.start();

// Later, during application shutdown:
await observer.close();
```

`positionMs` is a snapshot. Consumers should advance it from the observation time only while `playbackState` is `playing`. MPRIS does not send notifications for ordinary position advancement; it does signal seeks and other property changes.

## Metadata

Every session contains normalized title, artist, album, artwork, duration, position, playback state, application identity, track ID, and track URI fields when the platform and player expose them. `platformValues` retains useful native fields that do not have a portable equivalent.

Missing metadata is represented by absent properties. An unavailable media service produces a rejected discovery promise or an observer `error` event; it does not crash the host application.

## Development

```sh
npm install
npm run build:debug
npm test
```

The resulting `.node` module uses Node-API and can be loaded by Node.js or Electron without recompiling it for a particular Electron ABI.

The crate also exposes `sessions()`, `platform_capabilities()`, and `SessionObserver` directly to Rust consumers. The Node API is a binding over the same normalized core.

## Releases

Releases are built from version tags. The tag must exactly match the version in `package.json`.

```sh
npm version patch
git push origin main --follow-tags
```

GitHub Actions builds and tests every configured target before publishing anything. It then publishes a small root package plus an exact-version optional package for each supported OS and architecture. Consumers receive only their matching native package and do not need a Rust toolchain.

## License

Sideband Tunes is distributed under the GNU General Public License v3.0. See `LICENSE` and `COPYRIGHT` for details.
