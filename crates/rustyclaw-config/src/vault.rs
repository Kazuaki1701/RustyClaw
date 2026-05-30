use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes256Gcm, Nonce};
use pbkdf2::pbkdf2_hmac;
use sha2::Sha256;
use std::collections::HashMap;
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::PathBuf;
use anyhow::{anyhow, Result};
use rand::RngCore;

const SALT_LEN: usize = 16;
const NONCE_LEN: usize = 12;
const PBKDF2_ROUNDS: u32 = 100_000;

/// vault.enc の格納先: {config_dir}/vault.enc
pub fn get_vault_path() -> PathBuf {
    crate::get_config_dir().join("vault.enc")
}

/// パスフレーズ解決順:
/// 1. 明示引数
/// 2. systemd CREDENTIALS_DIRECTORY/vault-key
/// 3. VAULT_PASSPHRASE 環境変数
/// 4. 空文字（パスフレーズなし運用）
pub fn resolve_passphrase(explicit: Option<&str>) -> Result<String> {
    if let Some(p) = explicit {
        return Ok(p.trim().to_string());
    }
    if let Ok(cred_dir) = std::env::var("CREDENTIALS_DIRECTORY") {
        let cred_path = PathBuf::from(&cred_dir).join("vault-key");
        if cred_path.exists() {
            let mut file = File::open(&cred_path)?;
            let mut contents = String::new();
            file.read_to_string(&mut contents)?;
            return Ok(contents.trim().to_string());
        }
    }
    if let Ok(env_pass) = std::env::var("VAULT_PASSPHRASE") {
        return Ok(env_pass.trim().to_string());
    }
    Ok(String::new())
}

fn derive_key(passphrase: &str, salt: &[u8]) -> [u8; 32] {
    let mut key = [0u8; 32];
    pbkdf2_hmac::<Sha256>(passphrase.as_bytes(), salt, PBKDF2_ROUNDS, &mut key);
    key
}

/// vault.enc を復号して HashMap を返す
pub fn load_vault(passphrase_override: Option<&str>) -> Result<HashMap<String, String>> {
    let passphrase = resolve_passphrase(passphrase_override)?;
    let path = get_vault_path();
    if !path.exists() {
        return Err(anyhow!("Vault file does not exist: {}", path.display()));
    }

    let mut buffer = Vec::new();
    File::open(&path)?.read_to_end(&mut buffer)?;

    if buffer.len() < SALT_LEN + NONCE_LEN {
        return Err(anyhow!("Vault file is corrupted (too small)"));
    }

    let (salt, rest) = buffer.split_at(SALT_LEN);
    let (nonce_bytes, ciphertext) = rest.split_at(NONCE_LEN);

    let key_bytes = derive_key(&passphrase, salt);
    let cipher = Aes256Gcm::new_from_slice(&key_bytes)
        .map_err(|e| anyhow!("Failed to initialize cipher: {}", e))?;

    let decrypted = cipher
        .decrypt(Nonce::from_slice(nonce_bytes), ciphertext)
        .map_err(|_| anyhow!("Failed to decrypt vault (wrong passphrase or corrupted file)"))?;

    Ok(serde_json::from_slice(&decrypted)?)
}

/// シークレットマップを暗号化して vault.enc に書き込む
pub fn save_vault(secrets: &HashMap<String, String>, passphrase: &str) -> Result<()> {
    let path = get_vault_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    let plaintext = serde_json::to_vec(secrets)?;

    let mut rng = rand::thread_rng();
    let mut salt = [0u8; SALT_LEN];
    let mut nonce_bytes = [0u8; NONCE_LEN];
    rng.fill_bytes(&mut salt);
    rng.fill_bytes(&mut nonce_bytes);

    let key_bytes = derive_key(passphrase, &salt);
    let cipher = Aes256Gcm::new_from_slice(&key_bytes)
        .map_err(|e| anyhow!("Failed to initialize cipher: {}", e))?;

    let ciphertext = cipher
        .encrypt(Nonce::from_slice(&nonce_bytes), plaintext.as_slice())
        .map_err(|e| anyhow!("Failed to encrypt: {}", e))?;

    let mut file = File::create(&path)?;
    file.write_all(&salt)?;
    file.write_all(&nonce_bytes)?;
    file.write_all(&ciphertext)?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = fs::metadata(&path)?.permissions();
        perms.set_mode(0o600);
        fs::set_permissions(&path, perms)?;
    }

    Ok(())
}

