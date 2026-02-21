use poise::serenity_prelude as serenity;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::time;
use chrono::{Datelike, Local, NaiveDate, Utc, Timelike};
use rand::Rng;
use serenity::{
    CreateActionRow, CreateButton, ButtonStyle, CreateEmbed, CreateEmbedFooter,
    CreateMessage, CreateAttachment, EditRole, Color, GetMessages, ChannelId, EditMember, Timestamp
};
use tracing::{info, warn, error};
use regex::Regex;

// --- НАЛАШТУВАННЯ ---
// Токен та Admin ID з змінних оточення
fn get_token() -> String {
    std::env::var("DISCORD_TOKEN")
        .expect("DISCORD_TOKEN must be set in environment variables")
}

fn get_admin_id() -> u64 {
    std::env::var("ADMIN_ID")
        .expect("ADMIN_ID must be set in environment variables")
        .parse::<u64>()
        .expect("ADMIN_ID must be a valid number")
}

const USERS_FILE: &str = "users.json";
const BIRTHDAY_FILE: &str = "birthdays.json";
const AUTO_ROLES_FILE: &str = "auto_roles.json";
const BANNED_WORDS_FILE: &str = "banned_words.json";
const SUGGESTIONS_CHANNELS_FILE: &str = "suggestions_channels.json";
const SUGGESTIONS_DATA_FILE: &str = "suggestions_data.json";
const VOICE_XP_AMOUNT: u64 = 10;
const MSG_XP_AMOUNT: u64 = 2;
const BIRTHDAY_ROLE_NAME: &str = "誕生日 Іменинник 誕生日";



// --- СТРУКТУРИ ДАНИХ ---
fn default_chips() -> u64 { 100 }

#[derive(Debug, Serialize, Deserialize, Clone)]
struct UserProfile {
    xp: u64,
    level: u64,
    minutes: u64,
    #[serde(default)]
    last_daily: i64,
    #[serde(default = "default_chips")]
    chips: u64,
    
    // Бустери XP (timestamp закінчення дії)
    #[serde(default)]
    xp_booster_x2_until: i64,
    #[serde(default)]
    xp_booster_x5_until: i64,

    // Поля для анті-спаму (не зберігаються в JSON)
    #[serde(skip)]
    last_msg_time: i64,
    #[serde(skip)]
    spam_counter: u8,
    #[serde(skip)]
    spam_block_until: i64,
}


// Авто-роль при вході
#[derive(Debug, Serialize, Deserialize, Clone)]
struct AutoRole {
    guild_id: String,
    role_id: String,
}

// Структура для збереження ідей
#[derive(Debug, Serialize, Deserialize, Clone)]
struct SuggestionData {
    message_id: String,
    channel_id: String,
    author_id: String,
    author_name: String,
    content: String,
    status: String, // "pending", "approved", "rejected"
    votes_for: u32,
    votes_against: u32,
    #[serde(default)]
    voted_users: Vec<String>, // ID користувачів які проголосували (формат: "user_id:vote_type")
    timestamp: i64,
}

struct Data {
    users: Arc<Mutex<HashMap<String, UserProfile>>>,
    birthdays: Arc<Mutex<HashMap<String, String>>>,
    auto_roles: Arc<Mutex<Vec<AutoRole>>>,
    banned_words: Arc<Mutex<Vec<String>>>,
    suggestions_channels: Arc<Mutex<Vec<String>>>, // ID каналів для ідей
    suggestions_data: Arc<Mutex<HashMap<String, SuggestionData>>>, // message_id -> SuggestionData
}

type Error = Box<dyn std::error::Error + Send + Sync>;
type Context<'a> = poise::Context<'a, Data, Error>;

// --- КОНФІГУРАЦІЯ РОЛЕЙ ---
fn get_roles_config() -> Vec<(u64, &'static str, u32)> {
    vec![
        (0, "草 Дачник 草", 0x78B159),
        (5, "目 Сусід через паркан 目", 0x4E7F38),
        (10, "力 Тракторист 力", 0x3498DB),
        (15, "土 Агроном 土", 0x1ABC9C),
        (20, "牛 Зоотехнік 牛", 0xE67E22),
        (25, "蜂 Бджоляр 蜂", 0xF1C40F),
        (30, "長 Голова колгоспу 長", 0x9B59B6),
        (35, "金 Олігарх місцевий 金", 0xE91E63),
        (40, "城 Депутат райради 城", 0x2C3E50),
        (45, "仙 Мольфар 仙", 0x11806A),
        (50, "神 Дід Панас 神", 0xFFD700),
    ]
}

// --- ДОПОМІЖНІ ФУНКЦІЇ ---

/// Safely lock a mutex, recovering from poisoning instead of panicking.
fn safe_lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    match mutex.lock() {
        Ok(guard) => guard,
        Err(poisoned) => {
            warn!("⚠️ Mutex was poisoned, recovering...");
            poisoned.into_inner()
        }
    }
}

fn load_json<T: for<'a> Deserialize<'a> + Default>(path: &str) -> T {
    match fs::read_to_string(path) {
        Ok(data) => {
            match serde_json::from_str(&data) {
                Ok(parsed) => {
                    info!("✅ Завантажено файл: {}", path);
                    parsed
                }
                Err(e) => {
                    warn!("⚠️ Помилка парсингу {}: {}. Використовую значення за замовчуванням.", path, e);
                    T::default()
                }
            }
        }
        Err(e) => {
            info!("ℹ️ Файл {} не знайдено ({}), створюю новий.", path, e);
            T::default()
        }
    }
}

fn save_json<T: Serialize>(path: &str, data: &T) {
    match serde_json::to_string_pretty(data) {
        Ok(json) => {
            if let Err(e) = fs::write(path, json) {
                error!("❌ Не вдалося зберегти {}: {}", path, e);
            } else {
                info!("💾 Збережено: {}", path);
            }
        }
        Err(e) => {
            error!("❌ Помилка серіалізації {}: {}", path, e);
        }
    }
}

fn get_xp_needed(level: u64) -> u64 {
    // Power-logarithmic curve: combines exponential growth with log scaling.
    // Formula: 100 * (level + 1)^1.3 * ln(level + 2) + 100
    // Lv0→1: ~169 | Lv5→6: ~2098 | Lv10→11: ~5957 | Lv20→21: ~16822 | Lv37→38: ~41055
    let level_f = level as f64;
    let needed = 100.0 * (level_f + 1.0).powf(1.3) * (level_f + 2.0).ln() + 100.0;
    needed as u64
}

fn try_levelup(profile: &mut UserProfile) -> Option<u64> {
    let mut leveled_up = None;
    loop {
        let needed = get_xp_needed(profile.level);
        if profile.xp >= needed {
            profile.xp -= needed;
            profile.level += 1;
            leveled_up = Some(profile.level);
        } else {
            break;
        }
    }
    leveled_up
}

fn get_role_for_level(level: u64) -> Option<&'static str> {
    let roles = get_roles_config();
    let mut best_role = None;
    for (req_lvl, name, _) in roles {
        if level >= req_lvl {
            best_role = Some(name);
        }
    }
    best_role
}

async fn assign_role(ctx: &serenity::Context, guild_id: serenity::GuildId, user_id: serenity::UserId, level: u64) {
    let target_role_name = match get_role_for_level(level) {
        Some(name) => name,
        None => return,
    };

    if let Ok(roles) = guild_id.roles(&ctx.http).await {
        let target_role_id = roles.values().find(|r| r.name == target_role_name).map(|r| r.id);

        if let Some(add_id) = target_role_id {
            let member_res = guild_id.member(&ctx.http, user_id).await;
            if let Ok(member) = member_res {
                if !member.roles.contains(&add_id) {
                    let _ = member.add_role(&ctx.http, add_id).await;
                }
                let config = get_roles_config();
                for (_, name, _) in config {
                    if name != target_role_name {
                        if let Some(old_role) = roles.values().find(|r| r.name == name) {
                            if member.roles.contains(&old_role.id) {
                                let _ = member.remove_role(&ctx.http, old_role.id).await;
                            }
                        }
                    }
                }
            }
        }
    }
}

fn create_default_profile() -> UserProfile {
    UserProfile {
        xp: 0,
        level: 0,
        minutes: 0,
        last_daily: 0,
        chips: 100,
        xp_booster_x2_until: 0,
        xp_booster_x5_until: 0,
        last_msg_time: 0,
        spam_counter: 0,
        spam_block_until: 0
    }
}

// Функція для отримання активного множника XP
fn get_xp_multiplier(profile: &UserProfile) -> u64 {
    let now = Utc::now().timestamp();
    if now < profile.xp_booster_x5_until {
        5
    } else if now < profile.xp_booster_x2_until {
        2
    } else {
        1
    }
}

// --- КОМАНДИ ---

/// 📚 Показати всі доступні команди
#[poise::command(slash_command)]
async fn help(ctx: Context<'_>) -> Result<(), Error> {
    
    let embed = CreateEmbed::new()
        .title("📚 Довідка по боту StarostaBot")
        .description("**Привіт! Я — твій сільський помічник з автоматичним управлінням! 🌾**")
        .color(0x2ECC71)
        .field("👤 **Профіль і прогрес**", 
            "`/rank` — Твоя картка з рівнем і XP\n\
             `/leaderboard` — Топ учасників сервера\n\
             `/daily` — Отримай щоденну винагороду", 
            false)
        .field("🎰 **Розваги**", 
            "`/casino <сума>` — Випробуй удачу!\n\
             `/blackjack <ставка>` — Зіграй в блекджек\n\
             `/poll <питання>` — Створи голосування", 
            false)
        .field("🎂 **Дні народження**", 
            "`/set_birthday <день> <місяць>` — Вкажи свій ДН\n\
             `/birthdays` — Календар іменинників", 
            false)
        .field("🛒 **Магазин і бустери**", 
            "`/shop` — Магазин бустерів XP\n\
             `/buy_booster <тип>` — Купити бустер (x2 або x5)", 
            false)
        .field("💬 **Комунікація**", 
            "`/suggest <ідея>` — Запропонувати ідею", 
            false)
        .field("🛠️ **Утиліти**", 
            "`/avatar [@користувач]` — Показати аватар\n\
             `/info` — Інформація про бота", 
            false)
        .field("👮 **Адмін: Основне**", 
            "`/setup_roles` — Налаштувати ролі\n\
             `/admin_set_level/xp/chips` — Встановити рівень/XP/гривні\n\
             `/admin_mute/unmute` — Мут/розмут (текст/голос/всюди)\n\
             `/admin_add/remove_birthday` — Керувати ДН\n\
             `/suggest` — Встановлення каналу для ідей\n\
             `/purge` — Видалити повідомлення\n\
             `/clean` — Видалити повідомлення бота\n\
             `/admin_announce` — Оголошення", 
            false)
        .field("🤖 **Адмін: Автоматизація**", 
            "`/setup_autorole` — Авто-роль для новачків\n\
             `/remove_autorole` — Видалити авто-роль\n\
             `/cleanup_inactive` — Очистити неактивних\n\
             `/add_banned_word` — Додати заборонене слово\n\
             `/remove_banned_word` — Видалити заборонене слово\n\
             `/list_banned_words` — Список заборонених слів", 
            false)
        .field("✨ **Автоматичні функції**", 
            "• 🎭 Авто-роль для нових учасників\n\
             • 👋 Привітання новачків\n\
             • 🚫 Автоматична модерація лайок\n\
             • 🔄 Анті-спам система\n\
             • 🚀 Бустери XP для прискорення прогресу", 
            false)
        .footer(CreateEmbedFooter::new("💡 Пиши в голосових каналах для XP!"))
        .thumbnail("https://cdn.discordapp.com/emojis/1234567890.png");
    
    ctx.send(poise::CreateReply::default().embed(embed).ephemeral(true)).await?;
    Ok(())
}

