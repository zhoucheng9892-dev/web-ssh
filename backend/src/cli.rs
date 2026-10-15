//! Maintenance subcommands that run from the command line instead of starting
//! the server: `reset-admin` (recover a locked-out admin account) and
//! `list-users` (find the admin username). Both operate on the same database
//! the server uses, resolved via the usual `Config::load()` (env + data/.env),
//! so run them with the same working directory / environment as the server.

use anyhow::{bail, Context, Result};
use rand::Rng;

use crate::{
    config::Config,
    crypto,
    db::{self, models},
};

/// `reset-admin [username] [new-password]`
///
/// Resets an account's password. Without `username`, targets the first
/// (lowest-id) admin account. Without `new-password`, generates a random one
/// and prints it — avoiding shell-history / process-list exposure.
pub async fn reset_admin(username: Option<&str>, new_password: Option<&str>) -> Result<()> {
    let config = Config::load()?;
    let pool = db::create_pool(&config.db_path)?;

    let target = match username {
        Some(name) => {
            let users = models::list_users(&pool).await?;
            users
                .into_iter()
                .find(|u| u.username == name)
                .with_context(|| format!("user '{name}' not found (try `list-users`)"))?
        }
        None => {
            let users = models::list_users(&pool).await?;
            let mut admins: Vec<_> = users.into_iter().filter(|u| u.is_admin).collect();
            admins.sort_by_key(|u| u.id);
            admins.into_iter().next().context(
                "no admin account exists — start the server once and complete setup first",
            )?
        }
    };

    let password = match new_password {
        Some(p) => {
            if p.len() < 6 {
                bail!("password too short (min 6 chars)");
            }
            p.to_string()
        }
        None => generate_password(16),
    };

    // The web login hashes the typed password client-side (SHA-256) before
    // sending; the server then stores Argon2id of that. Mirror the exact same
    // scheme here so a CLI-reset password is accepted by the login page —
    // hashing the plaintext directly would store Argon2id(plaintext), which
    // login (Argon2id(SHA-256)) can never match.
    let wire = crypto::sha256_hex(&password);
    let hash = crypto::hash_password(&wire)?;
    if !models::update_user_password(&pool, target.id, &hash).await? {
        bail!("user not found");
    }

    println!("已重置账号密码：");
    println!("  用户名: {}", target.username);
    if target.is_disabled {
        println!("  ⚠️ 该账号当前处于冻结状态，重置密码后仍无法登录（需解冻）。");
    }
    match new_password {
        Some(_) => println!("  密码: （为您在命令行提供的值）"),
        None => {
            println!("  新密码: {password}");
            println!();
            println!("请立即登录，并在「修改密码」页面更改为您自己的密码。");
        }
    }
    Ok(())
}

/// `list-users` — print all accounts, to find the admin username.
pub async fn list_users() -> Result<()> {
    let config = Config::load()?;
    let pool = db::create_pool(&config.db_path)?;
    let users = models::list_users(&pool).await?;
    if users.is_empty() {
        println!("（还没有任何账号 — 先启动一次服务完成初始化）");
        return Ok(());
    }
    println!("{:<6} {:<24} {:<8} {}", "ID", "用户名", "管理员", "状态");
    for u in users {
        println!(
            "{:<6} {:<24} {:<8} {}",
            u.id,
            u.username,
            if u.is_admin { "是" } else { "否" },
            if u.is_disabled { "已冻结" } else { "正常" }
        );
    }
    Ok(())
}

/// Print CLI usage for the maintenance subcommands.
pub fn print_help() {
    println!(
        "web-ssh-backend [command]\n\
         \n\
         不带命令时启动 Web SSH 服务（默认行为）。\n\
         \n\
         命令：\n\
           reset-admin [用户名] [新密码]\n\
               重置账号密码。省略用户名时重置第一个管理员账号；\n\
               省略新密码时自动生成随机密码并打印。\n\
           list-users\n\
               列出所有账号（用于找回管理员用户名）。\n\
           help\n\
               显示本帮助。"
    );
}

/// Random password from an unambiguous charset (no 0/O, 1/l/I).
fn generate_password(len: usize) -> String {
    const CHARSET: &[u8] = b"ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz23456789";
    let mut rng = rand::thread_rng();
    (0..len)
        .map(|_| CHARSET[rng.gen_range(0..CHARSET.len())] as char)
        .collect()
}
