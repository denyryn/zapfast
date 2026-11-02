# Telegram M0: decision packet

This is the M0 artifact from [`PLAN.md`](PLAN.md). It records what exists, what
Telegram offers instead, how the spike is run, what to measure, and what would
make the plan stop. It adds no dependency to the repository.

Status: boundary and credential decisions made (section 6). The spike code
compiles against `grammers-client` 0.11.0 in a throwaway crate under
`.cache/`. Running it is the only remaining M0 item, and it needs a Telegram
test number and its `api_id`/`api_hash`.

## 1. Blocking decisions

### 1.1 Product boundary

`AGENTS.md` says WhatsApp only, no bridges. `PRODUCT.md` repeats it. Adding
Telegram means editing both, the site's "What is ZapFast?" page, and the
account-risk text.

Options, in the order this plan prefers:

| Option | Meaning | Consequence |
| --- | --- | --- |
| A. Approve unified | One app, one list, both providers | Full `PLAN.md` to M3B |
| B. Approve separate page | Both providers, separate lists and conversations | `PLAN.md` to M3A, stop there |
| C. Separate sibling app | ZapFast stays WhatsApp-only; Telegram in a new app on fastframe | No boundary change here; new repository |
| D. Decline | Stay WhatsApp-only | Delete this plan |

### 1.2 Telegram application credentials

Telegram requires an `api_id` and `api_hash` per application, created at
`my.telegram.org` under a phone number. Choices:

| Option | Meaning | Cost |
| --- | --- | --- |
| Project credentials | Register ZapFast and ship the pair | The pair is public by nature; Telegram can revoke it; it ties the app to one phone number |
| User supplied | Ask at first link, explain where to get one | Bad onboarding, good compliance, no shared secret |
| Defer | Decide after the spike | Spike needs *some* pair, so a personal one is used and never committed |
| Do not ship | Telegram stays a fork or a sibling app | No integration |

Never commit an `api_id`/`api_hash`, and never log them. For the spike use a
personal pair and a throwaway number, not a daily account.

`grammers`' own documentation says a developer API ID may be embedded in the
binary and shipped, since users log in with their own accounts. That makes the
project route technically viable. It was still declined: a shared pair is
public, can be revoked for everyone at once, and ties the app to one phone
number.

## 2. Capability matrix

Compiled from `src/model.rs` (`Content`, `ChatKind`, `Chat`, `GroupSettings`),
`src/backend/worker/` modules, and the feature map in
[`OVERALL_APP.md`](OVERALL_APP.md). "Partial" means both sides have something,
but the shapes differ and the UI or archive must branch. "Work" is a size
hint, not a schedule.

### 2.1 Chats

| Feature (code) | WhatsApp today | Telegram | Verdict |
| --- | --- | --- | --- |
| `ChatKind::Direct` | Phone number or privacy id | Private chat, bot, Saved Messages | Both, ids differ |
| `ChatKind::Group` | Group, up to large sizes | Group and supergroup; topics in forums | Partial, topics are TG-only |
| `ChatKind::Broadcast` | Channel, one-way posting | Channel, plus linked discussion group for comments | Partial, comments are TG-only |
| Unread, marked unread (`Chat.unread`, `marked_unread`) | Server and local | Server, read max id | Both |
| Archived (`Chat.archived`) | Server flag | Server archive plus folders | Both |
| Pinned (`Chat.pinned`, `pinned_at`) | Server, global order | Server; chat pins of messages too | Both |
| Muted (`muted_until`) | Server | Server, including "until I turn it back on" | Both |
| Favourites (`favorite`, `favorite_position`) | Server list, ordered | No equivalent list; folders instead | Partial, decide: local favourite or drop |
| Labels (`labels`) | Local to this computer | Folders, synced per account | Partial, keep local labels and add folders later or never |
| Locked chats (`locked`) | Server chat lock, hidden behind a code | No equivalent | WA-only |
| Disappearing (`ephemeral_expiration`) | Per-chat timer, synced | Auto-delete timer, per chat and per account | Partial, semantics differ |
| Read receipts | Server, with privacy switch | Server, with privacy switch | Both |
| Typing, recording, presence | Server | Server | Both |
| Mention marker (`unread_mention`) | Computed from mentions | Mentions exist; marker computed locally | Both, same approach |
| Drafts | Local, archive `drafts` | Server drafts, synced across devices | Partial, source differs |
| Group permissions (`GroupSettings`) | Announce, add members, approval, invite link, admins, super admins | Slow mode, invite links, admin rights, join requests, restrictions | Partial, different fields |

### 2.2 Message content

