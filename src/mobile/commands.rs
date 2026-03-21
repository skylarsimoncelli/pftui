use std::fs;
use std::net::{IpAddr, Ipv4Addr};
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use anyhow::{anyhow, Result};
use argon2::{password_hash::SaltString, Argon2, PasswordHasher};
use chrono::Utc;
use rand::rngs::OsRng;
use rand::RngCore;
use rcgen::{CertificateParams, DnType, KeyPair};
use serde_json::json;
use time::OffsetDateTime;

use crate::cli::MobileTokenPermissionArg;
use crate::config::{config_path, save_config, Config, MobileApiToken, MobileTokenPermission};

const CERT_FILE: &str = "mobile-cert.pem";
const KEY_FILE: &str = "mobile-key.pem";

pub fn enable(config: &Config, bind: &str, port: u16) -> Result<()> {
    let (cert_path, key_path) = ensure_tls_material(bind)?;

    let mut next = config.clone();
    next.mobile.enabled = true;
    next.mobile.bind = bind.to_string();
    next.mobile.port = port;
    next.mobile.cert_path = Some(cert_path.to_string_lossy().to_string());
    next.mobile.key_path = Some(key_path.to_string_lossy().to_string());
    save_config(&next)?;

    let fingerprint = certificate_fingerprint(&cert_path)?;
    println!("✓ Mobile API enabled");
    println!("  Bind: {}:{}", next.mobile.bind, next.mobile.port);
    println!("  TLS fingerprint: {}", fingerprint);
    if next.mobile.api_tokens.is_empty() {
        println!("  No API tokens configured yet. Generate one with:");
        println!("  pftui system mobile token generate --permission read --name ios");
    }
    println!("  Start with: pftui system mobile serve");
    Ok(())
}

pub fn disable(config: &Config) -> Result<()> {
    let mut next = config.clone();
    next.mobile.enabled = false;
    save_config(&next)?;
    println!("✓ Mobile API disabled");
    Ok(())
}

pub fn generate_token(
    config: &Config,
    name: &str,
    permission: MobileTokenPermissionArg,
) -> Result<()> {
    let raw_token = new_token(permission);
    let token_hash = hash_token(&raw_token)?;
    let prefix = token_prefix(&raw_token);

    let mut next = config.clone();
    next.mobile.api_tokens.push(MobileApiToken {
        name: name.trim().to_string(),
        prefix: prefix.clone(),
        token_hash,
        permission: map_permission(permission),
        created_at: Utc::now().to_rfc3339(),
    });
    save_config(&next)?;

    println!("✓ Mobile API token created");
    println!("  Name: {}", name.trim());
    println!(
        "  Permission: {}",
        format_permission(map_permission(permission))
    );
    println!("  Token: {}", raw_token);
    println!("  Save it now — it will not be shown again.");
    Ok(())
}

pub fn status(config: &Config, json_output: bool) -> Result<()> {
    let fingerprint = config
        .mobile
        .cert_path
        .as_deref()
        .map(Path::new)
        .filter(|path| path.exists())
        .map(certificate_fingerprint)
        .transpose()?;

    let tokens = config
        .mobile
        .api_tokens
        .iter()
        .map(|token| {
            json!({
                "name": token.name,
                "prefix": token.prefix,
                "permission": format_permission(token.permission),
                "created_at": token.created_at,
            })
        })
        .collect::<Vec<_>>();

    if json_output {
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({
                "enabled": config.mobile.enabled,
                "bind": config.mobile.bind,
                "port": config.mobile.port,
                "cert_path": config.mobile.cert_path,
                "key_path": config.mobile.key_path,
                "session_ttl_hours": config.mobile.session_ttl_hours,
                "fingerprint": fingerprint,
                "api_tokens": tokens,
            }))?
        );
    } else {
        println!("enabled = {}", config.mobile.enabled);
        println!("bind = {}", config.mobile.bind);
        println!("port = {}", config.mobile.port);
        println!("session_ttl_hours = {}", config.mobile.session_ttl_hours);
        println!(
            "cert_path = {}",
            config.mobile.cert_path.as_deref().unwrap_or("")
        );
        println!(
            "key_path = {}",
            config.mobile.key_path.as_deref().unwrap_or("")
        );
        println!("fingerprint = {}", fingerprint.unwrap_or_default());
        println!("api_tokens = {}", config.mobile.api_tokens.len());
        for token in &config.mobile.api_tokens {
            println!(
                "  - {} [{}] {}",
                token.name,
                format_permission(token.permission),
                token.prefix
            );
        }
    }
    Ok(())
}

pub fn certificate_fingerprint(path: &Path) -> Result<String> {
    use sha2::{Digest, Sha256};

    let pem = fs::read(path)?;
    let mut pem_slice = pem.as_slice();
    let mut certs = rustls_pemfile::certs(&mut pem_slice);
    let cert = certs
        .next()
        .transpose()?
        .ok_or_else(|| anyhow!("No certificate found in {}", path.display()))?;
    let digest = Sha256::digest(cert.as_ref());
    Ok(digest
        .iter()
        .map(|byte| format!("{:02X}", byte))
        .collect::<Vec<_>>()
        .join(":"))
}

fn hash_token(token: &str) -> Result<String> {
    let salt = SaltString::generate(&mut OsRng);
    Ok(Argon2::default()
        .hash_password(token.as_bytes(), &salt)
        .map_err(|e| anyhow!("failed to hash token: {}", e))?
        .to_string())
}

fn new_token(permission: MobileTokenPermissionArg) -> String {
    let mut bytes = [0_u8; 24];
    OsRng.fill_bytes(&mut bytes);
    let suffix = bytes
        .iter()
        .map(|b| format!("{:02x}", b))
        .collect::<String>();
    format!(
        "pftm_{}_{}",
        match permission {
            MobileTokenPermissionArg::Read => "read",
            MobileTokenPermissionArg::Write => "write",
        },
        suffix
    )
}