/// ℹ️ Інформація про бота
#[poise::command(slash_command)]
async fn info(ctx: Context<'_>) -> Result<(), Error> {
    let guild_count = ctx.serenity_context().cache.guilds().len();
    let user_count = {
        let users = safe_lock(&ctx.data().users);
        users.len()
    };
    
    let embed = CreateEmbed::new()
        .title("ℹ️ Інформація про StarostaBot")
        .description("**Сільський бот для Discord серверів** 🌾")
        .color(0x3498DB)
        .field("📊 Статистика", 
            format!("Серверів: **{}**\nКористувачів: **{}**", guild_count, user_count), 
            true)
        .field("⚙️ Технології", 
            "Rust 🦀\nSerenity + Poise\nHosted on Discloud", 
            true)
        .field("🎯 Можливості", 
            "• Система рівнів і ролей\n\
             • XP за повідомлення та голос\n\
             • Ігри та казино\n\
             • Привітання з ДН\n\
             • Інструменти модерації", 
            false)
        .footer(CreateEmbedFooter::new("Створено з ❤️ для твоєї спільноти"))
        .timestamp(Timestamp::now());
    
    ctx.send(poise::CreateReply::default().embed(embed)).await?;
    Ok(())
}

/// 🏆 Таблиця лідерів
#[poise::command(slash_command)]
async fn leaderboard(ctx: Context<'_>) -> Result<(), Error> {
    ctx.defer().await?;
    
    let mut leaders: Vec<(String, u64, u64, u64)> = {
        let users = safe_lock(&ctx.data().users);
        users.iter()
            .map(|(id, p)| (id.clone(), p.level, p.xp, p.minutes))
            .collect()
    };
    
    // Сортуємо по рівню (спадаюче), потім по XP
    leaders.sort_by(|a, b| {
        b.1.cmp(&a.1).then(b.2.cmp(&a.2))
    });
    
    let mut description = String::new();
    let medals = ["🥇", "🥈", "🥉"];
    
    for (i, (user_id, level, xp, minutes)) in leaders.iter().take(10).enumerate() {
        let medal = if i < 3 { medals[i] } else { "🏅" };
        let role_name = get_role_for_level(*level).unwrap_or("Новачок");
        description.push_str(&format!(
            "{}**{}. <@{}>**\n└ Рівень: **{}** | XP: **{}** | Голос: **{}** год\n└ Звання: *{}*\n\n",
            medal, i + 1, user_id, level, xp, minutes / 60, role_name
        ));
    }
    
    if description.is_empty() {
        description = "Поки що немає активних користувачів 😔".to_string();
    }
    
    let embed = CreateEmbed::new()
        .title("🏆 Таблиця лідерів")
        .description(description)
        .color(0xFFD700)
        .footer(CreateEmbedFooter::new("💪 Продовжуй працювати, щоб потрапити в топ!"));
    
    ctx.send(poise::CreateReply::default().embed(embed)).await?;
    Ok(())
}

/// [ADMIN] Налаштувати всі ролі на сервері
#[poise::command(slash_command, default_member_permissions = "ADMINISTRATOR")]
async fn setup_roles(ctx: Context<'_>) -> Result<(), Error> {
    ctx.defer().await?;
    let guild = ctx.guild_id().ok_or("Not in a guild")?;
    let existing_roles = guild.roles(&ctx.http()).await?;

    let config = get_roles_config();

    for (level, name, color_hex) in config {
        // Determine safe permissions based on level tier
        let permissions = if level < 10 {
            // Levels 0-9 (Basic): Basic chat and voice permissions
            serenity::Permissions::VIEW_CHANNEL
                | serenity::Permissions::SEND_MESSAGES
                | serenity::Permissions::READ_MESSAGE_HISTORY
                | serenity::Permissions::CONNECT
                | serenity::Permissions::SPEAK
                | serenity::Permissions::USE_VAD
        } else if level < 30 {
            // Levels 10-29 (Trusted): Basic + enhanced interaction permissions
            serenity::Permissions::VIEW_CHANNEL
                | serenity::Permissions::SEND_MESSAGES
                | serenity::Permissions::READ_MESSAGE_HISTORY
                | serenity::Permissions::CONNECT
                | serenity::Permissions::SPEAK
                | serenity::Permissions::USE_VAD
                | serenity::Permissions::CHANGE_NICKNAME
                | serenity::Permissions::ATTACH_FILES
                | serenity::Permissions::USE_EXTERNAL_EMOJIS
                | serenity::Permissions::ADD_REACTIONS
        } else {
            // Levels 30-50 (Elite): Trusted + advanced community features
            serenity::Permissions::VIEW_CHANNEL
                | serenity::Permissions::SEND_MESSAGES
                | serenity::Permissions::READ_MESSAGE_HISTORY
                | serenity::Permissions::CONNECT
                | serenity::Permissions::SPEAK
                | serenity::Permissions::USE_VAD
                | serenity::Permissions::CHANGE_NICKNAME
                | serenity::Permissions::ATTACH_FILES
                | serenity::Permissions::USE_EXTERNAL_EMOJIS
                | serenity::Permissions::ADD_REACTIONS
                | serenity::Permissions::USE_SOUNDBOARD
                | serenity::Permissions::PRIORITY_SPEAKER
                | serenity::Permissions::CREATE_PUBLIC_THREADS
                | serenity::Permissions::SEND_MESSAGES_IN_THREADS
        };

        if let Some(role) = existing_roles.values().find(|r| r.name == name) {
            // Update existing role with color, hoist, and safe permissions
            let _ = guild.edit_role(
                &ctx.http(), 
                role.id, 
                EditRole::new()
                    .colour(Color::from(color_hex))
                    .hoist(true)
                    .permissions(permissions)
            ).await;
        } else {
            // Create new role with color, hoist, and safe permissions
            let _ = guild.create_role(
                &ctx.http(), 
                EditRole::new()
                    .name(name)
                    .colour(Color::from(color_hex))
                    .hoist(true)
                    .permissions(permissions)
            ).await;
        }
    }

    // Create birthday role with basic permissions (no admin/moderation rights)
    if !existing_roles.values().any(|r| r.name == BIRTHDAY_ROLE_NAME) {
        let birthday_perms = serenity::Permissions::VIEW_CHANNEL
            | serenity::Permissions::SEND_MESSAGES
            | serenity::Permissions::READ_MESSAGE_HISTORY
            | serenity::Permissions::CONNECT
            | serenity::Permissions::SPEAK
            | serenity::Permissions::USE_VAD;
        
        let _ = guild.create_role(
            &ctx.http(), 
            EditRole::new()
                .name(BIRTHDAY_ROLE_NAME)
                .colour(0xFF69B4)
                .hoist(true)
                .permissions(birthday_perms)
        ).await;
    }

    ctx.say("✅ Всі ролі створено та пофарбовано з безпечними правами!").await?;
    Ok(())
}

/// [ADMIN] Встановити рівень користувачу
#[poise::command(slash_command, default_member_permissions = "ADMINISTRATOR")]
async fn admin_set_level(ctx: Context<'_>, user: serenity::User, level: u64) -> Result<(), Error> {
    let user_id = user.id.to_string();
    {
        let mut users = safe_lock(&ctx.data().users);
        let profile = users.entry(user_id).or_insert(create_default_profile());
        profile.level = level;
        save_json(USERS_FILE, &*users);
    }

    if let Some(guild_id) = ctx.guild_id() {
        assign_role(ctx.serenity_context(), guild_id, user.id, level).await;
    }

    ctx.say(format!("👮‍♂️ Адмін встановив рівень **{}** для користувача <@{}>.", level, user.id)).await?;
    Ok(())
}

/// [ADMIN] Змінити XP користувачу
#[poise::command(slash_command, default_member_permissions = "ADMINISTRATOR", rename = "admin_set_xp")]
async fn admin_set_xp(ctx: Context<'_>, user: serenity::User, xp: u64) -> Result<(), Error> {
    let user_id = user.id.to_string();
    {
        let mut users = safe_lock(&ctx.data().users);
        let profile = users.entry(user_id).or_insert(create_default_profile());
        profile.xp = xp;
        save_json(USERS_FILE, &*users);
    }

    ctx.say(format!("👮‍♂️ Адмін встановив **{} XP** для користувача <@{}>.", xp, user.id)).await?;
    Ok(())
}

/// [ADMIN] Змінити гривні користувачу
#[poise::command(slash_command, default_member_permissions = "ADMINISTRATOR", rename = "admin_set_chips")]
async fn admin_set_chips(ctx: Context<'_>, user: serenity::User, chips: u64) -> Result<(), Error> {
    let user_id = user.id.to_string();
    {
        let mut users = safe_lock(&ctx.data().users);
        let profile = users.entry(user_id).or_insert(create_default_profile());
        profile.chips = chips;
        save_json(USERS_FILE, &*users);
    }

    ctx.say(format!("👮‍♂️ Адмін встановив **{} гривень** 💰 для користувача <@{}>.", chips, user.id)).await?;
    Ok(())
}

