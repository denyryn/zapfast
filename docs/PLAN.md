# Plan: a second provider (Telegram)

Status: approved for B, separate page first (2026-11-02). Telegram
credentials are user-supplied. The spike is pending. Code starts at M1 only
if the spike passes.

Scope note: B stops before M3B (unified inbox). The first release is M3A, a
text-first Telegram page, hardened and shipped by M5; M4 (media, groups) is
the next release. The unified inbox is described for a later decision only.

This plan adds Telegram beside WhatsApp. It assumes the answer to "will it
work" is yes for core messaging, not for parity, and that the expensive part
is the archive and the interface, not the Telegram client.

Two constraints shape every milestone below:

- `AGENTS.md` says WhatsApp only, no bridges. That is a product boundary, not
  a limitation, so M0 is a decision, not a task.
- The app must stay one application. A separate Telegram build is the
  fallback if the unified path stalls (see M3A).

## Decisions taken (revisit only with evidence)

| # | Decision | Why |
| --- | --- | --- |
| D1 | Protocol client: `grammers` (pure Rust, crates.io), not TDLib | Fits the dependency-source rule, no C++/FFI, keeps the three-platform build. TDLib stays the option if reproducible voice/video calls become a goal |
| D2 | Dispatch with an `enum Provider` in the worker, not trait objects | `match` exhaustiveness finds every place that must decide; traits hide provider-only commands behind downcasts |
| D3 | One archive file per provider per account: `telegram.db` beside `archive.db` | No migration of the user's WhatsApp data, no risk of a bad backfill. Merge in memory for the unified list |
| D4 | Chat ids are provider-prefixed strings: `wa:` for existing ids, `tg:` for new ones | `ChatId` stays `String`; the 274 Actions and every query keep working. Existing WhatsApp ids must stay byte-identical |
| D5 | Capabilities, not `cfg`, in the model | The UI asks a chat what it supports. No provider checks inside drawing code |
| D6 | No automation, no bridging, no reading another account's messages | Unchanged product rule. Telegram support does not soften it |
| D7 | Calls are out of scope for v1 | New signaling, new media negotiation; only `call_audio` and the call UI tiles are reusable |

Resolved (2026-11-02): the user supplies `api_id`/`api_hash` at the first
Telegram link, with a short explanation and a link to `my.telegram.org`. M1
adds the prompt and per-account storage; M2 uses it. Nothing is shipped in the
binary and nothing is logged.

## Target shape

```rust
enum Provider { WhatsApp, Telegram }

struct ChatKey {
    account: AccountId,
    provider: Provider,
    id: String, // "wa:<jid>" or "tg:<peer>"
}
```

- `accounts.json` gains `provider` per account (defaults to WhatsApp so an
  existing file loads unchanged).
- `Backend` becomes a thin handle over `enum Runtime { WhatsApp(Worker), Telegram(TgWorker) }`.
  The `Command`/`Event` channels, `Waker`, startup permit and shutdown stay
  shared.
- `src/tg/` holds the Telegram worker: auth, session, dialogs, messages,
  media, groups. It translates grammers types into `model.rs`, exactly as
  `backend/worker.rs` translates protobufs.
- `src/tg/archive/` reuses the table conventions of `src/archive/` on its own
  connection. Anything WhatsApp-specific (LID mapping, group receipt
  snapshots, poll secrets, deletion barriers) is not ported unless Telegram
  has the same semantics.

## Milestones

Each milestone ends with something runnable and a focused test, and every
commit builds on its own. `main` stays releasable throughout; Telegram lives
behind a build feature until M5.

### M0: Decision and spike (1 to 2 weeks)

Goal: know whether to spend the next months. Artifacts live in
[`TELEGRAM_M0.md`](TELEGRAM_M0.md).

- [x] Maintainer approves the boundary change in `AGENTS.md`, `PRODUCT.md`
      and the site's "What is ZapFast" page, or declines it. Stop here if
      declined. Approved for B on 2026-11-02; the documents change in M1.
- [x] Resolve the `api_id` question stated above. User-supplied at first Telegram link.
- [x] Spike code written and compiling against `grammers-client` 0.11.0 in
      the throwaway crate `.cache/tg-spike/`: login, dialogs, history, one
      media download, one send, one live update.
- [ ] Run the spike with a test number and its credentials. Runbook in
      `TELEGRAM_M0.md` §3.
- [x] Write the capability matrix: for every `Content` variant and every
      right-side panel section, WhatsApp only, Telegram only, both.
      `TELEGRAM_M0.md` §2.