fn token_prefix(token: &str) -> String {
    let visible = token.chars().take(18).collect::<String>();
    format!("{}…", visible)
}

fn map_permission(permission: MobileTokenPermissionArg) -> MobileTokenPermission {
    match permission {
        MobileTokenPermissionArg::Read => MobileTokenPermission::Read,
        MobileTokenPermissionArg::Write => MobileTokenPermission::Write,
    }
}

pub fn list_tokens(config: &Config, json_output: bool) -> Result<()> {
    if config.mobile.api_tokens.is_empty() {
        if json_output {
            println!("[]");
        } else {
            println!("No mobile API tokens configured.");
        }
        return Ok(());
    }

    if json_output {
        let tokens: Vec<serde_json::Value> = config
            .mobile
            .api_tokens
            .iter()
            .map(|token| {
                json!({
                    "name": token.name,
                    "prefix": token.prefix,
                    "permission": format_permission(token.permission),
                    "created_at": token.created_at,
                })
            })
            .collect();
        println!("{}", serde_json::to_string_pretty(&tokens)?);
    } else {
        println!("Mobile API tokens:");
        for token in &config.mobile.api_tokens {
            println!(
                "  {} [{}] {} (created {})",
                token.name,
                format_permission(token.permission),
                token.prefix,
                token.created_at
            );
        }
    }
    Ok(())
}

pub fn revoke_token(config: &Config, prefix: &str, json_output: bool) -> Result<()> {
    let matching: Vec<usize> = config
        .mobile
        .api_tokens
        .iter()
        .enumerate()
        .filter(|(_, token)| {
            token.name.contains(prefix) || token.prefix.contains(prefix)
        })
        .map(|(idx, _)| idx)
        .collect();

    if matching.is_empty() {
        if json_output {
            println!(
                "{}",
                serde_json::to_string_pretty(&json!({
                    "revoked": false,
                    "error": format!("No token matching '{}'", prefix),
                }))?
            );
        } else {
            println!("No token matching '{}'", prefix);
        }
        return Ok(());
    }

    if matching.len() > 1 {
        if json_output {
            println!(
                "{}",
                serde_json::to_string_pretty(&json!({
                    "revoked": false,
                    "error": format!("Multiple tokens match '{}'. Be more specific.", prefix),
                }))?
            );
        } else {
            println!(
                "Multiple tokens match '{}'. Be more specific:",
                prefix
            );
            for idx in &matching {
                let token = &config.mobile.api_tokens[*idx];
                println!("  {} [{}] {}", token.name, format_permission(token.permission), token.prefix);
            }
        }
        return Ok(());
    }

    let idx = matching[0];
    let removed = &config.mobile.api_tokens[idx];
    let removed_name = removed.name.clone();

    let mut next = config.clone();
    next.mobile.api_tokens.remove(idx);
    save_config(&next)?;

    if json_output {
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({
                "revoked": true,
                "name": removed_name,
            }))?
        );
    } else {
        println!("✓ Revoked token '{}'", removed_name);
    }
    Ok(())
}

fn format_permission(permission: MobileTokenPermission) -> &'static str {
    match permission {
        MobileTokenPermission::Read => "read",
        MobileTokenPermission::Write => "write",
    }
}

fn ensure_tls_material(bind: &str) -> Result<(PathBuf, PathBuf)> {
    let dir = config_path()
        .parent()
        .ok_or_else(|| anyhow!("could not resolve pftui config directory"))?
        .to_path_buf();
    fs::create_dir_all(&dir)?;
    let cert_path = dir.join(CERT_FILE);
    let key_path = dir.join(KEY_FILE);
    if cert_path.exists() && key_path.exists() {
        // Ensure existing key file has correct permissions
        #[cfg(unix)]
        fs::set_permissions(&key_path, fs::Permissions::from_mode(0o600))?;
        return Ok((cert_path, key_path));
    }

    let mut names = vec![
        "localhost".to_string(),
        IpAddr::V4(Ipv4Addr::LOCALHOST).to_string(),
    ];
    if let Ok(ip) = bind.parse::<IpAddr>() {
        names.push(ip.to_string());
    }

    // Generate certificate with 2-year validity
    let key_pair = KeyPair::generate()?;
    let mut params = CertificateParams::new(names)?;
    params.distinguished_name.push(DnType::CommonName, "pftui mobile API");
    let now = OffsetDateTime::now_utc();
    params.not_before = now;
    params.not_after = now + time::Duration::days(730); // 2 years
    let cert = params.self_signed(&key_pair)?;

    fs::write(&cert_path, cert.pem())?;
    fs::write(&key_path, key_pair.serialize_pem())?;

    // Set private key file permissions to owner-only (C-2)
    #[cfg(unix)]
    fs::set_permissions(&key_path, fs::Permissions::from_mode(0o600))?;

    Ok((cert_path, key_path))
}

#[cfg(test)]
mod tests {
    use super::{format_permission, map_permission, token_prefix};
    use crate::cli::MobileTokenPermissionArg;
    use crate::config::MobileTokenPermission;

    #[test]
    fn token_prefix_truncates() {
        assert!(token_prefix("pftm_read_1234567890abcdef").starts_with("pftm_read_12345678"));
    }

    #[test]
    fn permission_mapping_matches() {
        assert_eq!(
            map_permission(MobileTokenPermissionArg::Read),
            MobileTokenPermission::Read
        );
        assert_eq!(format_permission(MobileTokenPermission::Write), "write");
    }
}