/// [ADMIN] Замутити користувача
#[poise::command(slash_command, default_member_permissions = "MODERATE_MEMBERS")]
async fn admin_mute(
    ctx: Context<'_>, 
    user: serenity::User, 
    minutes: i64,
    #[description = "Тип мута: text (текст), voice (голос), all (всюди)"] mute_type: Option<String>,
    reason: Option<String>
) -> Result<(), Error> {
    let guild_id = ctx.guild_id().ok_or("Not in a guild")?;
    let mute_mode = mute_type.unwrap_or("all".to_string()).to_lowercase();
    
    let _member = guild_id.member(&ctx.http(), user.id).await?;
    
    match mute_mode.as_str() {
        "text" | "текст" => {
            // Мут тільки в текстових каналах
            let time_until = Timestamp::from_unix_timestamp(Utc::now().timestamp() + (minutes * 60))?;
            guild_id.edit_member(&ctx.http(), user.id, EditMember::new().disable_communication_until(time_until.to_string())).await?;
            ctx.say(format!("🔇 Користувача <@{}> замучено в **текстових каналах** на {} хв.\nПричина: {}", 
                user.id, minutes, reason.unwrap_or("Не вказана".to_string()))).await?;
        }
        "voice" | "голос" => {
            // Мут тільки в голосових каналах
            guild_id.edit_member(&ctx.http(), user.id, EditMember::new().mute(true)).await?;
            ctx.say(format!("🔇 Користувача <@{}> замучено в **голосових каналах** на {} хв.\nПричина: {}\n\n⚠️ Потрібно вручну розмутити після закінчення часу.", 
                user.id, minutes, reason.clone().unwrap_or("Не вказана".to_string()))).await?;
        }
        _ => {
            // Мут всюди (текст + голос)
            let time_until = Timestamp::from_unix_timestamp(Utc::now().timestamp() + (minutes * 60))?;
            guild_id.edit_member(&ctx.http(), user.id, 
                EditMember::new()
                    .disable_communication_until(time_until.to_string())
                    .mute(true)
            ).await?;
            ctx.say(format!("🔇 Користувача <@{}> замучено **всюди** на {} хв.\nПричина: {}\n\n⚠️ Текстовий мут автоматичний, голосовий потрібно зняти вручну.", 
                user.id, minutes, reason.unwrap_or("Не вказана".to_string()))).await?;
        }
    }
    
    Ok(())
}

/// [ADMIN] Розмутити користувача
#[poise::command(slash_command, default_member_permissions = "MODERATE_MEMBERS")]
async fn admin_unmute(ctx: Context<'_>, user: serenity::User) -> Result<(), Error> {
    let guild_id = ctx.guild_id().ok_or("Not in a guild")?;
    guild_id.edit_member(&ctx.http(), user.id, EditMember::new().enable_communication()).await?;
    ctx.say(format!("🔊 Користувача <@{}> розмучено.", user.id)).await?;
    Ok(())
}

/// [ADMIN] Відправити оголошення в канал
#[poise::command(slash_command, default_member_permissions = "ADMINISTRATOR")]
async fn admin_announce(ctx: Context<'_>, channel: ChannelId, text: String) -> Result<(), Error> {
    channel.say(&ctx.http(), text).await?;
    ctx.send(poise::CreateReply::default().content("✅ Оголошення надіслано.").ephemeral(true)).await?;
    Ok(())
}

/// [ADMIN] Видалити повідомлення
#[poise::command(slash_command, default_member_permissions = "MANAGE_MESSAGES")]
async fn purge(ctx: Context<'_>, amount: u64) -> Result<(), Error> {
    ctx.defer_ephemeral().await?;
    let count = if amount > 100 { 100 } else { amount };
    let messages = ctx.channel_id().messages(&ctx.http(), GetMessages::new().limit(count as u8)).await?;
    let msg_ids: Vec<_> = messages.iter().map(|m| m.id).collect();

    if !msg_ids.is_empty() {
        ctx.channel_id().delete_messages(&ctx.http(), &msg_ids).await?;
    }
    ctx.say(format!("🧹 Адмін видалив {} повідомлень.", msg_ids.len())).await?;
    Ok(())
}

/// [ADMIN] Видалити повідомлення бота
#[poise::command(slash_command, default_member_permissions = "MANAGE_MESSAGES")]
async fn clean(ctx: Context<'_>) -> Result<(), Error> {
    ctx.defer_ephemeral().await?;
    let messages = ctx.channel_id().messages(&ctx.http(), GetMessages::new().limit(100)).await?;
    let bot_id = ctx.framework().bot_id;
    let to_delete: Vec<_> = messages.iter().filter(|m| m.author.id == bot_id).map(|m| m.id).collect();

    if !to_delete.is_empty() {
        ctx.channel_id().delete_messages(&ctx.http(), &to_delete).await?;
    }
    ctx.say(format!("🧹 Видалено {} моїх повідомлень.", to_delete.len())).await?;
    Ok(())
}

/// 📊 Створити голосування
#[poise::command(slash_command)]
async fn poll(ctx: Context<'_>, question: String) -> Result<(), Error> {
    let embed = CreateEmbed::new()
        .title("📊 Голосування")
        .description(format!("**{}**", question))
        .colour(0xF1C40F)
        .footer(CreateEmbedFooter::new(format!("Автор: {}", ctx.author().name)));

    let msg = ctx.send(poise::CreateReply::default().embed(embed)).await?;
    let m = msg.message().await?;
    m.react(&ctx.http(), '👍').await?;
    m.react(&ctx.http(), '👎').await?;
    Ok(())
}

/// 🖼️ Показати аватар користувача
#[poise::command(slash_command)]
async fn avatar(ctx: Context<'_>, user: Option<serenity::User>) -> Result<(), Error> {
    let u = user.as_ref().unwrap_or_else(|| ctx.author());
    ctx.send(poise::CreateReply::default().embed(CreateEmbed::new().title(u.name.clone()).image(u.face()).colour(0x99AAB5))).await?;
    Ok(())
}

/// 📊 Переглянути профіль і статистику
#[poise::command(slash_command)]
async fn rank(ctx: Context<'_>, user: Option<serenity::User>) -> Result<(), Error> {
    let target = user.as_ref().unwrap_or_else(|| ctx.author());
    let (level, xp, minutes, chips) = {
        let users = safe_lock(&ctx.data().users);
        match users.get(&target.id.to_string()) {
            Some(p) => (p.level, p.xp, p.minutes, p.chips),
            None => (0, 0, 0, 100),
        }
    };
    let needed = get_xp_needed(level);
    let pct = ((xp as f64 / needed as f64) * 10.0) as usize;
    let bar = format!("{}{}", "🟩".repeat(pct.min(10)), "⬜".repeat(10 - pct.min(10)));
    let role_name = get_role_for_level(level).unwrap_or("Немає");

    ctx.send(poise::CreateReply::default().embed(CreateEmbed::new()
        .title(format!("Картка {}", target.name))
        .thumbnail(target.face())
        .fields(vec![
            ("Звання", role_name, false),
            ("Рівень", &level.to_string(), true),
            ("Гривні", &format!("🪙 {}", chips), true),
            ("XP", &format!("{}/{}", xp, needed), true),
            ("В голосі", &format!("{} год {} хв", minutes / 60, minutes % 60), true),
            ("Прогрес", &bar, false)
        ]).colour(0x006400))).await?;
    Ok(())
}

/// 🎁 Отримати щоденну винагороду
#[poise::command(slash_command)]
async fn daily(ctx: Context<'_>) -> Result<(), Error> {
    let user_id = ctx.author().id.to_string();
    let now = Utc::now().timestamp();

    let result = {
        let mut users = safe_lock(&ctx.data().users);
        let profile = users.entry(user_id.clone()).or_insert(create_default_profile());

        if now - profile.last_daily < 86400 {
            let wait = 86400 - (now - profile.last_daily);
            Err(wait)
        } else {
            let bonus = rand::thread_rng().gen_range(50..150);
            profile.chips += bonus;
            profile.last_daily = now;
            save_json(USERS_FILE, &*users);
            Ok(bonus)
        }
    };

    match result {
        Err(wait) => {
            ctx.send(poise::CreateReply::default().content(format!("⏳ Чекай **{} год {} хв**.", wait / 3600, (wait % 3600) / 60)).ephemeral(true)).await?;
        }
        Ok(bonus) => {
            ctx.say(format!("🎁 Ти отримав **{} гривень** 💰! Приходь завтра.", bonus)).await?;
        }
    }
    Ok(())
}

/// 🎰 Спробуй удачу в казино
#[poise::command(slash_command)]
async fn casino(ctx: Context<'_>, amount: u64) -> Result<(), Error> {
    let user_id = ctx.author().id.to_string();

    let calc_result = {
        let mut users = safe_lock(&ctx.data().users);
        let profile = users.entry(user_id).or_insert(create_default_profile());

        if profile.chips < amount || amount == 0 {
            None
        } else {
            if rand::thread_rng().gen_bool(0.45) {
                profile.chips += amount;
                Some((format!("🎰 Виграв **{} гривень**! 🤑", amount), true))
            } else {
                profile.chips = profile.chips.saturating_sub(amount);
                Some((format!("🎰 Програв **{} гривень**. 📉", amount), false))
            }
        }
    };

    match calc_result {
        None => {
            ctx.send(poise::CreateReply::default().content("❌ Недостатньо гривень або ставка 0.").ephemeral(true)).await?;
        }
        Some((msg, _)) => {
            {
                let users = safe_lock(&ctx.data().users);
                save_json(USERS_FILE, &*users);
            }
            ctx.say(msg).await?;
        }
    }
    Ok(())
}