/// 既存の平文 vault.json から vault.enc へ一括移行する
pub fn import_from_json(json_path: &std::path::Path, passphrase: &str) -> Result<usize> {
    let content = fs::read_to_string(json_path)?;
    let map: HashMap<String, String> = serde_json::from_str(&content)
        .map_err(|e| anyhow!("Failed to parse vault.json: {}", e))?;
    let count = map.len();
    save_vault(&map, passphrase)?;
    Ok(count)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;
    use tempfile::TempDir;

    #[test]
    fn test_encrypt_decrypt_roundtrip() {
        let dir = TempDir::new().unwrap();
        // AGYCLAW_CONFIG 相当の env var は使わず、vault path を直接テスト
        let vault_path = dir.path().join("vault.enc");

        let mut secrets = HashMap::new();
        secrets.insert("cf-token".to_string(), "test-token-abc".to_string());
        secrets.insert("discord-token".to_string(), "discord-secret-xyz".to_string());

        // 暗号化保存（vault_path を一時的に上書き）
        let passphrase = "test-passphrase-123";
        let plaintext = serde_json::to_vec(&secrets).unwrap();
        let mut rng = rand::thread_rng();
        let mut salt = [0u8; SALT_LEN];
        let mut nonce_bytes = [0u8; NONCE_LEN];
        rng.fill_bytes(&mut salt);
        rng.fill_bytes(&mut nonce_bytes);
        let key = derive_key(passphrase, &salt);
        let cipher = Aes256Gcm::new_from_slice(&key).unwrap();
        let ciphertext = cipher.encrypt(Nonce::from_slice(&nonce_bytes), plaintext.as_slice()).unwrap();

        let mut file = File::create(&vault_path).unwrap();
        file.write_all(&salt).unwrap();
        file.write_all(&nonce_bytes).unwrap();
        file.write_all(&ciphertext).unwrap();

        // 復号確認
        let mut buf = Vec::new();
        File::open(&vault_path).unwrap().read_to_end(&mut buf).unwrap();
        let (s, rest) = buf.split_at(SALT_LEN);
        let (n, ct) = rest.split_at(NONCE_LEN);
        let key2 = derive_key(passphrase, s);
        let cipher2 = Aes256Gcm::new_from_slice(&key2).unwrap();
        let decrypted = cipher2.decrypt(Nonce::from_slice(n), ct).unwrap();
        let result: HashMap<String, String> = serde_json::from_slice(&decrypted).unwrap();

        assert_eq!(result.get("cf-token").unwrap(), "test-token-abc");
        assert_eq!(result.get("discord-token").unwrap(), "discord-secret-xyz");
    }

    #[test]
    fn test_wrong_passphrase_fails() {
        let dir = TempDir::new().unwrap();
        let vault_path = dir.path().join("vault.enc");

        let mut secrets = HashMap::new();
        secrets.insert("key".to_string(), "value".to_string());
        let plaintext = serde_json::to_vec(&secrets).unwrap();
        let mut rng = rand::thread_rng();
        let mut salt = [0u8; SALT_LEN];
        let mut nonce_bytes = [0u8; NONCE_LEN];
        rng.fill_bytes(&mut salt);
        rng.fill_bytes(&mut nonce_bytes);
        let key = derive_key("correct-pass", &salt);
        let cipher = Aes256Gcm::new_from_slice(&key).unwrap();
        let ciphertext = cipher.encrypt(Nonce::from_slice(&nonce_bytes), plaintext.as_slice()).unwrap();

        let mut file = File::create(&vault_path).unwrap();
        file.write_all(&salt).unwrap();
        file.write_all(&nonce_bytes).unwrap();
        file.write_all(&ciphertext).unwrap();

        let mut buf = Vec::new();
        File::open(&vault_path).unwrap().read_to_end(&mut buf).unwrap();
        let (s, rest) = buf.split_at(SALT_LEN);
        let (n, ct) = rest.split_at(NONCE_LEN);
        let key_wrong = derive_key("wrong-pass", s);
        let cipher_wrong = Aes256Gcm::new_from_slice(&key_wrong).unwrap();
        assert!(cipher_wrong.decrypt(Nonce::from_slice(n), ct).is_err());
    }
}