| `Content` | WhatsApp today | Telegram | Verdict |
| --- | --- | --- | --- |
| `Text` | WhatsApp markup, bidi, mentions | Different markup (bold, italic, spoiler, code, pre), usernames as mentions | Partial, second formatter |
| `Interactive` | Buttons, lists, carousels, copy code | Bots' inline keyboards, not user messages | WA-only |
| `Image` | Photo, caption, view once, motion photo | Photo, albums, self-destructing media in secret chats | Partial, albums and view-once differ |
| `Video` | Video, round note, motion clip | Video, round note, albums | Both |
| `Audio` | Voice note, waveform, speed | Voice note, waveform, speed | Both, same OGG/Opus family |
| `Document` | File, caption | File, caption, streaming for large media | Both |
| `Sticker` | WebP, static or animated | WebP static, TGS Lottie, WebM video | Partial, two new decoders (TGS reuses `rasterlottie`) |
| `StickerPack` | Pack send | Pack send | Both |
| `Location` | Pin | Pin | Both |
| `LiveLocation` | Live sharing | Live sharing | Both |
| `Contact` | vCard | Contact share | Both |
| `Poll` | Encrypted, keys, vote snapshots, history recovery | Server poll: anonymous, quiz, multiple answers, revoting | Partial, different model; do not port `archive/polls.rs` |
| `Revoked` | Delete for everyone | Delete for everyone | Both |
| `DisappearingTimer` | System notice | Service message for auto-delete | Both, different text |
| `Unsupported` | Fallback | Fallback | Both |
| `PhoneOnly` | Call we left to the phone | Not needed | WA-only |
| `Call` | Call log entry, local and phone | Call log exists, calls out of scope | Partial, log only |

### 2.3 Actions and receipts

| Feature | WhatsApp today | Telegram | Verdict |
| --- | --- | --- | --- |
| Reply quote | `Quoted` | Reply to message | Both |
| Edit | Content plus mentions, about 15 minutes | 48 hours, edit history | Both, window differs |
| Delete for me | Server-confirmed, barrier, pending queue | Local delete, no barrier needed | Partial, simpler model |
| Delete for everyone | Barrier plus aliases | Revoke | Both |
| Forward | Serial queue, limit | Forward, multiple destinations | Both |
| Reactions | One per person | Many per person, ordered, paid reactions | Partial, model must allow multiple |
| Pins | Message pins, local and synced | Pinned messages per chat | Both |
| Stars | Local stars on messages | Saved Messages folder | Partial, different mental model |
| Group receipts (`group_receipts`) | Per-recipient, least advanced | Read count, member list, per-user read max | Partial, archive shape differs |
| Read state sync (`read_sync`) | One private write at a time, backoff | Read max id, simpler | Partial |
| Forward limit | Server errors | Server errors | Both |
| Link previews | Fetched locally (`link_preview.rs`) | Server-provided, can be disabled per message | Partial, source differs |

### 2.4 Product surfaces

| Surface | WhatsApp today | Telegram | Verdict |
| --- | --- | --- | --- |
| Calls, group calls, call links | `voip()` stack, call log, tones | Different signaling; libtgvoip/WebRTC | Out of scope for v1; reuse `call_audio` and call UI only |
| App lock | Local PBKDF2 verifier | Provider-agnostic | Both |
| Locked chats | Server chat lock | No equivalent | WA-only |
| Export | Local ZIP or text | Local ZIP or text | Both, same code shape |
| Search | Local archive, accent-insensitive | Local plus server search | Both, server search is extra |
| Picker: emoji | Local set plus platform emoji | Same | Both |
| Picker: GIFs | Giphy key | Telegram's own GIF search | Partial, two sources |
| Picker: stickers | Phone packs, favourites | Telegram packs, recent, favourites, custom emoji | Partial |
| Notifications, badge, tray | Per-account routing | Same, plus provider in routing | Both |
| Theme, wallpaper | fastframe-theme, Omarchy | Provider-agnostic | Both |
| Proxy | HTTP/SOCKS, `ALL_PROXY` | MTProto proxy is a different protocol | Partial, new proxy kind |
| Updates | GitHub releases | Provider-agnostic | Both |

### 2.5 Honest differences to document

- WhatsApp chats are end-to-end encrypted by default. Telegram cloud chats are
  not; only secret chats and one-to-one calls are. The site must say so, or users will
  assume parity.
- Telegram is a full client, not a companion device. Logging out ends the
  session; there is no phone to keep as the source of history.
- Telegram exposes usernames, bots, topics, comments, albums, scheduled
  messages and server drafts. None of these shapes exist in `model.rs` today.
- Telegram file references expire. A downloaded file is the only durable copy,
  which is closer to WhatsApp's model than it looks, but the worker must
  re-fetch references rather than trust stored ids.

## 3. Spike procedure

Throwaway code. Do not merge it. Timebox: three days.

The code lives outside the repository at `.cache/tg-spike/`, with its own
`Cargo.toml` and lockfile, so neither ZapFast's manifest nor its lockfile
changes. `.cache/` is ignored and disposable.

1. Get an `api_id` and `api_hash` at <https://my.telegram.org/auth>: sign in
   with the account's phone, open **API development tools**, create an
   application (any title, platform Desktop), and copy the pair. The pair
   identifies the application; the account used to log in can be a different
   one. Use a throwaway account for the
   spike where possible, so the pair and the session are disposable.
   Keep both in the environment, never in the repository:
   `TG_API_ID`, `TG_API_HASH`.
