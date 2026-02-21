# StarostaBot

[![CI](https://github.com/Alkaness/discord-starosta-bot/actions/workflows/ci.yml/badge.svg)](https://github.com/Alkaness/discord-starosta-bot/actions/workflows/ci.yml)
[![Rust](https://img.shields.io/badge/rust-1.75%2B-orange.svg)](https://www.rust-lang.org/)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)
[![Discord](https://img.shields.io/badge/platform-Discord-5865F2.svg)](https://discord.com)

A Discord community bot built in Rust for managing Ukrainian servers. Features an XP/leveling system, economy, moderation tools, idea voting, and birthday tracking.

---

## Architecture

```
starosta_bot/
├── src/
│   └── main.rs              # Application entry point (monolith)
├── .github/
│   └── workflows/
│       └── ci.yml            # GitHub Actions CI pipeline
├── Cargo.toml                # Dependencies and project metadata
├── Cargo.lock                # Locked dependency versions
├── discloud.config           # Discloud deployment configuration
├── .env                      # Secrets (not committed)
├── .env.example              # Environment variable template
└── *.json                    # Runtime data files (not committed)
```

### Tech Stack

| Layer | Technology |
|-------|-----------|
| Language | Rust 2021 Edition |
| Discord Framework | [Poise](https://github.com/serenity-rs/poise) (built on Serenity) |
| Async Runtime | [Tokio](https://tokio.rs/) (multi-threaded) |
| Serialization | Serde + serde_json |
| Logging | tracing + tracing-subscriber |
| Data Storage | Flat JSON files with mutex-guarded in-memory state |

### Data Flow

```
Discord Gateway
    │
    ▼
Event Handler (messages, interactions, member joins)
    │
    ├── XP / Leveling ──► users.json
    ├── Moderation ────► banned_words.json
    ├── Suggestions ───► suggestions_data.json
    └── Birthdays ─────► birthdays.json

Background Tasks (tokio::select!)
    ├── Every 60s: Voice XP tick
    ├── Every 1h:  Birthday announcements
    └── Every 24h: Admin backup via DM
```

---

## Features

**Core Systems** -- XP and leveling with logarithmic scaling, economy (chips), XP boosters (x2/x5), anti-spam protection.

**Ideas and Suggestions** -- Designated channels where user messages become voteable embeds with approve/reject admin controls, auto-created discussion threads, and author editing via modals.

**Moderation** -- Text/voice/all muting, message purge, banned word filter with auto-delete, inactive role cleanup, auto-role on join.

**Economy** -- Daily rewards, casino, blackjack, XP booster shop.

**Birthdays** -- Automatic birthday tracking, role assignment, and server-wide announcements at 9 AM.

---

## Setup

### Prerequisites

- Rust (stable, 1.75+)
- A Discord bot token ([Developer Portal](https://discord.com/developers/applications))
- Your Discord user ID (enable Developer Mode, right-click profile, Copy ID)

### Installation

```bash
git clone https://github.com/Alkaness/discord-starosta-bot.git
cd discord-starosta-bot

cp .env.example .env
# Edit .env with your DISCORD_TOKEN and ADMIN_ID

cargo build --release
./target/release/rust_bot
```

### Environment Variables

| Variable | Description |
|----------|------------|
| `DISCORD_TOKEN` | Bot token from Discord Developer Portal |
| `ADMIN_ID` | Your Discord user ID (receives daily backups) |

---

## Commands

### User Commands

| Command | Description |
|---------|------------|
| `/help` | Show all available commands |
| `/rank [@user]` | View profile with XP, level, and progress |
| `/leaderboard` | Server leaderboard |
| `/daily` | Claim daily chip reward (24h cooldown) |
| `/casino <amount>` | Gamble chips |
| `/blackjack <bet>` | Play blackjack |
| `/shop` | View XP booster shop |
| `/buy_booster <x2\|x5>` | Purchase a 24h XP booster |
| `/set_birthday <day> <month>` | Set your birthday |
| `/birthdays` | View birthday calendar |
| `/poll <question>` | Create a vote |
| `/avatar [@user]` | Show user avatar |

### Admin Commands

| Command | Description |
|---------|------------|
| `/admin_set_level <user> <level>` | Set user level |
| `/admin_set_xp <user> <xp>` | Set user XP |
| `/admin_set_chips <user> <chips>` | Set user chips |
| `/admin_mute <user> <minutes> [type]` | Mute (text/voice/all) |
| `/admin_unmute <user>` | Unmute user |
| `/admin_announce <channel> <text>` | Send announcement |
| `/purge <amount>` | Delete messages (max 100) |
| `/clean` | Delete bot messages |
| `/setup_roles` | Create/update level roles |
| `/setup_autorole <role>` | Set auto-role for new members |
| `/suggest` / `/unsuggest` | Enable/disable ideas channel |
| `/add_banned_word <word>` | Add filtered word |
| `/cleanup_inactive <days>` | Strip roles from inactive users |

---

## XP and Leveling

XP is earned at **2 XP per message** and **10 XP per minute** in voice channels. Anti-spam prevents farming.

The leveling formula uses a power-logarithmic curve:

```
XP needed = 100 * (level + 1)^1.3 * ln(level + 2) + 100
```

| Level | XP Required |
|-------|------------|
| 0 to 1 | 169 |
| 5 to 6 | 2,098 |
| 10 to 11 | 5,957 |
| 20 to 21 | 16,822 |
| 37 to 38 | 41,055 |

Roles are unlocked at levels 0, 5, 10, 15, 20, 25, 30, 35, 40, 45, and 50.

---

## Deployment (Discloud)

The project includes `discloud.config` for deployment on [Discloud](https://discloudbot.com/):

```
NAME=StarostaBot
TYPE=bot
MAIN=src/main.rs
RAM=100
AUTORESTART=true
VERSION=latest
BUILD=cargo build --release
START=./target/release/rust_bot
```

1. Create a zip containing: `src/`, `Cargo.toml`, `Cargo.lock`, `discloud.config`, and any JSON data files
2. Set `DISCORD_TOKEN` and `ADMIN_ID` in Discloud environment variables
3. Upload the zip to Discloud

---

## CI/CD

GitHub Actions runs on every push and PR to `main`:

- **Check** -- `cargo check` for compilation errors
- **Format** -- `cargo fmt` enforcement
- **Clippy** -- Lint analysis with `-D warnings`
- **Build** -- Release build verification

See [`.github/workflows/ci.yml`](.github/workflows/ci.yml).

---

## Data Files

These JSON files are created at runtime and excluded from version control:

| File | Purpose |
|------|---------|
| `users.json` | User profiles (XP, level, chips, boosters) |
| `birthdays.json` | Birthday dates |
| `auto_roles.json` | Auto-role configuration per guild |
| `banned_words.json` | Filtered words list |
| `suggestions_channels.json` | Designated idea channel IDs |
| `suggestions_data.json` | Ideas, votes, and status tracking |

---

## License

[MIT](LICENSE)
