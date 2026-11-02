# ZapFast: overall application guide

An internal map of what ZapFast is, how it is put together, and where a change
belongs. It is written for someone about to work on the code, not for a user.
`AGENTS.md` remains the authoritative rulebook; this file is the overview that
makes those rules easier to place.

## 1. What the application is

ZapFast is a native WhatsApp client for Linux, macOS, and Windows: Rust with
egui for the interface, [whatsapp-rust](https://github.com/oxidezap/whatsapp-rust)
for the protocol, and [fastframe](https://github.com/crmne/fastframe) for the
desktop plumbing shared with the author's other apps. It links as a companion
device by QR code or phone-number code and keeps its own encrypted message
archive, so most reading works offline.

| Fact | Value |
| --- | --- |
| Crate | `zapfast`, lib + bin (`src/lib.rs`, `src/main.rs`) |
| Version | 0.21.0 (`Cargo.toml`) |
| Edition / MSRV | 2024 / Rust 1.98 (`rust-toolchain.toml` pins 1.98.0) |
| Licence | MIT |
| Size | ~211k lines of Rust in `src/`, 150 files, 59 modules |
| Tests | ~2,350 `#[test]` functions (215 async) |
| UI | egui 0.36.2 via a pinned `crmne/egui` fork; glow renderer, no wgpu |
| Translation | 11 interface languages, gettext catalogs in `assets/i18n/` |

### Product boundaries (do not cross)

These come from `AGENTS.md` and `PRODUCT.md` and decide whether a feature
exists at all:

- Small native client. No browser engine, telemetry, hosted backend, or
  ZapFast-operated account system.
- WhatsApp only. No other networks or bridges to them.
- No automation. No bots, MCP server, scripting/CLI/API that reads or sends
  messages, and no scheduled, bulk, or automatic replies. Opening a chat from
  a link is allowed because a person still reads and writes it.
- Cross-platform. A feature works on all three platforms, with `cfg` blocks
  and a sensible fallback where platforms differ.
- Fewer settings. A good default beats a new setting; a new setting needs the
  maintainer's approval.
- The protocol belongs to whatsapp-rust. Fix it upstream, carry a pinned
  commit from the approved fork, never reimplement protocol pieces here.
- The archive is the user's personal data. Message bodies, numbers, keys, and
  QR payloads are never logged, never read out for debugging, and never sent
  anywhere.
- Pull requests leave `assets/i18n/` alone unless the change is a translation.

## 2. Repository layout

```
src/            application code (see §4 for the module map)
  ui/           egui views and shared widgets
  backend/      runtime handle + worker (tokio) + per-feature worker modules
  archive/      SQLCipher archive: schema, queries, per-feature stores
  call_audio/   echo cancellation, device streams, dump/replay tooling
  demo/         offline demo data, tours, screenshots (feature `demo`)
assets/         icons, fonts, sounds, wallpaper, i18n catalogs, demo fixtures
docs/           the Jekyll site at zapfast.rocks, plus internal notes
tests/          integration tests (update flags, packaging, macOS menu, presence)
examples/       aec_replay (echo-canceller offline analysis)
packaging/      Flatpak metainfo, AUR/DEB/RPM recipes, release notes
contrib/        Omarchy theme templates
nix/, flake.nix Nix packaging
scripts/        repository helper scripts
.github/        CI, release, packaging, docs workflows and policy checks
```

Supporting documents: `AGENTS.md` (architecture rules, the reference to
extend), `CONTRIBUTING.md` (issue/PR process and design principles),
`DEMO.md` (offline demo, screenshots, recorded tours), `PACKAGING.md` (native
packages and releases), `PRODUCT.md` (users, character, design principles),
`docs/interactive-message-actions.md` (interactive message compatibility).

## 3. Building, running, testing

`mise.toml` routes `cargo` through `mbx`, which shares compiled artifacts and
budgets each checkout's `target/`. Plain `cargo` still works.

```sh
cargo run                          # the real app
cargo run --features demo -- --demo            # offline sample chats
cargo run --features demo -- --demo-page login # one screen
cargo run --features demo -- --demo-shot out.png --demo-page chat,light
cargo run --features demo -- --demo-tour       # recorded 41 s tour
```

Linux build dependencies: `libasound2-dev`, `libpipewire-0.3-dev`, clang and
libclang (audio), plus the usual X11/Wayland/GL headers; cmake, perl and a C++
compiler for OpenSSL, OpenH264 and libopus that build from source. `nasm` is
optional (OpenH264 SIMD paths only).

The definition of done (`AGENTS.md`) is the full matrix:

```sh
cargo fmt --all --check
cargo clippy --locked --all-targets -- -D warnings
cargo clippy --locked --all-targets --all-features -- -D warnings
cargo test --locked --all-targets
cargo test --locked --all-targets --all-features
RUSTDOCFLAGS='-D warnings' cargo doc --locked --all-features --no-deps
```

The `demo` feature doubles as a test harness: `src/demo.rs` carries sample data
and a headless layout test of every screen, and `--demo-shot` renders one to a
PNG. That is the normal way to see a view change without an account. Unit tests
sit beside the code; integration tests live in `tests/`; the echo canceller has
a recorded-room test that only runs when `ZAPFAST_ECHO_RECORDING` names a file,
plus `examples/aec_replay.rs` for sample-exact replay.

CI (`.github/workflows/ci.yml`) runs a RustSec audit, fmt/clippy, tests with and
without `--all-features`, docs, a demo build and a macOS window smoke shot.
Pull requests run Linux only; `main` runs the macOS, Windows and Nix matrix.
`policy.yml` enforces repository policy: dependency sources must be crates.io or
`github.com/crmne/...`, and review media must not be committed.

## 4. Architecture

One sentence: an event-driven egui front end owns no protocol state, a tokio
runtime per linked account owns all of it, and a SQLCipher archive is the only
thing both sides agree on.

```
        UI thread                          backend thread(s)
  ┌──────────────────┐   Command (mpsc)  ┌────────────────────┐
  │ frames: draw     │ ────────────────► │ Worker (tokio)      │
  │ Action queue     │                   │  Client (whatsapp-  │
  │ apply() after    │ ◄──────────────── │  rust) + archive +  │
  │ the frame        │  Event (mpsc) +   │  downloads, calls   │
  └──────────────────┘  Waker.wake()     └─────────┬──────────┘
           │                                        │
           └───────────── archive.db (SQLCipher) ───┘
```

### 4.1 Process and accounts

- `App` (`src/app.rs:450-981`) is the process shell: settings, theme, window,
  tray, accounts roster, app lock, updates, notifications. About 500 fields;
  it derefs to the active `Account`, so views address the current account as
  `app.chats`, `app.backend`, `app.composer`, and so on.
- `Account` (`src/account.rs:22`) is one linked number: its `Backend`, loaded
  chats/messages, open chat, drafts, per-account settings. `accounts.json` and
  `state/accounts/<id>/` keep them apart; `ChatId` is unique only inside one
  account, so every notification, tray click, and search hit carries an
  `AccountId`.
- All accounts share the window. Events from a background account are applied
  with `App::events_hidden` set: they update that account and never touch the
  visible composer, dialogs, playback or read state.
- `Backend` (`src/backend.rs:1747`) spawns a two-worker tokio runtime on its
  own OS thread (`Backend::spawn`, :1762). It exposes exactly two channels:
  `Command` (UI → runtime, unbounded `mpsc`, 206 variants) and `Event`
  (runtime → UI, `std::sync::mpsc`, 93 variants). Every emit calls the
  backend's `Waker` (`fastframe_shell::Waker`) so egui repaints only when
  something happened; the app is otherwise idle. A oneshot permit
  (`Backend::take_startup`, :1927) holds migrations and the connection until
  the first window frame, and dropping it cancels startup.
- `Worker` (`src/backend/worker.rs:855`, 27k lines) is the runtime side: the
  whatsapp-rust `Client`, the archive, download and sticker state, and at most
  one call. Its main loop is one `select!` over protocol events, UI commands,
  and runtime timers (:1107). Per-feature modules live in
  `src/backend/worker/` and are listed in §4.3.

`Command` groups (names only): blocklist; polls; send/link/voice/stickers;
export; read & presence; history and archive reads; media download, clipboard,
picture editing; chat mutation (mute, lock, labels, edit, revoke, delete,
pin, star, leave); calls; sticker packs; settings and pickers; profile and
group administration; contacts; link lifecycle (`Relink`, `RemoveAccount`,
`Reconnect`, `Shutdown`); privacy and channels; updates. Internal task replies
are starred variants (`*Downloaded`, `*PollCreated`, `*GroupManaged`, …) so a
command that completes on a spawned task returns through the same queue.

`Event` groups: link/account; chats, labels, contacts, drafts, stars, pins,
search; messages, receipts, typing, presence, avatars, sync progress; media
and upload progress; stickers and pictures; export; group administration;
calls (`Call`, `PhoneOnlyCall`, `CallVideo`, `CallLog`); privacy and settings;
updates.

### 4.2 Interface layer

`App::frame_ui` (`src/app.rs:11350`) draws one frame through `ui::show`
(`src/ui/mod.rs:42`), then drains `App::actions` in `apply_actions`
(:7239 → `apply`, :7264). This is the hard rule: **views never mutate
application state**. A view reads `&App` and pushes `Action` values (274
variants in `src/model.rs:2710`); the app applies them after the frame and
re-drains any action queued during application. A frame therefore sees the
previous frame's state, which is what makes the UI single-threaded and
deterministic.

`ui::show` routing, in order: app lock (only the lock screen) → keyboard
handling (`ui/keys.rs`) → title bar → unlinked (QR/pairing login) → linked:
narrow (<620 pt) switches between list and detail with a slide; wide draws
`rail` (52 pt), optional `chats` list, optional in-chat search `pane`, optional
`info` panel, and the central page (`conversation`, `settings`, `calls`,
wallpaper) → overlays (update, picker, dialogs, image preview, call, editor,
toasts, focus ring, accessibility announcements, window chrome).

| Module | Role |
| --- | --- |
| `ui/chats.rs` | chat list, search, chips (unread/favourites/labels), archive |
| `ui/conversation.rs` | header, transcript, composer, attachments, in-bubble actions |
| `ui/info.rs` | contact/group/channel panel, media grid, group admin, permissions |
| `ui/pane.rs` | in-chat search and jump-to-message |
| `ui/rail.rs` | left navigation: Chats, Calls, Starred, Archived, Locked, Settings, avatar |
| `ui/calls.rs`, `ui/call.rs` | Calls history page; live call surface, tiles, ring prompt |
| `ui/dialogs.rs` | shortcuts, contact, chat, confirmation dialogs |
| `ui/settings.rs` | settings pages and account privacy |
| `ui/picker.rs` | emoji, GIF and sticker picker; reaction picker |
| `ui/image_preview.rs`, `ui/image_editor.rs`, `ui/attachment_preview.rs` | media viewer, crop/rotate, staged attachment review |
| `ui/new_group.rs`, `ui/blocking.rs`, `ui/labels.rs`, `ui/polls.rs`, `ui/locations.rs`, `ui/message_info.rs`, `ui/chat_export.rs` | one feature each |
| `ui/login.rs`, `ui/lock.rs`, `ui/accounts.rs` | QR/pairing, app lock screen, account switcher |
| `ui/widgets.rs`, `ui/glass.rs`, `ui/focus.rs`, `ui/list_nav.rs`, `ui/overlay.rs` | shared controls, header blur, focus order, list cursor, overlay focus |
| `ui/keys.rs` | shortcut dispatch, the `SHORTCUTS` table, the shortcuts dialog |

Theming: `src/theme.rs` owns `Palette` (every semantic colour plus bubble
style), light/dark presets, fonts, zoom, and the icon table
(`fastframe_icons::icons!`). Custom palettes are JSON from fastframe-theme,
including the Omarchy live theme. New colour = a `Palette` field set in both
presets; new icon = a 24 px Lucide-style SVG in `assets/icons/` plus a table
entry (or `lucide "name"` when fastframe-icons already ships it).

Text, i18n, accessibility: `i18n::gettext(app.locale, "English")` against
catalogs compiled by `build.rs` from `assets/i18n/*.po`; `src/a11y.rs` builds
spoken message labels from plain data and `src/announcer.rs` emits AccessKit
live regions, silenced while locked; `markup.rs` lays out WhatsApp markup,
links and mentions, and `transcript.rs` rebuilds copied sweeps.

`Action` handling respects the app lock: `App::allowed_while_locked`
(`src/app.rs:12263`) is an allow-list, and the few private actions
(`OpenMessage`, `StartChat`, `OpenUrl`, `PrefillComposer`) are deferred until
unlock instead of dropped.

### 4.3 Worker modules

`src/backend/worker/` is where features that need the network live.

| Module | Responsibility |
| --- | --- |
| `calls.rs` | one call per account, signaling through `voip()`, group roster |
| `polls.rs`, `poll_history.rs` | create/vote/decrypt polls, recover vote snapshots |
| `interactive.rs`, `interactive/replies.rs` | parse interactive messages, send allowed replies |
| `group_admin.rs`, `new_group.rs` | permissions, members, invite links, group creation |
| `stickers.rs`, `sticker_pace.rs` | packs, favourites, recent, rate-limited fetch and send |
| `disappearing.rs` | expiry timers and message lifetime |
| `favorite_chats.rs`, `archive_chats.rs`, `read_sync.rs`, `link_preview_setting.rs` | app-state sync with the phone |
| `star_recovery.rs`, `blocking.rs`, `contact_names.rs`, `channel_pictures.rs` | stars, blocklist, names and pictures |
| `prefetch.rs`, `download_folders.rs`, `chat_export.rs` | background download, folder layout, local export |
| `link_preview.rs`, `link_watch.rs` | link previews; detecting a socket alive but silent |
| `early_events.rs` | receipts and reactions that arrive before their message |
| `device_store.rs` | opens whatsapp-rust's SQLite store, keeps the legacy column |
| `bot_replies.rs` | WhatsApp Business quick replies |

### 4.4 Invariants the worker protects

- A failed archive unlock stops linking. Never fall back to a disposable
  archive.
- Every late async answer is discarded by a generation counter
  (`subject_generation`, `settings_generation`, `star_generation`,
  `presence_generation`, `privacy_generation`, `older_serial`, edit
  sequences, and others). When adding a command that changes synced state,
  add its generation.
- One attachment send per chat (`send_turns`), one call per account, one read
  state write at a time (`read_sync`, with backoff), serial forward queue.
- Dedupe in-flight work by id (`downloads`, `sticker_fetches`,
  `message_removals_in_flight`, `early_acks`).
- Private content is withheld until the phone's lock state is known
  (`privacy_ready`, `PRIVACY_GRACE`), and pages withheld then are re-read once
  revealed.
- Removing an account stops its backend, then deletes its folders and keyring
  entry.

### 4.5 Domain model and archive

`src/model.rs` (4.5k lines) is the vocabulary both sides share. It is
protocol-agnostic in intent: the worker translates protobufs into it
(`classify`, `parse_conversation`) and views never see a protobuf. It is still
WhatsApp-shaped in places, notably `ChatId` being a JID string
(`model.rs:12`) and group-specific receipt and permission types.

| Group | Types |
| --- | --- |
| Identity and navigation | `ChatId`, `AccountId`, `ChatKind`, `Page`, `ChatFilter`, `ListFilter`, `CallFilter`, `MediaTab`, `PickerTab`, `Scroll` |
| Chats | `Chat` (:264), `GroupSettings` (:330), `LastMessage` (:346), `Label`, `Contact` |
| Messages | `Message` (:575), `Delivery`, `Content` (:783: text, interactive, image, video, audio, document, sticker, pack, location, live location, contact, poll, revoked, timer, unsupported, phone-only, call), `Quoted`, `Reaction`, `MentionRef`, `LinkPreview`, `Media`, `Motion` |
| Interactive and polls | `InteractiveCard/Button/Action/Option`, `PollState/Voter/Draft` |
| Calls | `CallStatus`, `CallDirection`, `OnceMedia`, `CALL_ENTRY_PREFIX`, `CALL_LINK_CHAT` |
| Receipts | `Recipient`, `MessageReceipts` |
| Interface state | `Dialog`, `InfoPanel`, `Toast`, `Page`, `Upload`, `ComposerFormat`, `Action` (:2710) |

`src/archive.rs` (6.6k lines) is the SQLCipher store. Core tables:
`chats`, `messages` (JSON `content`, `raw` protobuf, status, quoted,
reactions, `edited`), `motion_clips`, `contacts`, `meta` (sync cursors and
backfill keys), `chat_removals`, `message_removals`,
`pending_message_removals` (deletion barriers), `lids` (privacy id → phone),
`stickers`, `group_receipts`. Feature modules add their own: `drafts`,
`favorites`/`favorite_changes`, `local_labels`/`local_chat_labels`,
`message_pins`, `polls`/`poll_history`/`poll_votes`, `stars`,
`removed_recent_stickers`/`favorite_stickers`.

Schema changes are additive: every open runs `CREATE TABLE/INDEX IF NOT
EXISTS` and adds each missing column after a `PRAGMA table_info` check
(`archive.rs:499`). There is no numbered migration ladder; re-derived content
is rebuilt by backfills gated on `meta` keys (`derived`, `interactive_text`,
`lottie_stickers`). **When changing derived content, bump the matching
version and let the backfill rebuild it**; older builds then keep working but
may not understand newer content shapes (accepted risk).

Encryption (`src/archive/encryption.rs`): SQLCipher with a random 32-byte key
in the OS keyring, service `rocks.zapfast.ZapFast`, entry
`archive-<sha256(parent path)>`. A plaintext archive from an older build is
detected by its SQLite header and migrated through an encrypted staging file
that is verified with `integrity_check`, then renamed atomically. A missing or
invalid key stops linking; the app never replaces an unreadable archive with a
disposable one. The session store (`session.db`) is separate and is what
unlinking deletes.

Archive invariants:

- `messages.raw` keeps the original protobuf because attachment keys and
  re-derivation live there. Do not drop it.
- Upserts keep the newest status (except an explicit failure), non-empty
  reactions, and the older history order; deletion barriers refuse reinsert.
- Group receipts snapshot the audience at send time and report the least
  advanced recipient. Never infer an audience from current membership.
- Deletions are confirmed with the server before local removal; barriers stop
  history sync from restoring a deleted message.
- Local call and timer entries, and the call-link pseudo-chat, are never sent
  or receipted.
- Star, pin and favourite changes are written only after confirmation or phone
  sync, through queues with backoff.

### 4.6 Calls and media

Calls (`src/calls.rs`, 5.5k lines) are WhatsApp's `voip()` stack from
whatsapp-rust: signaling, keys, relay and codecs are the library's. The local
phase machine is `Placing` (drawn immediately, cancellable), then signaling
gives `Dialing`, `Ringing` (peer preaccept), `Incoming`, `Connecting`,
`Accepted`, `Active`, `Ended`, `Failed`. Group calls are on
(`GROUP_CALLS_ENABLED`). While they are off, an incoming group offer is kept
for the phone: WhatsApp is told nothing, the phone keeps ringing, and the app
shows a notification saying who is calling which group and to answer there.

Audio path: engine frames are 16 kHz mono, 960 samples. `call_audio.rs` binds
fastframe-audio microphone and speaker streams behind stable channels so a
device change rebinds instead of ending the call. Echo cancellation
(`call_audio/echo.rs`, `chain.rs`) taps what the speaker actually played
through a lock-free `rtrb` ring, then runs sonora high-pass → WebRTC AEC3
(driven directly with a tuned room configuration) → sonora noise suppression
and AGC2. Headphones need no setting: AEC3 goes transparent when it hears no
echo. `call_tones.rs` synthesises ringback and ringtone.

Video path: `camera.rs` reads cameras (linuxvideo on Linux, nokhwa elsewhere),
converts to I420, `openh264` encodes Constrained Baseline 720p/25 fps, and
`video_rate.rs` adapts bitrate and resolution step to RTCP loss, REMB, queue
delay, and the peer's received rate, keeping room for 1:1 FEC. Remote access
units decode on their own thread. Diagnostics are opt-in: audio,
`ZAPFAST_CALL_AUDIO_DUMP`; video, `ZAPFAST_CALL_VIDEO_DUMP`; echo recording,
`ZAPFAST_ECHO_RECORDING`. Recordings never enter the repository.

Local media:

- Voice (`src/voice.rs`): OGG/Opus mono 48 kHz with a 64-bar waveform and
  normalization; `audio.rs` plays and records through fastframe-audio, one clip
  at a time, speed 1×/1.5×/2× through `timestretch.rs` with pitch preserved.
- Animation (`animation.rs`): GIF/WebP/Lottie in process, MP4 through `mp4` +
  `openh264`, `ffmpeg` only as a fallback codec path. Paused animations decode
  only their poster.
- Video messages (`video.rs`): `mp4` demux, H.264 decode, AAC through
  symphonia and rodio, clocked by the sound's position. Round notes draw as
  circles. Unsupported formats go to the system player.
- Emoji (`emoji.rs`): fastframe-emoji swaps emoji for placeholder glyphs at
  layout time and paints platform bitmaps over them; `EmojiPlugin` covers
  labels and menus without double-painting message text.
- Markup and copy (`markup.rs`, `transcript.rs`): WhatsApp markup, links,
  mentions and bidi into an egui `LayoutJob`; selection is shared across
  bubbles and a copied sweep is rebuilt with `[time, date] Name:` headers.

### 4.7 Platform integration

| Concern | Where |
| --- | --- |
| Window survives closing, tray, headless loop | fastframe-shell; `main.rs:428`, `App::background_frame` |
| Single instance, second launch surfaces the first | `single_instance.rs`, slot `fastsapp`, legacy port 47119 |
| Tray, autostart at login, Dock icon and badge | fastframe-tray / autostart / dock (`autostart.rs`) |
| Notifications, clicks, withdrawal | `notify.rs`; 32 pending cap, per-chat throttling, clicks carry chat and message |
| Custom title bar, traffic lights, window drag | `ui::window_chrome`, fastframe-titlebar, `macos.rs` |
| Proxy and transport | `proxy.rs` (Settings, then `ALL_PROXY`/`HTTPS_PROXY`), `transport.rs` (DNS per dial, IPv6/IPv4 race) |
| Updates | `updates.rs`, fastframe-update, legacy `fastsapp` releases and two publisher keys |
| File paths and old-name migration | `paths.rs` (`fastsapp` → `fastwhatsapp` → `zapfast`), single-account → `accounts/1` |
| App lock | `app_lock.rs` (PBKDF2 verifier only, not encryption) |
| Link safety | `safety.rs` (http/https/mailto only, invite and call-link host checks) |

Logging never carries message content, phone numbers, keys or QR payloads;
`diagnostics.rs` maps protocol errors to fixed categories. The log is meant to
be attached to bug reports, so treat anything added there as public.

### 4.8 Startup order

`main.rs` runs this sequence, and the order matters:

1. `fastframe_update::intercept` (before argument parsing, so older releases'
   updater flags keep working).
2. CLI parsing; a control subcommand (`quit`, `reload-themes`) is sent to a
   running copy and exits.
3. Target/invite arguments are parsed **before** the single-instance guard, so
   a second launch forwards its target to the first.
4. Single instance claim; then `adopt_previous_names`, `dirs.ensure`,
   logging (filter, protocol redaction, per-run file), and the single-account
   folder move into `accounts/1`.
5. `fastframe_audio::set_app_info` before any device opens, settings load,
   title bar and Dock setup, `App::new`.
6. The window is handed to `fastframe_shell::Shell`; each account's
   `Backend::take_startup` permit is released after the first frame, which is
   what lets migrations and the network start without delaying the window.

Shutdown flushes `fastframe_file::Writer` settings, stops each backend
(`runtime.shutdown_timeout(3s)`) and leaves the tray and window as configured.
Closing the window keeps the process alive only when the tray exists; a
hidden start that no panel shows yet waits `TRAY_WAIT` before showing itself.


## 5. How a message flows

1. whatsapp-rust delivers a protocol event on the account's runtime.
2. `Worker` classifies it (`classify`/`parse_conversation`), canonicalises the
   chat id through `Worker::canonical` (privacy id → phone), writes it to the
   archive, settles any early receipts, and emits `Event::Incoming`,
   `Event::Messages` or `Event::ChatUpdated`.
3. `App::handle_events` drains the channel, applying hidden-account events with
   `events_hidden`, then queues a notification, badge and sound.
4. The next frame draws from the updated archive-backed state. A user action in
   a view becomes an `Action`, applied after the frame.
5. Sensitive or network actions become a `Command`; the worker performs them,
   awaits the library, and answers with a starred internal event, so the UI
   learns the outcome through the same queue.

Sending is the same path in reverse: composer → `Action::SendPending` → prepared
preview (`ComposedPreview`) → `Command::SendText/SendFiles/...` → worker
files the outgoing message, then the server updates status and receipts.

## 6. Feature map

| Area | Entry points |
| --- | --- |
| Linking, QR, phone code | `ui/login.rs`, `backend/worker.rs` link lifecycle, `qr.rs` |
| Chat list, search, chips, archive | `ui/chats.rs`, `archive` queries, `util.rs` (accent-insensitive search) |
| Conversation, composer, drafts | `ui/conversation.rs`, `archive/drafts.rs` |
| Replies, edits, reactions, forwards, pins, stars | `model::Content`, `archive` barriers, worker commands |
| Media: images, video, voice, documents, stickers, GIFs | `ui/picker.rs`, `image_cache.rs`, `media` cache, `animation.rs`, `video.rs`, `voice.rs` |
| Polls | `backend/worker/polls.rs`, `archive/polls.rs`, `ui/polls.rs` |
| Interactive messages and carousels | `backend/worker/interactive.rs`, `ui/widgets.rs` |
| Groups, permissions, invite links | `backend/worker/group_admin.rs`, `ui/info.rs`, `group_admin.rs` |
| Channels, communities | `ChatKind`, channel pictures and mutes |
| Calls, group calls, call links | `calls.rs`, `ui/call.rs`, `ui/calls.rs` |
| Privacy: app lock, locked chats, disappearing | `app_lock.rs`, `privacy.rs`, `backend/worker/disappearing.rs` |
| Export | `archive/chat_export.rs`, `backend/worker/chat_export.rs` |
| Themes and wallpaper | `theme.rs`, `wallpaper.rs`, `contrib/omarchy/` |
| Updates, packaging, single instance | `updates.rs`, `packaging/`, `single_instance.rs` |

## 7. Where a change belongs

- **New user-visible behaviour**: `model.rs` gets the types and `Action`;
  `ui/` draws it and pushes the action; `app.rs::apply` handles it; the worker
  gets the `Command` and `Event`. Nothing else may touch state from a view.
- **Protocol capability**: check whatsapp-rust first. If it exists, wire it in
  `backend/worker/`; if not, fix it upstream and carry the pinned commit.
- **New stored data**: add the table or column additively in the relevant
  `archive/` module, add a `meta` backfill key if it is derived from existing
  rows, and cover migration with a test.
- **New screen**: `ui/` module, route in `ui::show`, page or dialog enum in
  `model.rs`, sample data and a headless layout test in `demo.rs`.
- **New string**: `gettext` with the English source; catalogs are
  maintainer-generated, so do not edit `assets/i18n/*.po` in a normal change.
- **New icon or colour**: `assets/icons/` + `fastframe_icons::icons!`, or a
  `Palette` field set in both presets.
- **New setting**: needs the maintainer's approval first. Prefer a default.
- **New dependency**: crates.io or `github.com/crmne/...`, justified by build
  time, size, maintenance and security; CI enforces the source rule.

## 8. Testing and observability

- Unit tests live beside the code; `tests/` covers process behaviours
  (update flags, packaging, macOS menu, presence).
- The `demo` feature (`src/demo.rs`, 26k lines, plus `src/demo/tour/`) provides
  offline sample chats, synthetic pages and dialogs, a headless layout test of
  every screen, scripted tours, and `--demo-shot` PNG capture. Media for demos
  and reviews is synthetic and never real conversations.
- The echo canceller has a recorded-room test that runs only with
  `ZAPFAST_ECHO_RECORDING`, and `examples/aec_replay.rs` replays a dump sample
  for sample with alternative AEC3 settings.
- Runtime diagnostics: `--verbose`, per-run log and previous-run log,
  `panic.log`, `diagnostics.rs` protocol error categories, and the opt-in
  call dumps above.
- Platform reporting is honest: say what was run on which OS and what was only
  compiled.

## 9. Known tensions

- `model.rs` is protocol-agnostic in intent but WhatsApp-specific in shape
  (JID ids, group receipts, WhatsApp content kinds).
- The archive has no numbered schema version. Additive columns plus `meta`
  backfill keys keep old archives opening; they do not make an older build
  understand newer content.
- `app.rs` (27k lines) and `backend/worker.rs` (27k lines) are the two gravity
  centres. New behaviour should land in a module they call, not as another
  thousand lines in either.
- WhatsApp's terms forbid automated use, and an unofficial client may be
  against them; the product boundary exists to keep account risk low, not out
  of taste.
- The UI thread must stay free of network and disk waits. Anything slow goes to
  the worker or a thread of its own with a channel back.