2. Run it:

   ```sh
   cd .cache/tg-spike
   TG_API_ID=12345 TG_API_HASH=... cargo run
   ```

   It connects, signs in (login code and 2FA if set), prints dialog count and
   the first ten dialogs, asks which one to use, pages ten messages of it,
   downloads the first media it finds to `spike-media.bin` beside it, sends
   one text message there, and waits up to 60 seconds for a live update.

   Choose a chat you control that has a photo in its last ten messages. While
   it waits, send a message to that chat from another client to exercise the
   live update.
3. Watch for friction the plan must absorb: peer reference resolution
   (`to_ref` can return `None`), access hashes, update fan-out, flood waits,
   media download API, and how much of the dialog title and unread data is
   available without extra requests.
4. Delete `.cache/tg-spike/` afterwards, whatever the outcome. If the spike
   passes, M1 adds `grammers` to the real manifest behind the `telegram`
   feature; if it fails, nothing in the repository ever referenced it.

The spike uses `MemorySession`, so every run logs in again. Telegram flood
waits make repeated logins expensive; do not loop it. Persisting a session is
a separate question for M1 (`SqliteSession` behind `grammers-session`'s
`sqlite` feature), not something M0 needs to prove.

Telegram's test servers are not a shortcut. `grammers` 0.11 hardcodes the
production datacenters and exposes no sign-up call, so a fresh `99966...` test
number returns `SignUpRequired`. Test numbers are also shared and public. The
spike therefore logs in to production with a throwaway account.

Do not add Telegram auth to the real app during M0. The spike sends a real
message, so use a test account and a chat you control. Report pass or fail,
counts, timings and errors only; the dialog names and message text printed in
the terminal stay there.

## 4. Measurements

Record these before and after the spike so M1 can budget honestly. Run the
spike build in its own target directory; do not clean the shared one.

| Metric | How | Baseline |
| --- | --- | --- |
| Normal dependency packages | `cargo tree --locked --edges normal --prefix none` unique count | 598 |
| Locked packages | `grep -c '^name = ' Cargo.lock` | 861 |
| Spike dependency packages | `cargo tree --edges normal --prefix none` unique count in `.cache/tg-spike` | 72 |
| Spike locked packages | `grep -c '^name = ' .cache/tg-spike/Cargo.lock` | 115 |
| Spike build wall time | time a clean `cargo build --release` in `.cache/tg-spike` | not measured, `cargo check` only |
| Binary size delta | release build of the spike against a release build of the app | not measured yet |
| Idle RSS with a linked test account | demo or real run, `ps -o rss=` after a minute | measure |
| Cold start delta | `hyperfine` or time to first frame in the log | measure |

A delta above roughly 40 MB of binary, 30 percent of cold build time, or 60
new crates is a warning, not an automatic stop. Write down the number.

The measured dependency delta is 72 normal packages and 115 locked packages
for the spike, against 598 and 861 for the app. `cargo check` compiles; a
release build and its size are still to be measured after the spike runs.

## 5. Kill criteria

Stop and record why if any of these holds after the spike:

- History paging or media download is unreliable, or requires reimplementing
  pieces of MTProto.
- Login cannot complete without a captcha or anti-bot step the app cannot
  present.
- No legitimate `api_id` route exists that fits shipping.
- The dependency drags in C++ or a platform toolchain the three builds cannot
  share.
- The capability matrix shows that more than half of the interface would need
  a second implementation for the first release.

Partial success is normal. What matters is whether text, media, groups and
notifications are each clearly possible without touching the WhatsApp paths.

## 6. Decision record

Fill the evidence and outcome after the spike. Keep it short.

```text
Date:                2026-11-02
Decision:            B, separate page first (1.1)
api_id route:        user supplied at first Telegram link
Protocol client:     grammers 0.11.0 (spike)
Evidence:            spike compiles and cargo check passes; dependency counts
                     in section 4; runtime evidence pending
Kill criteria hit:   not run
Consequences:        AGENTS.md, PRODUCT.md and the site gain a two-provider
                     boundary after the spike passes, not before
Next milestone:      run the spike (section 3), then M1 or stop
```

Decision notes:

- B means the first release has separate Telegram list and conversation
  (`PLAN.md` M3A). The unified inbox (M3B) is not in scope unless revisited
  with evidence from M3A.
- User-supplied credentials mean the first Telegram link asks for an
  `api_id`/`api_hash`, with a short explanation and a link to
  `my.telegram.org`. They are stored per account, never logged, never sent
  anywhere but Telegram. No pair ships in the binary.
- The boundary documents are edited in M1, after the spike, so a failed spike
  leaves the project unchanged.

## 7. What is already decided

These do not need the spike, and they are the first commits if M1 happens:
`Provider`, `ChatKey`, the `provider` field in `accounts.json` with a serde
default, provider-prefixed ids, separate `telegram.db`, and the disabled
`telegram` feature. Nothing else.
