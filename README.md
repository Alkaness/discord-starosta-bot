#  StarostaBot - Discord Community Bot

A feature-rich Discord bot built with Rust for managing Ukrainian community servers with XP/leveling system, ideas management, tickets, birthdays, and more.

##  Features

### 🎮 Core Systems
- **XP & Leveling System** - Earn XP from messages and voice activity, level up to unlock roles
- **Economy System** - Collect chips through daily rewards, work, casino, and level-ups
- **XP Boosters** - Purchase temporary XP multipliers (x2, x5) from the shop
- **Anti-Spam Protection** - Automatic spam detection with temporary cooldowns

###  Ideas & Suggestions System
- Users write ideas in designated channels
- Automatic embed creation with voting buttons (👍 Like / 👎 Dislike)
- One vote per user, author cannot vote on their own ideas
- Admins can approve/reject ideas
- Authors can edit their suggestions via modal forms
- Vote tracking and percentage calculations

###  Birthday System
- Automatic birthday tracking and notifications
- Special birthday role assignment
- Sorted calendar view with month names
- Admin commands for managing birthdays

###  Moderation Tools
- Mute system (text/voice/all channels)
- Message cleanup commands
- Banned words filter with automatic deletion
- Role management for inactive users
- Auto-role assignment on member join

###  Economy Features
- Casino gambling system
- Daily rewards (24h cooldown)
- Work command for earning chips
- XP booster shop (x2 and x5 multipliers)

##  Quick Setup

### Prerequisites
- Rust (latest stable version)
- Discord Bot Token
- Your Discord User ID (for admin commands)

### Installation Steps

1. **Clone or download this repository**

2. **Configure environment variables**
   
   Edit the `.env` file and add your credentials:
   ```env
   DISCORD_TOKEN=your_discord_bot_token_here
   ADMIN_ID=your_user_id_here
   ```

   How to get these values:
   - **Bot Token**: Go to [Discord Developer Portal](https://discord.com/developers/applications), create/select your application, go to "Bot" section, and copy the token
   - **Admin ID**: Enable Developer Mode in Discord (User Settings > Advanced > Developer Mode), right-click your profile, and select "Copy ID"

3. **Build the bot**
   ```bash
   cargo build --release
   ```

4. **Run the bot**
   ```bash
   ./target/release/rust_bot
   ```

##  Commands List

### User Commands
- `/help` - Display all available commands
- `/info` - Bot information and statistics
- `/profile [@user]` - View user profile with XP and level
- `/rank` - Show XP leaderboard
- `/daily` - Claim daily chips reward
- `/work` - Work to earn chips
- `/casino <amount>` - Gamble chips in casino
- `/shop` - View XP booster shop
- `/buy_booster <type>` - Purchase XP booster (x2 or x5)
- `/birthdays` - View birthday calendar

### Admin Commands
- `/admin_set_level <user> <level>` - Set user level
- `/admin_set_xp <user> <xp>` - Set user XP
- `/admin_set_chips <user> <chips>` - Set user chips
- `/admin_mute <user> <duration> [type]` - Mute user (text/voice/all)
- `/admin_cleanup [amount]` - Delete bot messages
- `/admin_add_birthday <user> <date>` - Add birthday (format: DD.MM)
- `/admin_remove_birthday <user>` - Remove birthday
- `/clean_roles` - Remove roles from inactive users
- `/suggest` - Set current channel for ideas
- `/unsuggest` - Disable ideas in current channel

##  Key Features Explained

### Ideas System Workflow
1. Admin sets up ideas channel with `/suggest`
2. Users post their ideas in that channel
3. Bot automatically converts messages to embeds with voting buttons
4. Community votes with 👍 (Like) or 👎 (Dislike)
5. Admins can approve ✅ or reject ❌ ideas
6. Authors can edit ✏️ their ideas using modal forms

### XP & Leveling
- Earn **2 XP** per message (with anti-spam protection)
- Earn **10 XP** per minute in voice channels
- Level up formula: `level * 100 XP` required for next level
- Unlock roles at specific levels
- XP boosters multiply gains temporarily

##  Deployment on Discloud

### Configuration
The bot includes `discloud.config` for easy deployment:
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

### Deployment Steps
1. Create a zip file with all project files
2. Make sure `.env` is configured with your credentials
3. Upload to Discloud
4. Bot will automatically build and start

##  Data Files

The bot automatically creates and manages these JSON files:
- `users.json` - User profiles (XP, level, chips, boosters)
- `birthdays.json` - Birthday tracking
- `auto_roles.json` - Auto-role configuration
- `banned_words.json` - Filtered words list
- `suggestions_channels.json` - Ideas channel IDs
- `suggestions_data.json` - Ideas and votes tracking

## 🔧 Technical Details

- **Language**: Rust 2021 Edition
- **Framework**: Poise (Discord bot framework)
- **Runtime**: Tokio (async runtime)
- **Serialization**: Serde + serde_json
- **Logging**: Tracing + tracing-subscriber

### Dependencies
- `poise` - Discord bot framework
- `tokio` - Async runtime
- `serde` / `serde_json` - Data serialization
- `rand` - Random number generation
- `chrono` - Date and time handling
- `regex` - Regular expressions

##  Security & Best Practices

-  Token stored in environment variables (not in code)
-  Admin ID configurable via environment
-  Anti-spam protection with cooldowns
-  Permission checks on all admin commands
-  Automatic data backups sent to admin daily
-  Error handling and logging

##  Support & Contribution

For issues, questions, or contributions, please create an issue or pull request in this repository.

##  License

This project is provided as-is for community use. Feel free to modify and adapt it to your server's needs.

---

**Version**: 2.0  
**Status**: Production Ready ✅  
**Made with**: 🦀 Rust + ❤️