/// 🃏 Зіграй у блекджек
#[poise::command(slash_command)]
async fn blackjack(ctx: Context<'_>, bet: u64) -> Result<(), Error> {
    let uid_str = ctx.author().id.to_string();

    let can_play = {
        let users = safe_lock(&ctx.data().users);
        let p = users.get(&uid_str);
        if p.is_none() || p.unwrap().chips < bet || bet == 0 { false } else { true }
    };

    if !can_play {
        ctx.send(poise::CreateReply::default().content("❌ Недостатньо гривень.").ephemeral(true)).await?;
        return Ok(());
    }

    let mut deck: Vec<u8> = vec![2,3,4,5,6,7,8,9,10,10,10,10,11].iter().cycle().take(52).cloned().collect();
    use rand::seq::SliceRandom;
    deck.shuffle(&mut rand::thread_rng());
    let (mut player, mut dealer) = (vec![deck.pop().unwrap(), deck.pop().unwrap()], vec![deck.pop().unwrap(), deck.pop().unwrap()]);
    fn calc(h: &Vec<u8>) -> u8 {
        let mut s: u16 = h.iter().map(|&x| x as u16).sum();
        let mut a = h.iter().filter(|&&x| x == 11).count();
        while s > 21 && a > 0 { s -= 10; a -= 1; }
        s as u8
    }
    let uuid = ctx.id();
    let (hit, stand) = (format!("{}h", uuid), format!("{}s", uuid));
    let make_embed = |p: &Vec<u8>, d: &Vec<u8>, hide: bool, t: &str, c: u32| {
        let dv = if hide { format!("[{}, ?]", d[0]) } else { format!("{:?} ({})", d, calc(d)) };
        CreateEmbed::new().title(t).colour(c).field(format!("Твоя ({})", calc(p)), format!("{:?}", p), true).field("Дилер", dv, true).footer(CreateEmbedFooter::new(format!("Ставка: {} гривень", bet)))
    };
    let btns = vec![CreateActionRow::Buttons(vec![CreateButton::new(&hit).label("Ще").style(ButtonStyle::Success), CreateButton::new(&stand).label("Все").style(ButtonStyle::Primary)])];
    let msg = ctx.send(poise::CreateReply::default().embed(make_embed(&player, &dealer, true, "🃏 Блекджек", 0x3498DB)).components(btns)).await?;
    let mut ended = false;
    let mut res = 0;

    while let Some(m) = msg.message().await?.await_component_interaction(&ctx.serenity_context().shard).timeout(Duration::from_secs(60)).await {
        if m.user.id != ctx.author().id { m.defer(&ctx.http()).await?; continue; }
        if m.data.custom_id == hit {
            player.push(deck.pop().unwrap_or(10));
            if calc(&player) > 21 { ended = true; res = -1; }
        } else if m.data.custom_id == stand {
            ended = true;
            while calc(&dealer) < 17 { if let Some(card) = deck.pop() { dealer.push(card); } else { break; } }
            let (ps, ds) = (calc(&player), calc(&dealer));
            if ds > 21 || ps > ds { res = 1; } else if ps < ds { res = -1; }
        }
        let (t, c) = if !ended { ("🃏 Блекджек", 0x3498DB) } else { match res { 1 => ("🎉 Перемога!", 0x2ECC71), -1 => ("📉 Програш", 0xE74C3C), _ => ("🤝 Нічия", 0xF1C40F) } };
        m.create_response(&ctx.http(), serenity::CreateInteractionResponse::UpdateMessage(serenity::CreateInteractionResponseMessage::new().embed(make_embed(&player, &dealer, !ended && res == 0, t, c)).components(if ended {vec![]} else {vec![CreateActionRow::Buttons(vec![CreateButton::new(&hit).label("Ще").style(ButtonStyle::Success), CreateButton::new(&stand).label("Все").style(ButtonStyle::Primary)])]}))).await?;
        if ended { break; }
    }
    if ended && res != 0 {
        let mut users = safe_lock(&ctx.data().users);
        let p = users.entry(uid_str).or_insert(create_default_profile());
        if res == 1 { p.chips += bet; } else { p.chips = p.chips.saturating_sub(bet); }
        save_json(USERS_FILE, &*users);
    }
    Ok(())
}

/// 🛒 Магазин бустерів XP
#[poise::command(slash_command, rename = "shop")]
async fn shop(ctx: Context<'_>) -> Result<(), Error> {
    let user_id = ctx.author().id.to_string();
    let (chips, x2_until, x5_until) = {
        let users = safe_lock(&ctx.data().users);
        match users.get(&user_id) {
            Some(p) => (p.chips, p.xp_booster_x2_until, p.xp_booster_x5_until),
            None => (100, 0, 0),
        }
    };
    
    let now = Utc::now().timestamp();
    let mut active_booster = "Немає активних бустерів".to_string();
    
    if now < x5_until {
        let hours_left = (x5_until - now) / 3600;
        active_booster = format!("🚀 **x5 XP бустер** (залишилось {} год)", hours_left);
    } else if now < x2_until {
        let hours_left = (x2_until - now) / 3600;
        active_booster = format!("⚡ **x2 XP бустер** (залишилось {} год)", hours_left);
    }
    
    let embed = CreateEmbed::new()
        .title("🛒 Магазин бустерів XP")
        .description(format!("**Твої гривні:** 💰 {}\n**Активний бустер:** {}", chips, active_booster))
        .color(0xF1C40F)
        .field("⚡ x2 XP Бустер", 
            "**Ціна:** 💰 2000 гривень\n**Тривалість:** 24 години\n**Ефект:** Подвоює отримання XP\n\nВикористовуй `/buy_booster x2`", 
            false)
        .field("🚀 x5 XP Бустер", 
            "**Ціна:** 💰 5000 гривень\n**Тривалість:** 24 години\n**Ефект:** Збільшує отримання XP в 5 разів!\n\nВикористовуй `/buy_booster x5`", 
            false)
        .footer(CreateEmbedFooter::new("💡 Бустери допоможуть швидше прокачатися!"));
    
    ctx.send(poise::CreateReply::default().embed(embed)).await?;
    Ok(())
}

/// 💳 Купити бустер XP
#[poise::command(slash_command, rename = "buy_booster")]
async fn buy_booster(ctx: Context<'_>, booster_type: String) -> Result<(), Error> {
    let user_id = ctx.author().id.to_string();
    
    let (price, multiplier, duration) = match booster_type.to_lowercase().as_str() {
        "x2" => (2000, 2, 86400),
        "x5" => (5000, 5, 86400),
        _ => {
            ctx.send(poise::CreateReply::default()
                .content("❌ Невірний тип бустера! Використовуй `x2` або `x5`.")
                .ephemeral(true)).await?;
            return Ok(());
        }
    };
    
    let result = {
        let mut users = safe_lock(&ctx.data().users);
        let profile = users.entry(user_id.clone()).or_insert(create_default_profile());
        
        if profile.chips < price {
            Err(format!("❌ Недостатньо гривень! Потрібно 💰 {}, а у тебе {}", price, profile.chips))
        } else {
            let now = Utc::now().timestamp();
            profile.chips -= price;
            let remaining_chips = profile.chips;
            
            if multiplier == 2 {
                profile.xp_booster_x2_until = now + duration;
            } else {
                profile.xp_booster_x5_until = now + duration;
            }
            
            save_json(USERS_FILE, &*users);
            Ok(format!("✅ Ти купив **x{} XP бустер** на 24 години!\n💰 Витрачено {} гривень. Залишок: {}", 
                multiplier, price, remaining_chips))
        }
    };
    
    match result {
        Ok(msg) => { ctx.say(msg).await?; }
        Err(msg) => { ctx.send(poise::CreateReply::default().content(msg).ephemeral(true)).await?; }
    }
    
    Ok(())
}

/// 🎂 Встановити свій день народження
#[poise::command(slash_command)]
async fn set_birthday(ctx: Context<'_>, day: u32, month: u32) -> Result<(), Error> {
    if NaiveDate::from_ymd_opt(2000, month, day).is_none() { ctx.say("❌ Дата не існує.").await?; return Ok(()); }
    let d = format!("{:02}.{:02}", day, month);
    {
        let mut b = safe_lock(&ctx.data().birthdays);
        b.insert(ctx.author().id.to_string(), d.clone());
        save_json(BIRTHDAY_FILE, &*b);
    }
    ctx.say(format!("✅ ДН встановлено: {}", d)).await?;
    Ok(())
}

/// 📅 Переглянути список днів народження
#[poise::command(slash_command)]
async fn birthdays(ctx: Context<'_>) -> Result<(), Error> {
    let birthdays_data = {
        let b = safe_lock(&ctx.data().birthdays);
        b.clone()
    };
    
    if birthdays_data.is_empty() {
        ctx.say("📅 Поки що немає збережених днів народження.").await?;
        return Ok(());
    }
    
    // Сортуємо дні народження по даті (місяць.день)
    let mut sorted: Vec<_> = birthdays_data.iter().collect();
    sorted.sort_by(|a, b| {
        let parse_date = |s: &str| -> (u32, u32) {
            let parts: Vec<&str> = s.split('.').collect();
            if parts.len() == 2 {
                let day = parts[0].parse::<u32>().unwrap_or(0);
                let month = parts[1].parse::<u32>().unwrap_or(0);
                (month, day)
            } else {
                (0, 0)
            }
        };
        
        let (month_a, day_a) = parse_date(a.1);
        let (month_b, day_b) = parse_date(b.1);
        
        month_a.cmp(&month_b).then(day_a.cmp(&day_b))
    });
    
    let mut description = String::new();
    let months = ["Січ", "Лют", "Бер", "Кві", "Тра", "Чер", "Лип", "Сер", "Вер", "Жов", "Лис", "Гру"];
    let total_count = sorted.len();
    
    for (user_id, date) in &sorted {
        let parts: Vec<&str> = date.split('.').collect();
        if parts.len() == 2 {
            let day = parts[0];
            let month_num = parts[1].parse::<usize>().unwrap_or(1);
            let month_name = if month_num > 0 { months.get(month_num - 1).unwrap_or(&"???") } else { &"???" };
            description.push_str(&format!("🎂 **{} {}** — <@{}>\n", day, month_name, user_id));
        }
    }
    
    let embed = CreateEmbed::new()
        .title("📅 Календар днів народження")
        .description(description)
        .color(0xFF69B4)
        .footer(CreateEmbedFooter::new(format!("Всього іменинників: {}", total_count)));
    
    ctx.send(poise::CreateReply::default().embed(embed)).await?;
    Ok(())
}

/// [ADMIN] Додати день народження користувачу
#[poise::command(slash_command, default_member_permissions = "ADMINISTRATOR", rename = "admin_add_birthday")]
async fn admin_add_birthday(ctx: Context<'_>, user: serenity::User, day: u32, month: u32) -> Result<(), Error> {
    if NaiveDate::from_ymd_opt(2000, month, day).is_none() { 
        ctx.say("❌ Дата не існує.").await?; 
        return Ok(()); 
    }
    
    let date = format!("{:02}.{:02}", day, month);
    {
        let mut b = safe_lock(&ctx.data().birthdays);
        b.insert(user.id.to_string(), date.clone());
        save_json(BIRTHDAY_FILE, &*b);
    }
    
    ctx.say(format!("✅ Адмін встановив ДН **{}** для користувача <@{}>", date, user.id)).await?;
    Ok(())
}

/// [ADMIN] Видалити день народження користувача
#[poise::command(slash_command, default_member_permissions = "ADMINISTRATOR", rename = "admin_remove_birthday")]
async fn admin_remove_birthday(ctx: Context<'_>, user: serenity::User) -> Result<(), Error> {
    let removed = {
        let mut b = safe_lock(&ctx.data().birthdays);
        let result = b.remove(&user.id.to_string());
        if result.is_some() {
            save_json(BIRTHDAY_FILE, &*b);
        }
        result.is_some()
    };
    
    if removed {
        ctx.say(format!("✅ День народження користувача <@{}> видалено!", user.id)).await?;
    } else {
        ctx.say(format!("❌ У користувача <@{}> немає збереженого дня народження.", user.id)).await?;
    }
    Ok(())
}

// --- СИСТЕМА ТІКЕТІВ ---


// --- СИСТЕМА ІДЕЙ ---