- [x] Measure protocol and baseline: dependency count recorded, build and
      binary measurements defined. `TELEGRAM_M0.md` §4.
- [x] Kill criteria and decision record template. `TELEGRAM_M0.md` §5, §6.

Exit criteria: a written decision, a capability matrix, and a measured cost.
Kill criteria: grammers cannot page history or download media reliably, or
the `api_id` cannot be obtained legitimately. Stop and record why.

### M1: Provider foundation (3 to 4 weeks)

Goal: two account kinds exist in the data model and in the process, with no
Telegram UI yet.

- [ ] `Provider` enum and `ChatKey` in `model.rs`; `ChatId` stays `String`
      with the prefix (D4). Test: an existing WhatsApp id parses unchanged.
- [ ] `accounts.json` gains `provider` with a serde default; older files load.
      Test: fixture file from 0.21.0.
- [ ] `Backend` owns `enum Runtime`; `Command`/`Event` unchanged. WhatsApp
      behaviour must not change: run the existing suite.
- [ ] `paths.rs`: `state/accounts/<id>/telegram.db` and
      `cache/accounts/<id>/tg/`. Reuse the keyring scheme; its identity is
      already path-derived.
- [ ] Archive opens `telegram.db` with the same SQLCipher rules, separate key
      entry, same refusal to fall back to plaintext. Test: locked key stops
      the Telegram runtime only, never WhatsApp.
- [ ] `--demo` gains a provider field on its sample accounts, still WhatsApp
      only. Headless layout test still passes.
- [ ] Feature gate `telegram` in `Cargo.toml`, off by default.

Exit criteria: app starts, links WhatsApp as before, and a stub Telegram
account shows an empty list without touching the WhatsApp archive. No
Telegram dependency is compiled in a default build.

### M2: Telegram runtime, text only (6 to 8 weeks)

Goal: a real Telegram account that lists chats and sends and receives text.

- [ ] `src/tg/session.rs`: `grammers` session persisted in `session.db` under
      the account folder, encrypted at rest by the same keyring scheme.