/// 💡 [ADMIN] Встановити канал для ідей
#[poise::command(slash_command, default_member_permissions = "ADMINISTRATOR", rename = "suggest")]
async fn setup_suggestions_channel(ctx: Context<'_>) -> Result<(), Error> {
    let channel_id = ctx.channel_id().to_string();
    
    {
        let mut channels = safe_lock(&ctx.data().suggestions_channels);
        if !channels.contains(&channel_id) {
            channels.push(channel_id.clone());
            save_json(SUGGESTIONS_CHANNELS_FILE, &*channels);
        }
    }
    
    ctx.send(poise::CreateReply::default()
        .content("✅ Цей канал тепер використовується для ідей!\nУсі повідомлення будуть автоматично перетворюватися на ідеї.")
        .ephemeral(true)).await?;
    
    Ok(())
}

/// 🚫 [ADMIN] Відключити канал для ідей
#[poise::command(slash_command, default_member_permissions = "ADMINISTRATOR", rename = "unsuggest")]
async fn remove_suggestions_channel(ctx: Context<'_>) -> Result<(), Error> {
    let channel_id = ctx.channel_id().to_string();
    
    let removed = {
        let mut channels = safe_lock(&ctx.data().suggestions_channels);
        let initial_len = channels.len();
        channels.retain(|c| c != &channel_id);
        let removed = initial_len != channels.len();
        if removed {
            save_json(SUGGESTIONS_CHANNELS_FILE, &*channels);
        }
        removed
    };
    
    if removed {
        ctx.send(poise::CreateReply::default()
            .content("✅ Цей канал більше не використовується для ідей.")
            .ephemeral(true)).await?;
    } else {
        ctx.send(poise::CreateReply::default()
            .content("❌ Цей канал не був налаштований для ідей.")
            .ephemeral(true)).await?;
    }
    
    Ok(())
}


// --- АВТОМАТИЧНЕ УПРАВЛІННЯ РОЛЯМИ ---

/// 🎭 [ADMIN] Налаштувати авто-ролі для нових користувачів
#[poise::command(slash_command, default_member_permissions = "ADMINISTRATOR")]
async fn setup_autorole(ctx: Context<'_>, role: serenity::Role) -> Result<(), Error> {
    let guild_id = ctx.guild_id().ok_or("Not in a guild")?;
    
    let auto_role = AutoRole {
        guild_id: guild_id.to_string(),
        role_id: role.id.to_string(),
    };
    
    {
        let mut roles = safe_lock(&ctx.data().auto_roles);
        // Видаляємо стару роль для цього серверу
        roles.retain(|r| r.guild_id != guild_id.to_string());
        // Додаємо нову
        roles.push(auto_role);
        save_json(AUTO_ROLES_FILE, &*roles);
    }
    
    ctx.say(format!("✅ Авто-роль встановлено: **{}**\nНові користувачі автоматично отримають цю роль!", role.name)).await?;
    Ok(())
}

/// 🗑️ [ADMIN] Відключити авто-ролі
#[poise::command(slash_command, default_member_permissions = "ADMINISTRATOR")]
async fn remove_autorole(ctx: Context<'_>) -> Result<(), Error> {
    let guild_id = ctx.guild_id().ok_or("Not in a guild")?;
    
    {
        let mut roles = safe_lock(&ctx.data().auto_roles);
        roles.retain(|r| r.guild_id != guild_id.to_string());
        save_json(AUTO_ROLES_FILE, &*roles);
    }
    
    ctx.say("✅ Авто-ролі відключено для цього сервера.").await?;
    Ok(())
}

// --- АВТОМАТИЧНА МОДЕРАЦІЯ ---

/// 🚫 [ADMIN] Додати заборонене слово
#[poise::command(slash_command, default_member_permissions = "ADMINISTRATOR")]
async fn add_banned_word(ctx: Context<'_>, word: String) -> Result<(), Error> {
    {
        let mut words = safe_lock(&ctx.data().banned_words);
        let word_lower = word.to_lowercase();
        if !words.contains(&word_lower) {
            words.push(word_lower);
            save_json(BANNED_WORDS_FILE, &*words);
        }
    }
    
    ctx.send(poise::CreateReply::default()
        .content(format!("✅ Слово додано до чорного списку!"))
        .ephemeral(true))
        .await?;
    Ok(())
}

/// 📋 [ADMIN] Список заборонених слів
#[poise::command(slash_command, default_member_permissions = "ADMINISTRATOR")]
async fn list_banned_words(ctx: Context<'_>) -> Result<(), Error> {
    let words = {
        let w = safe_lock(&ctx.data().banned_words);
        w.clone()
    };
    
    if words.is_empty() {
        ctx.send(poise::CreateReply::default()
            .content("📋 Чорний список порожній.")
            .ephemeral(true))
            .await?;
        return Ok(());
    }
    
    let list = words.join(", ");
    ctx.send(poise::CreateReply::default()
        .content(format!("🚫 **Заборонені слова:**\n{}", list))
        .ephemeral(true))
        .await?;
    Ok(())
}

/// 🗑️ [ADMIN] Видалити заборонене слово
#[poise::command(slash_command, default_member_permissions = "ADMINISTRATOR")]
async fn remove_banned_word(ctx: Context<'_>, word: String) -> Result<(), Error> {
    let removed = {
        let mut words = safe_lock(&ctx.data().banned_words);
        let word_lower = word.to_lowercase();
        let len_before = words.len();
        words.retain(|w| w != &word_lower);
        let removed = words.len() < len_before;
        if removed {
            save_json(BANNED_WORDS_FILE, &*words);
        }
        removed
    };
    
    if removed {
        ctx.send(poise::CreateReply::default()
            .content("✅ Слово видалено з чорного списку!")
            .ephemeral(true))
            .await?;
    } else {
        ctx.send(poise::CreateReply::default()
            .content("❌ Слово не знайдено в списку.")
            .ephemeral(true))
            .await?;
    }
    Ok(())
}

// --- АВТОМАТИЧНЕ ОЧИЩЕННЯ НЕАКТИВНИХ РОЛЕЙ ---

/// 🧹 [ADMIN] Видалити ролі з неактивних користувачів
#[poise::command(slash_command, default_member_permissions = "ADMINISTRATOR")]
async fn cleanup_inactive(ctx: Context<'_>, days: u64) -> Result<(), Error> {
    ctx.defer().await?;
    
    let guild_id = ctx.guild_id().ok_or("Not in a guild")?;
    let threshold = Utc::now().timestamp() - (days as i64 * 86400);
    
    let inactive_users: Vec<String> = {
        let users = safe_lock(&ctx.data().users);
        users.iter()
            .filter(|(_, p)| p.last_msg_time != 0 && p.last_msg_time < threshold * 1000)
            .map(|(id, _)| id.clone())
            .collect()
    };
    
    let mut removed_count = 0;
    
    for user_id_str in inactive_users {
        if let Ok(user_id_num) = user_id_str.parse::<u64>() {
            let user_id = serenity::UserId::new(user_id_num);
            if let Ok(member) = guild_id.member(&ctx.http(), user_id).await {
                let config = get_roles_config();
                for (_, role_name, _) in config {
                    if let Ok(roles) = guild_id.roles(&ctx.http()).await {
                        if let Some(role) = roles.values().find(|r| r.name == role_name) {
                            if member.roles.contains(&role.id) {
                                let _ = member.remove_role(&ctx.http(), role.id).await;
                                removed_count += 1;
                            }
                        }
                    }
                }
            }
        }
    }
    
    ctx.say(format!("🧹 Очищено ролі з {} неактивних користувачів (неактивні > {} днів).", removed_count, days)).await?;
    Ok(())
}

// --- ФОНОВІ ЗАВДАННЯ ---
async fn event_handler(ctx: &serenity::Context, event: &serenity::FullEvent, _framework: poise::FrameworkContext<'_, Data, Error>, data: &Data) -> Result<(), Error> {
    // Обробка нових учасників (авто-роль + привітання)
    if let serenity::FullEvent::GuildMemberAddition { new_member } = event {
        let guild_id = new_member.guild_id;
        
        // Привітання
        let system_channel = {
            guild_id.to_guild_cached(&ctx.cache)
                .and_then(|g| g.system_channel_id)
        };
        
        if let Some(system_channel) = system_channel {
            let embed = CreateEmbed::new()
                .title("🌾 Ласкаво просимо!")
                .description(format!("Вітаємо <@{}> на нашому сервері!\n\nПочни спілкуватися, щоб отримувати XP та підвищувати рівень!", new_member.user.id))
                .color(0x2ECC71)
                .thumbnail(new_member.user.face())
                .footer(CreateEmbedFooter::new("Використовуй /help для списку команд"));
            
            let _ = system_channel.send_message(&ctx.http, CreateMessage::new().embed(embed)).await;
        }
        
        // Авто-роль
        let role_to_assign = {
            let auto_roles = safe_lock(&data.auto_roles);
            auto_roles.iter()
                .find(|r| r.guild_id == guild_id.to_string())
                .and_then(|r| r.role_id.parse::<u64>().ok())
        };
        
        if let Some(role_id) = role_to_assign {
            let _ = new_member.add_role(&ctx.http, serenity::RoleId::new(role_id)).await;
            info!("✅ Авто-роль надано новому користувачу: {}", new_member.user.name);
        }
        
        return Ok(());
    }
    
    if let serenity::FullEvent::Message { new_message } = event {
        if new_message.author.bot { return Ok(()); }

        // Перевірка на заборонені слова
        let msg_lower = new_message.content.to_lowercase();
        let contains_banned = {
            let banned_words = safe_lock(&data.banned_words);
            let mut found = false;
            
            for word in banned_words.iter() {
                let pattern = format!(r"\b{}\b", regex::escape(word));
                if let Ok(re) = Regex::new(&pattern) {
                    if re.is_match(&msg_lower) {
                        found = true;
                        break;
                    }
                }
            }
            found
        };
        
        if contains_banned {
            let _ = new_message.delete(&ctx.http).await;
            let warning = new_message.channel_id.say(&ctx.http, 
                format!("🚫 <@{}>, використання забороненої лексики заборонено!", new_message.author.id)
            ).await;
            
            // Видаляємо попередження через 5 секунд
            if let Ok(w) = warning {
                let http = ctx.http.clone();
                let channel_id = new_message.channel_id;
                tokio::spawn(async move {
                    tokio::time::sleep(Duration::from_secs(5)).await;
                    let _ = channel_id.delete_message(&http, w.id).await;
                });
            }
            
            // Додаємо попередження користувачу
            if let Some(guild_id) = new_message.guild_id {
                let timeout_end = Timestamp::from_unix_timestamp(Utc::now().timestamp() + 300); // 5 хв мут
                if let Ok(ts) = timeout_end {
                    let _ = guild_id.edit_member(&ctx.http, new_message.author.id, 
                        EditMember::new().disable_communication_until(ts.to_string())).await;
                }
            }
            
            return Ok(());
        }
        
        // Кастомні команди видалено за запитом
        
        // Обробка повідомлень у каналах ідей
        let channel_id = new_message.channel_id.to_string();
        let is_suggestions_channel = {
            let channels = safe_lock(&data.suggestions_channels);
            channels.contains(&channel_id)
        };
        
        if is_suggestions_channel {
            // Перевіряємо чи це reply на повідомлення бота (для редагування ідеї)
            if let Some(ref replied_msg) = new_message.referenced_message {
                if replied_msg.author.id == ctx.cache.current_user().id {
                    // Це reply на повідомлення бота - перевіряємо чи це автор ідеї
                    let msg_id = replied_msg.id.to_string();
                    let author_id = new_message.author.id.to_string();
                    
                    let can_edit = {
                        let suggestions = safe_lock(&data.suggestions_data);
                        if let Some(suggestion) = suggestions.get(&msg_id) {
                            suggestion.author_id == author_id
                        } else {
                            false
                        }
                    };
                    
                    if can_edit {
                        let new_content = new_message.content.clone();
                        
                        // Видаляємо повідомлення користувача
                        let _ = new_message.delete(&ctx.http).await;
                        
                        // Оновлюємо ідею
                        let updated = {
                            let mut suggestions = safe_lock(&data.suggestions_data);
                            if let Some(suggestion) = suggestions.get_mut(&msg_id) {
                                suggestion.content = new_content.clone();
                                let cloned = suggestion.clone();
                                save_json(SUGGESTIONS_DATA_FILE, &*suggestions);
                                Some(cloned)
                            } else {
                                None
                            }
                        };
                        
                        if let Some(suggestion) = updated {
                            // Оновлюємо embed
                            let total = suggestion.votes_for + suggestion.votes_against;
                            let percent = if total > 0 { (suggestion.votes_for as f64 / total as f64 * 100.0) as u32 } else { 0 };
                            
                            let updated_embed = CreateEmbed::new()
                                .title(format!("💡 Ідея користувача @{}", suggestion.author_name))
                                .description(format!("**Ідея**\n{}", suggestion.content))
                                .color(0xF1C40F)
                                .field(format!("За: {} | Проти: {} | Процентів за: {}%", suggestion.votes_for, suggestion.votes_against, percent), "", false)
                                .field("Статус", "📊 | Чекаємо на відгук спільноти! Все у ваших руках", false)
                                .footer(CreateEmbedFooter::new("Хочете додати свою ідею? Просто напишіть її прямо сюди"));
                            
                            let _ = replied_msg.channel_id.edit_message(&ctx.http, replied_msg.id, serenity::EditMessage::new().embed(updated_embed)).await;
                        }
                        
                        return Ok(());
                    }
                }
            }
            
            // Перевіряємо чи це не адмін
            let is_admin = if let Some(guild_id) = new_message.guild_id {
                if let Ok(member) = guild_id.member(&ctx.http, new_message.author.id).await {
                    member.permissions(&ctx.cache).map(|p| p.administrator()).unwrap_or(false)
                } else {
                    false
                }
            } else {
                false
            };
            
            if !is_admin && new_message.author.id.get() != get_admin_id() {
                let author_id = new_message.author.id.to_string();
                let author_name = new_message.author.name.clone();
                let content = new_message.content.clone();
                let timestamp = Utc::now().timestamp();
                
                // Видаляємо оригінальне повідомлення
                let _ = new_message.delete(&ctx.http).await;
                
                // Створюємо embed з ідеєю
                let embed = CreateEmbed::new()
                    .title(format!("💡 Ідея користувача @{}", author_name))
                    .description(format!("**Ідея**\n{}", content))
                    .color(0xF1C40F)
                    .field("За: 0 | Проти: 0 | Процентів за: 0%", "", false)
                    .field("Статус", "📊 | Чекаємо на відгук спільноти! Все у ваших руках", false)
                    .footer(CreateEmbedFooter::new("Хочете додати свою ідею? Просто напишіть її прямо сюди"));
                
                // Створюємо кнопки
                let buttons = vec![
                    CreateActionRow::Buttons(vec![
                        CreateButton::new(format!("idea_like_{}", timestamp))
                            .label("Класнючка")
                            .style(ButtonStyle::Success)
                            .emoji('👍'),
                        CreateButton::new(format!("idea_dislike_{}", timestamp))
                            .label("Жах")
                            .style(ButtonStyle::Danger)
                            .emoji('👎'),
                    ]),
                    CreateActionRow::Buttons(vec![
                        CreateButton::new(format!("idea_approve_{}", timestamp))
                            .label("Прийняти ідею")
                            .style(ButtonStyle::Primary)
                            .emoji('✅'),
                        CreateButton::new(format!("idea_reject_{}", timestamp))
                            .label("Відхилити ідею")
                            .style(ButtonStyle::Danger)
                            .emoji('❌'),
                        CreateButton::new(format!("idea_edit_{}", timestamp))
                            .label("Змінити")
                            .style(ButtonStyle::Secondary)
                            .emoji('✏'),
                    ])
                ];
                
                let msg = new_message.channel_id.send_message(&ctx.http, 
                    CreateMessage::new().embed(embed).components(buttons)
                ).await;
                
                if let Ok(sent_msg) = msg {
                    //АВТОМАТИЧНЕ СТВОРЕННЯ ТРЕДУ ДЛЯ ОБГОВОРЕННЯ

                    let truncated: String = content.chars().take(50).collect();
                    let thread_name = if content.chars().count() > 50 {
                        format!("Обговорення: {}...", truncated)
                    } else {
                        format!("Обговорення: {}", content)
                    };

                    // Створюємо тред прикріплений до повідомлення з ідеєю
                    // ✅ Corrected Code
                    let thread_result = sent_msg.channel_id.create_thread_from_message(
                        &ctx.http,
                        sent_msg.id,
                        serenity::CreateThread::new(thread_name)
                            .auto_archive_duration(serenity::AutoArchiveDuration::ThreeDays)
                    ).await;

                    if let Ok(thread) = thread_result {
                        info!("✅ Створено тред для обговорення ідеї: {}", thread.id);

                        // Відправляємо привітальне повідомлення в тред
                        let _ = thread.send_message(&ctx.http,
                                                    CreateMessage::new()
                                                        .content(format!("💬 **Тут можна обговорити цю ідею!**\n\nАвтор: <@{}>\n\nПишіть свої думки, пропозиції та питання!", author_id))
                        ).await;
                    } else {
                        warn!("⚠️ Не вдалося створити тред для ідеї");
                    }

                    // Зберігаємо дані про ідею
                    let suggestion = SuggestionData {
                        message_id: sent_msg.id.to_string(),
                        channel_id: channel_id.clone(),
                        author_id: author_id.clone(),
                        author_name: author_name.clone(),
                        content: content.clone(),
                        status: "pending".to_string(),
                        votes_for: 0,
                        votes_against: 0,
                        voted_users: Vec::new(),
                        timestamp,
                    };
                    
                    let mut suggestions = safe_lock(&data.suggestions_data);
                    suggestions.insert(sent_msg.id.to_string(), suggestion);
                    save_json(SUGGESTIONS_DATA_FILE, &*suggestions);
                }
                
                return Ok(());
            }
        }

        let lvl;
        let now_millis = Utc::now().timestamp_millis();

        let mut punish_spam = false;

        {
            let mut users = safe_lock(&data.users);
            let p = users.entry(new_message.author.id.to_string()).or_insert(create_default_profile());

            if now_millis < p.spam_block_until {
                return Ok(());
            }

            let time_diff = now_millis - p.last_msg_time;

            if time_diff < 2000 {
                p.spam_counter += 1;
            } else {
                p.spam_counter = 0;
            }

            p.last_msg_time = now_millis;

            if p.spam_counter >= 5 {
                p.spam_block_until = now_millis + 30_000;
                p.spam_counter = 0;
                punish_spam = true;
            } else {
                let multiplier = get_xp_multiplier(p);
                p.xp += MSG_XP_AMOUNT * multiplier;
            }

            lvl = try_levelup(p);
        }

        if punish_spam {
            if let Some(g) = new_message.guild_id {
                let timeout_end = Timestamp::from_unix_timestamp(Utc::now().timestamp() + 30);
                if let Ok(ts) = timeout_end {
                    let _ = g.edit_member(&ctx.http, new_message.author.id, EditMember::new().disable_communication_until(ts.to_string())).await;
                    let _ = new_message.channel_id.say(&ctx.http, format!("🚫 <@{}>, не спам! Мут на 30 сек.", new_message.author.id)).await;
                }
            }
        } else if let Some(l) = lvl {
            let _ = new_message.channel_id.say(&ctx.http, format!("🎉 <@{}> апнув рівень **{}**!", new_message.author.id, l)).await;
            if let Some(g) = new_message.guild_id { assign_role(ctx, g, new_message.author.id, l).await; }
        }
    }
    
    // Обробка натискань кнопок та modal форм
    if let serenity::FullEvent::InteractionCreate { interaction } = event {
        // Обробка modal форм
        if let Some(modal_interaction) = interaction.as_modal_submit() {
            let custom_id = &modal_interaction.data.custom_id;
            
            if custom_id.starts_with("edit_idea_") {
                let msg_id = custom_id.strip_prefix("edit_idea_").unwrap_or("");
                
                // Отримуємо нову версію ідеї з modal
                let new_content = modal_interaction.data.components.get(0)
                    .and_then(|row| row.components.get(0))
                    .and_then(|component| {
                        match component {
                            serenity::ActionRowComponent::InputText(input) => {
                                if let Some(ref val) = input.value {
                                    if !val.is_empty() {
                                        Some(val.clone())
                                    } else {
                                        None
                                    }
                                } else {
                                    None
                                }
                            }
                            _ => None
                        }
                    });
                
                let new_content = if let Some(content) = new_content {
                    content
                } else {
                    modal_interaction.create_response(&ctx.http, serenity::CreateInteractionResponse::Message(
                        serenity::CreateInteractionResponseMessage::new()
                            .content("❌ Ідея не може бути порожньою!")
                            .ephemeral(true)
                    )).await?;
                    return Ok(());
                };
                
                // Оновлюємо ідею
                let updated = {
                    let mut suggestions = safe_lock(&data.suggestions_data);
                    if let Some(suggestion) = suggestions.get_mut(msg_id) {
                        suggestion.content = new_content.clone();
                        let cloned = suggestion.clone();
                        save_json(SUGGESTIONS_DATA_FILE, &*suggestions);
                        Some(cloned)
                    } else {
                        None
                    }
                };
                
                if let Some(suggestion) = updated {
                    // Оновлюємо повідомлення
                    let total = suggestion.votes_for + suggestion.votes_against;
                    let percent = if total > 0 { (suggestion.votes_for as f64 / total as f64 * 100.0) as u32 } else { 0 };
                    
                    let status_text = match suggestion.status.as_str() {
                        "approved" => "✅ | Крута ідея, інтегруєм!",
                        "rejected" => "❌ | До одного місця такі ідеї!",
                        _ => "📊 | Чекаємо на відгук спільноти! Все у ваших руках",
                    };
                    
                    let color = match suggestion.status.as_str() {
                        "approved" => 0x2ECC71,
                        "rejected" => 0xE74C3C,
                        _ => 0xF1C40F,
                    };
                    
                    let updated_embed = CreateEmbed::new()
                        .title(format!("💡 Ідея користувача @{}", suggestion.author_name))
                        .description(format!("**Ідея**\n{}", suggestion.content))
                        .color(color)
                        .field(format!("За: {} | Проти: {} | Процентів за: {}%", suggestion.votes_for, suggestion.votes_against, percent), "", false)
                        .field("Статус", status_text, false)
                        .footer(CreateEmbedFooter::new("Хочете додати свою ідею? Просто напишіть її прямо сюди"));
                    
                    let channel_num: u64 = match suggestion.channel_id.parse() {
                        Ok(v) if v > 0 => v,
                        _ => { warn!("Invalid channel_id in suggestion"); return Ok(()); }
                    };
                    let msg_num: u64 = match msg_id.parse() {
                        Ok(v) if v > 0 => v,
                        _ => { warn!("Invalid message_id in suggestion"); return Ok(()); }
                    };
                    let channel_id = serenity::ChannelId::new(channel_num);
                    let message_id = serenity::MessageId::new(msg_num);
                    
                    let _ = channel_id.edit_message(&ctx.http, message_id, serenity::EditMessage::new().embed(updated_embed)).await;
                    
                    modal_interaction.create_response(&ctx.http, serenity::CreateInteractionResponse::Message(
                        serenity::CreateInteractionResponseMessage::new()
                            .content("✅ Ідею успішно оновлено!")
                            .ephemeral(true)
                    )).await?;
                } else {
                    modal_interaction.create_response(&ctx.http, serenity::CreateInteractionResponse::Message(
                        serenity::CreateInteractionResponseMessage::new()
                            .content("❌ Не вдалося знайти ідею для оновлення.")
                            .ephemeral(true)
                    )).await?;
                }
            }
            
            return Ok(());
        }
        
        // Обробка кнопок
        if let Some(interaction) = interaction.as_message_component() {
            let custom_id = &interaction.data.custom_id;
            
            // Обробка кнопок ідей
            if custom_id.starts_with("idea_") {
                let msg_id = interaction.message.id.to_string();
                
                let suggestion_data = {
                    let suggestions = safe_lock(&data.suggestions_data);
                    suggestions.get(&msg_id).cloned()
                };
                
                if let Some(mut suggestion) = suggestion_data {
                    let user_id = interaction.user.id.to_string();
                    let is_author = user_id == suggestion.author_id;
                    let is_admin = if let Some(member) = &interaction.member {
                        if let Some(guild_id) = interaction.guild_id {
                            // 1. Fetch the channel FIRST (because this has an .await)
                            if let Ok(channel) = interaction.channel_id.to_channel(&ctx.http).await {
                                // 2. NOW get the guild from cache (no .await inside here)
                                if let Some(guild) = guild_id.to_guild_cached(&ctx.cache) {
                                    if let Some(guild_channel) = channel.guild() {
                                        guild.user_permissions_in(&guild_channel, member).administrator()
                                    } else {
                                        false
                                    }
                                } else {
                                    // Fallback if guild not in cache
                                    #[allow(deprecated)]
                                    member.permissions(&ctx.cache).map(|p| p.administrator()).unwrap_or(false)
                                }
                            } else {
                                // Fallback if channel fetch fails
                                #[allow(deprecated)]
                                member.permissions(&ctx.cache).map(|p| p.administrator()).unwrap_or(false)
                            }
                        } else {
                            false
                        }
                    } else {
                        false
                    };
                    
                    if custom_id.starts_with("idea_like_") {
                        // Перевірка: чи це автор ідеї?
                        if is_author {
                            interaction.create_response(&ctx.http, serenity::CreateInteractionResponse::Message(
                                serenity::CreateInteractionResponseMessage::new()
                                    .content("❌ Ви не можете голосувати за свою власну ідею! Ви можете тільки змінити її.")
                                    .ephemeral(true)
                            )).await?;
                            return Ok(());
                        }
                        
                        // Перевірка: чи користувач вже голосував?
                        let vote_key = format!("{}:like", user_id);
                        let already_voted_like = suggestion.voted_users.contains(&vote_key);
                        let vote_key_dislike = format!("{}:dislike", user_id);
                        let already_voted_dislike = suggestion.voted_users.contains(&vote_key_dislike);
                        
                        if already_voted_like {
                            interaction.create_response(&ctx.http, serenity::CreateInteractionResponse::Message(
                                serenity::CreateInteractionResponseMessage::new()
                                    .content("❌ Ви вже проголосували \"Класнючка\" за цю ідею!")
                                    .ephemeral(true)
                            )).await?;
                            return Ok(());
                        }
                        
                        if already_voted_dislike {
                            interaction.create_response(&ctx.http, serenity::CreateInteractionResponse::Message(
                                serenity::CreateInteractionResponseMessage::new()
                                    .content("❌ Ви вже проголосували \"Жах\" за цю ідею! Неможливо змінити голос.")
                                    .ephemeral(true)
                            )).await?;
                            return Ok(());
                        }
                        
                        // Додаємо голос
                        suggestion.votes_for += 1;
                        suggestion.voted_users.push(vote_key);
                        {
                            let mut suggestions = safe_lock(&data.suggestions_data);
                            suggestions.insert(msg_id.clone(), suggestion.clone());
                            save_json(SUGGESTIONS_DATA_FILE, &*suggestions);
                        }
                        
                        // Оновлюємо embed
                        let total = suggestion.votes_for + suggestion.votes_against;
                        let percent = if total > 0 { (suggestion.votes_for as f64 / total as f64 * 100.0) as u32 } else { 0 };
                        
                        let updated_embed = CreateEmbed::new()
                            .title(format!("💡 Ідея користувача @{}", suggestion.author_name))
                            .description(format!("**Ідея**\n{}", suggestion.content))
                            .color(0xF1C40F)
                            .field(format!("За: {} | Проти: {} | Процентів за: {}%", suggestion.votes_for, suggestion.votes_against, percent), "", false)
                            .field("Статус", "📊 | Чекаємо на відгук спільноти! Все у ваших руках", false)
                            .footer(CreateEmbedFooter::new("Хочете додати свою ідею? Просто напишіть її прямо сюди"));
                        
                        interaction.create_response(&ctx.http, serenity::CreateInteractionResponse::UpdateMessage(
                            serenity::CreateInteractionResponseMessage::new().embed(updated_embed)
                        )).await?;
                        
                    } else if custom_id.starts_with("idea_dislike_") {
                        // Перевірка: чи це автор ідеї?
                        if is_author {
                            interaction.create_response(&ctx.http, serenity::CreateInteractionResponse::Message(
                                serenity::CreateInteractionResponseMessage::new()
                                    .content("❌ Ви не можете голосувати за свою власну ідею! Ви можете тільки змінити її.")
                                    .ephemeral(true)
                            )).await?;
                            return Ok(());
                        }
                        
                        // Перевірка: чи користувач вже голосував?
                        let vote_key = format!("{}:dislike", user_id);
                        let already_voted_dislike = suggestion.voted_users.contains(&vote_key);
                        let vote_key_like = format!("{}:like", user_id);
                        let already_voted_like = suggestion.voted_users.contains(&vote_key_like);
                        
                        if already_voted_dislike {
                            interaction.create_response(&ctx.http, serenity::CreateInteractionResponse::Message(
                                serenity::CreateInteractionResponseMessage::new()
                                    .content("❌ Ви вже проголосували \"Жах\" за цю ідею!")
                                    .ephemeral(true)
                            )).await?;
                            return Ok(());
                        }
                        
                        if already_voted_like {
                            interaction.create_response(&ctx.http, serenity::CreateInteractionResponse::Message(
                                serenity::CreateInteractionResponseMessage::new()
                                    .content("❌ Ви вже проголосували \"Класнючка\" за цю ідею! Неможливо змінити голос.")
                                    .ephemeral(true)
                            )).await?;
                            return Ok(());
                        }
                        
                        // Додаємо голос
                        suggestion.votes_against += 1;
                        suggestion.voted_users.push(vote_key);
                        {
                            let mut suggestions = safe_lock(&data.suggestions_data);
                            suggestions.insert(msg_id.clone(), suggestion.clone());
                            save_json(SUGGESTIONS_DATA_FILE, &*suggestions);
                        }
                        
                        let total = suggestion.votes_for + suggestion.votes_against;
                        let percent = if total > 0 { (suggestion.votes_for as f64 / total as f64 * 100.0) as u32 } else { 0 };
                        
                        let updated_embed = CreateEmbed::new()
                            .title(format!("💡 Ідея користувача @{}", suggestion.author_name))
                            .description(format!("**Ідея**\n{}", suggestion.content))
                            .color(0xF1C40F)
                            .field(format!("За: {} | Проти: {} | Процентів за: {}%", suggestion.votes_for, suggestion.votes_against, percent), "", false)
                            .field("Статус", "📊 | Чекаємо на відгук спільноти! Все у ваших руках", false)
                            .footer(CreateEmbedFooter::new("Хочете додати свою ідею? Просто напишіть її прямо сюди"));
                        
                        interaction.create_response(&ctx.http, serenity::CreateInteractionResponse::UpdateMessage(
                            serenity::CreateInteractionResponseMessage::new().embed(updated_embed)
                        )).await?;
                        
                    } else if custom_id.starts_with("idea_approve_") && is_admin {
                        suggestion.status = "approved".to_string();
                        
                        let total = suggestion.votes_for + suggestion.votes_against;
                        let percent = if total > 0 { (suggestion.votes_for as f64 / total as f64 * 100.0) as u32 } else { 0 };
                        
                        let updated_embed = CreateEmbed::new()
                            .title(format!("💡 Ідея користувача @{}", suggestion.author_name))
                            .description(format!("**Ідея**\n{}", suggestion.content))
                            .color(0x2ECC71)
                            .field(format!("За: {} | Проти: {} | Процентів за: {}%", suggestion.votes_for, suggestion.votes_against, percent), "", false)
                            .field("Статус", "✅ | Крута ідея, інтегруєм!", false)
                            .footer(CreateEmbedFooter::new("Ідея прийнята і буде реалізована!"));
                        
                        interaction.create_response(&ctx.http, serenity::CreateInteractionResponse::UpdateMessage(
                            serenity::CreateInteractionResponseMessage::new()
                                .embed(updated_embed)
                                .components(vec![]) // Видаляємо кнопки
                        )).await?;
                        
                        // Видаляємо ідею з JSON після прийняття
                        {
                            let mut suggestions = safe_lock(&data.suggestions_data);
                            suggestions.remove(&msg_id);
                            save_json(SUGGESTIONS_DATA_FILE, &*suggestions);
                        }
                        
                    } else if custom_id.starts_with("idea_reject_") && is_admin {
                        suggestion.status = "rejected".to_string();
                        
                        let total = suggestion.votes_for + suggestion.votes_against;
                        let percent = if total > 0 { (suggestion.votes_for as f64 / total as f64 * 100.0) as u32 } else { 0 };
                        
                        let updated_embed = CreateEmbed::new()
                            .title(format!("💡 Ідея користувача @{}", suggestion.author_name))
                            .description(format!("**Ідея**\n{}", suggestion.content))
                            .color(0xE74C3C)
                            .field(format!("За: {} | Проти: {} | Процентів за: {}%", suggestion.votes_for, suggestion.votes_against, percent), "", false)
                            .field("Статус", "❌ | До одного місця такі ідеї!", false)
                            .footer(CreateEmbedFooter::new("Ідея відхилена."));
                        
                        interaction.create_response(&ctx.http, serenity::CreateInteractionResponse::UpdateMessage(
                            serenity::CreateInteractionResponseMessage::new()
                                .embed(updated_embed)
                                .components(vec![]) // Видаляємо кнопки
                        )).await?;
                        
                        // Видаляємо ідею з JSON після відхилення
                        {
                            let mut suggestions = safe_lock(&data.suggestions_data);
                            suggestions.remove(&msg_id);
                            save_json(SUGGESTIONS_DATA_FILE, &*suggestions);
                        }
                        
                    } else if custom_id.starts_with("idea_edit_") && is_author {
                        // Інформуємо автора як змінити ідею
                        interaction.create_response(&ctx.http, serenity::CreateInteractionResponse::Message(
                            serenity::CreateInteractionResponseMessage::new()
                                .content("✏️ Щоб змінити ідею, просто **відповідте (reply)** на це повідомлення з новою версією вашої ідеї!")
                                .ephemeral(true)
                        )).await?;
                    } else {
                        interaction.create_response(&ctx.http, serenity::CreateInteractionResponse::Message(
                            serenity::CreateInteractionResponseMessage::new()
                                .content("❌ У вас немає прав для цієї дії!")
                                .ephemeral(true)
                        )).await?;
                    }
                }
            }
        }
    }
    
    Ok(())
}