- [ ] Auth: phone number, login code, 2FA password, error paths, logout. No
      QR (Telegram's QR login is a different flow; add later if asked).
- [ ] `src/tg/worker.rs`: startup permit, reconnect with backoff, sleep/wake
      through fastframe-power, one runtime per account.
- [ ] Dialogs to `Chat`; messages to `Message`; users and groups to
      `Contact`/`ChatKind`. The translation boundary is `src/tg/convert.rs`.
- [ ] Send text, reply quote, edit, delete-for-me, forward. Mark read.
      Typing and presence.
- [ ] History paging on scroll, with `meta` cursors. Do not invent a
      `messages.raw` blob: store what Telegram gives and re-fetch references
      when they expire.
- [ ] Read receipts and unread counts; mute state.
- [ ] `Command::*` plumbing only for what this milestone needs. Add events
      rather than overloading WhatsApp ones.

Exit criteria: two accounts in one window, text conversations both ways,
restart keeps the session and history, no WhatsApp path changed. Focused
tests for auth state, id mapping, paging and reconnect.

### M3A: Separate Telegram page (4 to 5 weeks) [recommended first release]

Goal: a usable Telegram client in weeks, with no unified-inbox refactor.

- [ ] `ui/tg_chats.rs` and `ui/tg_conversation.rs` reuse `widgets`, `theme`,
      `markup`, `image_cache`, `animation`, `video` and `audio`. They do not
      reuse `ui/chats.rs` or `ui/conversation.rs`.
- [ ] Rail: one entry per provider with a linked account. The account switcher
      groups by provider.
- [ ] Notifications, badge and tray carry `provider` in their routing.
- [ ] Capability flags hide WhatsApp-only surfaces (polls, disappearing,
      interactive, communities) in this page.
- [ ] Demo pages for the Telegram list and conversation; a headless layout
      test for both.

Exit criteria: link, read, send text, notifications, restart, no WhatsApp
regression. Media and groups are M4, not this release: the page says so
instead of drawing bubbles it cannot render. This is the point to decide
whether the unified inbox is still worth it.

### M3B: Unified inbox (6 to 10 weeks, not approved, only after M3A)

Goal: one list and one conversation, provider transparent to the reader.

- [ ] Merge chat lists by last activity; search across both; unread totals.
- [ ] `ui/conversation.rs` reads capabilities. Every `Action` that is
      provider-specific checks the flag, and unsupported controls are absent,
      not disabled.
- [ ] Chips, labels, pins and archive behave per provider; no cross-provider
      assumptions.
- [ ] Drafts, read state, notification withdrawal and export route by provider.
- [ ] One `AccountId` can own both runtimes; the account switcher shows one
      identity with two protocols, or two entries (choose in M3B kickoff).
- [ ] Accessibility labels name the provider where the difference
      matters; no extra chrome otherwise.

Exit criteria: a reader cannot tell which provider a chat is on except by its
features; no WhatsApp regression; full test matrix green.

### M4: Media and groups (4 to 6 weeks, after M3A in scope B)

Goal: the rest of what people actually use.

- [ ] Photos, albums, documents, voice, video, GIFs, stickers (TGS through
      `rasterlottie`, WebM through the existing video path), link previews.
- [ ] Upload and download progress, retry, file-reference expiry with a clear
      bubble message, size limits per provider.
- [ ] Groups: metadata, members, add/remove, leave, permissions, invite links.
      Channels: read-only posting rules, admin rights.
- [ ] Reactions (multiple, ordered), pins, forwarding to multiple chats.
- [ ] Local export for Telegram chats, or an explicit "not supported" message.

Exit criteria: media and group tests per provider; demo data for each media
kind; download failures never toast, they live in the bubble.

### M5: Hardening and release (4 weeks)

Goal: ship it, or shelve it with evidence.

- [ ] Security review: session at rest, token handling, no content in logs,
      link safety extended to `t.me` and Telegram invite links.
- [ ] Soak: two accounts, one week, reconnect and sleep/wake cycles.
- [ ] Demo build, screenshots and tour for Telegram, synthetic content only.
- [ ] i18n: new strings go through `gettext`; the maintainer regenerates
      catalogs.
- [ ] Docs site: what works, what does not, account risk, the `api_id` story.
- [ ] Packaging: binary size, Windows/macOS/Linux builds, optional Nix.
- [ ] Release notes in the house style; note the boundary change explicitly.

Exit criteria: full check matrix on all three platforms, honest limitations
list, no WhatsApp regression.

### Deferred, explicitly

Calls and group calls (needs new signaling; reuse `call_audio` and call UI
only), secret chats, topics UI beyond a flat chat, custom emoji, stories,
Telegram Business features, bridging to other networks, and any automation.

## Effort summary

| Milestone | Weeks, one developer who knows the code |
| --- | --- |
| M0 | 1 to 2 |
| M1 | 3 to 4 |
| M2 | 6 to 8 |
| M3A | 4 to 5 |
| M4 | 4 to 6 |
| M3B | 6 to 10 |
| M5 | 4 |
| Total to M3A release, without M4 | 18 to 23 |
| Total to M3A release, with M4 | 22 to 29 |
| Total to unified release | 28 to 39 |

Add 30 to 50 percent if `grammers` cannot cover media and history cleanly, or
if the capability matrix turns out to have more "both" entries than expected.

## Work plan rules

- Trunk based, one topic per commit, every commit compiles and passes the
  focused checks. No feature branches unless the maintainer asks.
- Each milestone leaves `main` releasable; the `telegram` feature stays off in
  release builds until M5.
- Focused tests per milestone, full matrix once per milestone, not per change.
- No new setting without approval. Provider choice is not a setting; it is
  what the account is.
- Do not touch `assets/i18n/` except in the translation step.
- Do not copy dependency source, do not point at a fork outside
  `github.com/crmne/`.

## Risks

| Risk | Impact | Mitigation |
| --- | --- | --- |
| Boundary change rejected or reversed | Whole plan wasted | M0 is a written decision with a stop |
| No legitimate `api_id` | Cannot ship | Resolve in M0 before any code |
| Archive semantics diverge more than expected | M2 and M4 slip | Keep `telegram.db` separate; never retrofit WhatsApp barriers |
| Capability flags leak into drawing code | UI becomes unreadable | One `Capabilities` type, checked in `apply`, not in widgets |
| Two protocol stacks bloat build and test time | Developer friction | Feature gate; measure in M0; optional per-provider CI lane |
| WhatsApp regressions from refactors | Trust loss | No behaviour change in M1; run the existing suite every milestone |
| Telegram terms or rate limits bite a client | Account risk | Same honesty as the WhatsApp warning; no automation, respect limits |

## First commit if approved

Only after M0: add `Provider`, `ChatKey`, the `provider` field in
`accounts.json` with a serde default, and the `telegram` feature gate, with
tests proving a 0.21.0 account file and every existing WhatsApp id are
unchanged. Nothing else.