async fn background_tasks(ctx: serenity::Context, data: Arc<Data>) {
    let mut m_tick = time::interval(Duration::from_secs(60));
    let mut h_tick = time::interval(Duration::from_secs(3600));
    let mut d_tick = time::interval(Duration::from_secs(86400));

    loop {
        tokio::select! {
            _ = m_tick.tick() => {
                let mut updates: Vec<(serenity::UserId, serenity::GuildId, u64)> = Vec::new();
                let mut save = false;

                let guilds = ctx.cache.guilds();
                for g in guilds {
                    let voice_users: Vec<serenity::UserId> = if let Some(guild) = g.to_guild_cached(&ctx.cache) {
                         guild.voice_states.iter()
                            .filter(|(_, s)| !s.self_deaf && !s.self_mute)
                            .map(|(_, s)| s.user_id)
                            .collect()
                    } else {
                        continue;
                    };

                    for user_id in voice_users {
                        if let Ok(user) = user_id.to_user(&ctx.http).await {
                            if !user.bot {
                                let mut users = safe_lock(&data.users);
                                let p = users.entry(user_id.to_string()).or_insert(create_default_profile());
                                let multiplier = get_xp_multiplier(p);
                                p.xp += VOICE_XP_AMOUNT * multiplier;
                                p.minutes += 1;

                                if let Some(new_lvl) = try_levelup(p) {
                                    updates.push((user_id, g, new_lvl));
                                }
                                save = true;
                            }
                        }
                    }
                }

                if save {
                    let u = safe_lock(&data.users);
                    save_json(USERS_FILE, &*u);
                }

                for (uid, gid, lvl) in updates {
                    assign_role(&ctx, gid, uid, lvl).await;

                    let system_channel_id = gid.to_guild_cached(&ctx.cache)
                        .and_then(|g| g.system_channel_id);

                    if let Some(chan_id) = system_channel_id {
                         let _ = chan_id.say(&ctx.http, format!("🎉 <@{}> апнув рівень **{}** (Voice)!", uid, lvl)).await;
                    }
                }
            }
            _ = h_tick.tick() => {
                let now = Local::now();
                if now.hour() == 9 {
                    let today = format!("{:02}.{:02}", now.day(), now.month());
                    let celebs = {
                        let bds = safe_lock(&data.birthdays);
                        bds.iter().filter(|(_, d)| *d == &today).map(|(u,_)| format!("<@{}>", u)).collect::<Vec<_>>()
                    };

                    if !celebs.is_empty() {
                         let guilds: Vec<serenity::GuildId> = ctx.cache.guilds();
                         for g in guilds {
                            let system_channel_id = g.to_guild_cached(&ctx.cache)
                                .and_then(|g| g.system_channel_id);

                            if let Some(chan_id) = system_channel_id {
                                let _ = chan_id.say(&ctx.http, format!("🎂 **СВЯТО!** Вітаємо: {}", celebs.join(", "))).await;
                            }
                        }
                    }
                }
            }
            _ = d_tick.tick() => {
                 let admin = serenity::UserId::new(get_admin_id());
                 if let Ok(chan) = admin.create_dm_channel(&ctx.http).await {
                     let f = vec![CreateAttachment::path(USERS_FILE).await, CreateAttachment::path(BIRTHDAY_FILE).await];
                     let valid: Vec<_> = f.into_iter().filter_map(|x| x.ok()).collect();
                     if !valid.is_empty() {
                         let _ = chan.send_files(&ctx.http, valid, CreateMessage::new().content("📦 Щоденний бекап")).await;
                         info!("📤 Бекап відправлено адміністратору");
                     }
                 }
            }
        }
    }
}

#[tokio::main]
async fn main() {
    // Ініціалізація логування
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .with_target(false)
        .with_thread_ids(false)
        .init();

    info!("🚀 Запуск StarostaBot...");
    
    let users_data = Arc::new(Mutex::new(load_json(USERS_FILE)));
    let birthdays_data = Arc::new(Mutex::new(load_json(BIRTHDAY_FILE)));
    let auto_roles_data = Arc::new(Mutex::new(load_json(AUTO_ROLES_FILE)));
    let banned_words_data = Arc::new(Mutex::new(load_json(BANNED_WORDS_FILE)));
    let suggestions_channels_data = Arc::new(Mutex::new(load_json::<Vec<String>>(SUGGESTIONS_CHANNELS_FILE)));
    let suggestions_data_data = Arc::new(Mutex::new(load_json::<HashMap<String, SuggestionData>>(SUGGESTIONS_DATA_FILE)));
    
    let data = Data { 
        users: users_data.clone(), 
        birthdays: birthdays_data.clone(),
        auto_roles: auto_roles_data.clone(),
        banned_words: banned_words_data.clone(),
        suggestions_channels: suggestions_channels_data.clone(),
        suggestions_data: suggestions_data_data.clone(),
    };

    let framework = poise::Framework::builder()
        .options(poise::FrameworkOptions {
            commands: vec![
                help(),
                info(),
                leaderboard(),
                setup_roles(),
                purge(),
                clean(),
                poll(),
                avatar(),
                rank(),
                daily(),
                casino(),
                blackjack(),
                shop(),
                buy_booster(),
                setup_suggestions_channel(),
                remove_suggestions_channel(),
                set_birthday(),
                birthdays(),
                admin_set_level(),
                admin_set_xp(),
                admin_set_chips(),
                admin_add_birthday(),
                admin_remove_birthday(),
                admin_mute(),
                admin_unmute(),
                admin_announce(),
                // Нові команди для управління
                setup_autorole(),
                remove_autorole(),
                add_banned_word(),
                list_banned_words(),
                remove_banned_word(),
                cleanup_inactive(),
            ],
            event_handler: |ctx, event, framework, data| Box::pin(event_handler(ctx, event, framework, data)),
            ..Default::default()
        })
        .setup(|ctx, _ready, framework| {
            Box::pin(async move {
                poise::builtins::register_globally(ctx, &framework.options().commands).await?;
                let ctx_clone = ctx.clone();
                let data_clone = Arc::new(Data { 
                    users: users_data.clone(), 
                    birthdays: birthdays_data.clone(),
                    auto_roles: auto_roles_data.clone(),
                    banned_words: banned_words_data.clone(),
                    suggestions_channels: suggestions_channels_data.clone(),
                    suggestions_data: suggestions_data_data.clone(),
                });
                tokio::spawn(async move { background_tasks(ctx_clone, data_clone).await; });
                info!("✅ StarostaBot успішно запущено!");
                info!("📊 Завантажено користувачів: {}", safe_lock(&data.users).len());
                info!("🎂 Завантажено днів народження: {}", safe_lock(&data.birthdays).len());
                info!("🎭 Завантажено авто-ролей: {}", safe_lock(&data.auto_roles).len());
                info!("🚫 Завантажено заборонених слів: {}", safe_lock(&data.banned_words).len());
                info!("💡 Завантажено каналів ідей: {}", safe_lock(&data.suggestions_channels).len());
                info!("📝 Завантажено ідей: {}", safe_lock(&data.suggestions_data).len());
                Ok(data)
            })
        })
        .build();

    let intents = serenity::GatewayIntents::non_privileged()
        | serenity::GatewayIntents::MESSAGE_CONTENT
        | serenity::GatewayIntents::GUILD_VOICE_STATES
        | serenity::GatewayIntents::GUILDS
        | serenity::GatewayIntents::GUILD_MEMBERS;

    let token = get_token();
    
    let client = serenity::ClientBuilder::new(token, intents)
        .framework(framework)
        .await;

    match client {
        Ok(mut client) => {
            info!("🔌 Підключення до Discord...");
            if let Err(e) = client.start().await {
                error!("❌ Помилка запуску клієнта: {:?}", e);
            }
        }
        Err(e) => {
            error!("❌ Не вдалося створити клієнт: {:?}", e);
        }
    }
}
