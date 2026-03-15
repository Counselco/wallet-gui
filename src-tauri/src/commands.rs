// ╔══════════════════════════════════════════════════════════════════════════════╗
// ║  PLATFORM COMPILATION GUIDE — ChronX Wallet Backend (Tauri commands)       ║
// ║                                                                            ║
// ║  This file compiles for THREE targets from ONE source:                     ║
// ║    • Windows/macOS/Linux  — `#[cfg(not(mobile))]` or `#[cfg(desktop)]`     ║
// ║    • Android              — `#[cfg(mobile)]`                               ║
// ║    • iOS                  — `#[cfg(mobile)]`                               ║
// ║                                                                            ║
// ║  Tauri sets `cfg(mobile)` automatically for Android + iOS builds,          ║
// ║  and `cfg(desktop)` for Windows/macOS/Linux.                               ║
// ║                                                                            ║
// ║  KEY DIFFERENCE: File paths.                                               ║
// ║    Desktop  → ~/.chronx/*.json  (home directory)                           ║
// ║    Mobile   → app_data_dir()    (sandboxed app storage, Android & iOS)     ║
// ║                                                                            ║
// ║  All Tauri commands compile on ALL platforms. UI gating (is_desktop()      ║
// ║  in lib.rs) controls which features are shown per platform.                ║
// ║                                                                            ║
// ║  Build commands:                                                           ║
// ║    Windows:  cargo tauri build                                             ║
// ║    Android:  JAVA_HOME="..." cargo tauri android build --target aarch64    ║
// ║    iOS:      cargo tauri ios build   (on macOS only)                       ║
// ╚══════════════════════════════════════════════════════════════════════════════╝

use base64::Engine as _;
use sha2::{Sha256, Digest};
#[cfg(mobile)]
use tauri::Manager;
use chronx_core::{
    constants::{CHRONOS_PER_KX, POW_INITIAL_DIFFICULTY},
    transaction::{Action, AuthScheme, Transaction, TransactionBody},
    types::{AccountId, TimeLockId, TxId},
};
use rand::RngCore as _;
use chronx_crypto::{hash::tx_id_from_body, mine_pow, KeyPair};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tauri::AppHandle;
use chacha20poly1305::{aead::Aead, ChaCha20Poly1305, KeyInit};
use hkdf::Hkdf;
use x25519_dalek::{EphemeralSecret, PublicKey as X25519PublicKey};

const DEFAULT_RPC_URL: &str = "https://rpc.chronx.io";

// ── Platform-aware paths ──────────────────────────────────────────────────────

fn expand_tilde(path: &str) -> PathBuf {
    if path.starts_with("~/") {
        let home = std::env::var("USERPROFILE")
            .or_else(|_| std::env::var("HOME"))
            .unwrap_or_else(|_| ".".to_string());
        PathBuf::from(home).join(&path[2..])
    } else {
        PathBuf::from(path)
    }
}

fn keyfile_path(app: &AppHandle) -> PathBuf {
    #[cfg(mobile)]
    {
        app.path()
            .app_data_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join("wallet.json")
    }
    #[cfg(not(mobile))]
    {
        let _ = app; // unused on desktop
        expand_tilde("~/.chronx/wallet.json")
    }
}

fn config_path(app: &AppHandle) -> PathBuf {
    #[cfg(mobile)]
    {
        app.path()
            .app_data_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join("wallet-config.json")
    }
    #[cfg(not(mobile))]
    {
        let _ = app;
        expand_tilde("~/.chronx/wallet-config.json")
    }
}

// ── Config file ───────────────────────────────────────────────────────────────

#[derive(Serialize, Deserialize)]
struct WalletConfig {
    node_url: String,
    #[serde(default)]
    pin_hash: Option<String>,
    /// PIN length: 4, 6, or 8. Default 4 if missing.
    #[serde(default)]
    pin_length: Option<u8>,
    /// Legacy single claim email (kept for backward compat with old config files).
    #[serde(default)]
    claim_email: Option<String>,
    /// Up to 3 claim email addresses stored locally for incoming email-lock discovery.
    #[serde(default)]
    claim_emails: Option<Vec<String>>,
    /// Account IDs of cold storage wallets (address book).
    #[serde(default)]
    cold_wallets: Option<Vec<String>>,
    /// Email addresses that have been verified via the /verify-email flow.
    #[serde(default)]
    verified_emails: Option<Vec<String>>,
    /// Blocked poke sender emails (user chose "Block this sender" on decline).
    #[serde(default)]
    blocked_senders: Option<Vec<String>>,
    /// BLAKE3 hashes from successful claim_by_code calls. Used to discover
    /// sibling locks (e.g. 30d/60d faucet stages) even without email registration.
    #[serde(default)]
    claimed_hashes: Option<Vec<String>>,
    /// Base (Ethereum L2) wallet address for KX↔USDC conversions.
    #[serde(default)]
    base_address: Option<String>,
    /// Nickname for saved Base address (e.g. "My Trust Wallet").
    #[serde(default)]
    base_address_nickname: Option<String>,
    /// Multiple saved Base addresses (max 5) with nicknames.
    #[serde(default)]
    base_addresses: Option<Vec<SavedBaseAddress>>,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct SavedBaseAddress {
    pub address: String,
    pub nickname: String,
}

fn read_config(app: &AppHandle) -> WalletConfig {
    let path = config_path(app);
    let mut cfg: WalletConfig = std::fs::read_to_string(&path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or(WalletConfig {
            node_url: DEFAULT_RPC_URL.to_string(),
            pin_hash: None,
            pin_length: None,
            claim_email: None,
            claim_emails: None,
            cold_wallets: None,
            verified_emails: None,
            blocked_senders: None,
            claimed_hashes: None,
            base_address: None,
            base_address_nickname: None,
            base_addresses: None,
        });
    // Auto-migrate: if old single claim_email exists but claim_emails is empty, migrate it.
    if cfg.claim_emails.is_none() {
        if let Some(ref e) = cfg.claim_email {
            if !e.trim().is_empty() {
                cfg.claim_emails = Some(vec![e.trim().to_string()]);
            }
        }
    }
    cfg
}

fn rpc_url(app: &AppHandle) -> String {
    read_config(app).node_url
}

fn write_config(app: &AppHandle, cfg: &WalletConfig) -> Result<(), String> {
    let path = config_path(app);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("Creating config dir: {e}"))?;
    }
    let json = serde_json::to_string_pretty(cfg).map_err(|e| e.to_string())?;
    std::fs::write(&path, json).map_err(|e| format!("Writing config: {e}"))
}

fn hash_pin(pin: &str) -> String {
    let result = Sha256::digest(pin.as_bytes());
    result.iter().map(|b| format!("{:02x}", b)).collect()
}

// ── Types returned to the frontend ───────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct AccountInfo {
    pub account_id: String,
    pub balance_kx: String,
    pub balance_chronos: String,
    pub spendable_kx: String,
    pub spendable_chronos: String,
    pub nonce: u64,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct TimeLockInfo {
    pub lock_id: String,
    pub sender: String,
    pub recipient_account_id: String,
    pub amount_kx: String,
    pub amount_chronos: String,
    pub unlock_at: i64,
    pub created_at: i64,
    pub status: String,
    pub memo: Option<String>,
    /// Hex of BLAKE3(claim_code) — locks sharing the same hash belong to a Promise Series.
    #[serde(default)]
    pub claim_secret_hash: Option<String>,
    /// Cancellation window in seconds (72 h for email, 24 h for ≥1-year).
    #[serde(default)]
    pub cancellation_window_secs: Option<u32>,
    /// Direction: "incoming" or "outgoing". Set by get_all_promises.
    #[serde(default)]
    pub direction: Option<String>,
}

/// Returned by `create_email_timelock` — carries both the on-chain TxId and
/// the human-readable claim code that should be emailed to the recipient.
#[derive(Debug, Serialize, Clone)]
pub struct EmailLockResult {
    pub tx_id: String,
    pub claim_code: String,
}

/// Returned by `generate_cold_wallet` — a new keypair that is NOT saved to disk.
#[derive(Debug, Serialize, Clone)]
pub struct ColdWalletResult {
    pub account_id: String,
    pub private_key_b64: String,
}

/// Input for a single entry in a Promise Series.
#[derive(Debug, Deserialize, Clone)]
pub struct SeriesEntryInput {
    pub amount_kx: f64,
    pub unlock_at_unix: i64,
    pub memo: Option<String>,
}

/// Returned by `create_email_timelock_series`.
#[derive(Debug, Serialize, Clone)]
pub struct EmailSeriesResult {
    pub tx_ids: Vec<String>,
    pub claim_code: String,
}

/// Returned by `claim_by_code` — auto-finds and claims all matching locks.
#[derive(Debug, Serialize, Clone)]
pub struct ClaimByCodeResult {
    pub tx_id: String,
    pub claimed_count: usize,
    pub total_chronos: String,
    pub lock_ids: Vec<String>,
}

// ── RPC helper ────────────────────────────────────────────────────────────────

async fn rpc_call(
    url: &str,
    method: &str,
    params: serde_json::Value,
) -> Result<serde_json::Value, String> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .map_err(|e| e.to_string())?;

    let body = serde_json::json!({
        "jsonrpc": "2.0",
        "method":  method,
        "params":  params,
        "id":      1
    });

    let resp = client
        .post(url)
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("Node unreachable ({url}): {e}"))?;

    let status = resp.status();
    let text = resp
        .text()
        .await
        .map_err(|e| format!("Reading response body: {e}"))?;

    if !status.is_success() {
        let preview = if text.len() > 200 { &text[..200] } else { &text };
        return Err(format!("HTTP {status}: {preview}"));
    }

    let json: serde_json::Value = serde_json::from_str(&text)
        .map_err(|e| {
            let preview = if text.len() > 200 { &text[..200] } else { &text };
            format!("Bad RPC response (not JSON): {e} — body: {preview}")
        })?;

    if let Some(err) = json.get("error") {
        return Err(format!("RPC error: {err}"));
    }

    Ok(json["result"].clone())
}

// ── MISAI lock_metadata encryption ───────────────────────────────────────────

/// Fetch MISAI's X25519 public key from the node. Returns None if not available.
async fn fetch_misai_x25519_pubkey(url: &str) -> Option<[u8; 32]> {
    let result = rpc_call(url, "chronx_getMisaiPubkey", serde_json::json!([])).await;
    match result {
        Ok(val) => {
            let hex_str = val.get("pubkey_hex")?.as_str()?;
            let bytes = hex::decode(hex_str).ok()?;
            if bytes.len() != 32 { return None; }
            let mut arr = [0u8; 32];
            arr.copy_from_slice(&bytes);
            Some(arr)
        }
        Err(_) => None,
    }
}

/// Encrypt a claim code for MISAI using X25519 + HKDF + ChaCha20-Poly1305.
///
/// Layout: ephemeral_pubkey (32) || nonce (12) || ciphertext+tag (48)
/// Total: 92 bytes → 184 hex chars.
///
/// The claim_code (19 bytes, e.g. "KX-ABCD-1234-EF56-7890") is zero-padded
/// to 32 bytes before encryption so the ciphertext length is fixed.
fn encrypt_claim_for_misai(
    claim_code: &str,
    misai_pubkey: &[u8; 32],
) -> Result<String, String> {
    // Zero-pad claim_code to 32 bytes
    let mut plaintext = [0u8; 32];
    let code_bytes = claim_code.as_bytes();
    if code_bytes.len() > 32 {
        return Err("claim_code too long".into());
    }
    plaintext[..code_bytes.len()].copy_from_slice(code_bytes);

    // Generate ephemeral X25519 keypair
    let ephemeral_secret = EphemeralSecret::random_from_rng(rand::rngs::OsRng);
    let ephemeral_public = X25519PublicKey::from(&ephemeral_secret);

    // X25519 key agreement
    let their_public = X25519PublicKey::from(*misai_pubkey);
    let shared_secret = ephemeral_secret.diffie_hellman(&their_public);

    // HKDF-SHA256 to derive symmetric key
    let hk = Hkdf::<sha2::Sha256>::new(Some(b"chronx-misai-v1"), shared_secret.as_bytes());
    let mut symmetric_key = [0u8; 32];
    hk.expand(&[], &mut symmetric_key)
        .map_err(|e| format!("HKDF expand failed: {e}"))?;

    // Generate random 12-byte nonce
    let mut nonce_bytes = [0u8; 12];
    rand::rngs::OsRng.fill_bytes(&mut nonce_bytes);
    let nonce = chacha20poly1305::Nonce::from_slice(&nonce_bytes);

    // ChaCha20-Poly1305 encrypt
    let cipher = ChaCha20Poly1305::new_from_slice(&symmetric_key)
        .map_err(|e| format!("cipher init: {e}"))?;
    let ciphertext = cipher.encrypt(nonce, plaintext.as_ref())
        .map_err(|e| format!("encryption failed: {e}"))?;
    // ciphertext = 32 bytes data + 16 bytes tag = 48 bytes

    // Pack: ephemeral_pubkey (32) || nonce (12) || ciphertext+tag (48) = 92 bytes
    let mut packed = Vec::with_capacity(92);
    packed.extend_from_slice(ephemeral_public.as_bytes());
    packed.extend_from_slice(&nonce_bytes);
    packed.extend_from_slice(&ciphertext);

    Ok(hex::encode(packed))
}

// ── Keypair helpers ───────────────────────────────────────────────────────────

fn load_keypair(app: &AppHandle) -> Result<KeyPair, String> {
    let path = keyfile_path(app);
    let json = std::fs::read_to_string(&path).map_err(|_| {
        format!(
            "Wallet not found at {}. Run: chronx-wallet.exe keygen",
            path.display()
        )
    })?;
    serde_json::from_str::<KeyPair>(&json).map_err(|e| format!("Corrupt keyfile: {e}"))
}

// ── Shared tx builder ─────────────────────────────────────────────────────────

async fn build_sign_mine_submit(
    kp: &KeyPair,
    actions: Vec<Action>,
    url: &str,
) -> Result<String, String> {
    let account_id_b58 = kp.account_id.to_b58();

    let nonce = {
        let res = rpc_call(url, "chronx_getAccount", serde_json::json!([account_id_b58]))
            .await
            .map_err(|e| format!("Failed to fetch nonce: {e}"))?;
        if res.is_null() { 0u64 } else { res["nonce"].as_u64().unwrap_or(0) }
    };

    let tips_hex: Vec<String> = {
        let res = rpc_call(url, "chronx_getDagTips", serde_json::json!([]))
            .await
            .map_err(|e| format!("Failed to fetch DAG tips: {e}"))?;
        serde_json::from_value(res).map_err(|e| format!("Parsing DAG tips: {e}"))?
    };
    let tips: Vec<TxId> = tips_hex
        .iter()
        .map(|h| TxId::from_hex(h).map_err(|e| format!("Bad tip hex: {e}")))
        .collect::<Result<_, _>>()?;

    let timestamp = chrono::Utc::now().timestamp();

    let body = TransactionBody {
        parents: &tips,
        timestamp,
        nonce,
        from: &kp.account_id,
        actions: &actions,
        auth_scheme: &AuthScheme::SingleSig,
    };
    let body_bytes =
        bincode::serialize(&body).map_err(|e| format!("Serialising tx body: {e}"))?;

    let body_for_pow = body_bytes.clone();
    let pow_nonce = tokio::task::spawn_blocking(move || {
        mine_pow(&body_for_pow, POW_INITIAL_DIFFICULTY)
    })
    .await
    .map_err(|e| format!("PoW thread panicked: {e}"))?;

    let signature = kp.sign(&body_bytes);
    let tx_id = tx_id_from_body(&body_bytes);

    let tx = Transaction {
        tx_id,
        parents: tips,
        timestamp,
        nonce,
        from: kp.account_id.clone(),
        actions,
        pow_nonce,
        signatures: vec![signature],
        auth_scheme: AuthScheme::SingleSig,
        tx_version: 1,
        client_ref: None,
        fee_chronos: 0,
        expires_at: None,
        sender_public_key: Some(kp.public_key.clone()),
    };

    let tx_bytes =
        bincode::serialize(&tx).map_err(|e| format!("Serialising tx: {e}"))?;
    let tx_hex = hex::encode(&tx_bytes);

    let result = rpc_call(url, "chronx_sendTransaction", serde_json::json!([tx_hex]))
        .await
        .map_err(|e| format!("Sending transaction: {e}"))?;

    result
        .as_str()
        .map(|s| s.to_string())
        .ok_or_else(|| "No TxId in response".to_string())
}

// ── Tauri commands ────────────────────────────────────────────────────────────

/// Returns true if the node RPC is reachable.
#[tauri::command]
pub async fn check_node(app: AppHandle) -> bool {
    let url = rpc_url(&app);
    rpc_call(&url, "chronx_getGenesisInfo", serde_json::json!([])).await.is_ok()
}

/// Load the local keyfile and return account info from the node.
#[tauri::command]
pub async fn get_account_info(app: AppHandle) -> Result<AccountInfo, String> {
    let url = rpc_url(&app);
    let kp = load_keypair(&app)?;
    let b58 = kp.account_id.to_b58();

    let result = rpc_call(&url, "chronx_getAccount", serde_json::json!([b58]))
        .await
        .map_err(|e| format!("RPC failed: {e}"))?;

    if result.is_null() {
        eprintln!("[get_account_info] chronx_getAccount returned null for {b58}");
        return Ok(AccountInfo {
            account_id: b58,
            balance_kx: "0".to_string(),
            balance_chronos: "0".to_string(),
            spendable_kx: "0".to_string(),
            spendable_chronos: "0".to_string(),
            nonce: 0,
        });
    }

    let nonce = result["nonce"].as_u64().unwrap_or(0);
    let balance_chronos = result["balance_chronos"].as_str().unwrap_or("0").to_string();
    let spendable_chronos = result["spendable_chronos"].as_str().unwrap_or("0").to_string();
    eprintln!(
        "[get_account_info] {} — nonce={nonce} balance_chronos={balance_chronos} spendable_chronos={spendable_chronos}",
        &b58[..8.min(b58.len())]
    );

    Ok(AccountInfo {
        account_id: result["account_id"]
            .as_str()
            .map(|s| s.to_string())
            .unwrap_or(b58),
        balance_kx: result["balance_kx"].as_str().unwrap_or("0").to_string(),
        balance_chronos,
        spendable_kx: result["spendable_kx"].as_str().unwrap_or("0").to_string(),
        spendable_chronos,
        nonce,
    })
}

/// Build, sign, mine PoW, and submit a Transfer transaction.
#[tauri::command]
pub async fn send_transfer(app: AppHandle, to: String, amount_kx: f64) -> Result<String, String> {
    let url = rpc_url(&app);
    let kp = load_keypair(&app)?;

    let to_id = AccountId::from_b58(&to)
        .map_err(|e| format!("Invalid recipient address: {e}"))?;

    if amount_kx <= 0.0 {
        return Err("Amount must be greater than 0".to_string());
    }
    let chronos = (amount_kx * CHRONOS_PER_KX as f64) as u128;

    let actions = vec![Action::Transfer { to: to_id, amount: chronos }];
    let txid = build_sign_mine_submit(&kp, actions, &url).await?;
    let now = chrono::Utc::now().timestamp();
    append_transfer_history(&app, &TxHistoryEntry {
        tx_id: txid.clone(),
        tx_type: "Transfer".to_string(),
        amount_chronos: Some(format!("{}", chronos)),
        counterparty: Some(to.clone()),
        timestamp: now,
        status: "Confirmed".to_string(),
        unlock_date: None,
        cancellation_window_secs: None,
        created_at: Some(now),
        claim_code: None,
        claim_secret_hash: None,
        recipient_registered: None,
        memo: None,
        sender_wallet: None,
        sender_email: None,
        sender_display: None,
    });
    Ok(txid)
}

/// Create a timelock commitment.
/// `unlock_at_unix` is a UTC Unix timestamp (seconds).
/// `to_pubkey_hex` is the recipient's Dilithium2 public key as hex.
/// Leave `to_pubkey_hex` as None (or empty string) to lock funds to yourself.
#[tauri::command]
pub async fn create_timelock(
    app: AppHandle,
    amount_kx: f64,
    unlock_at_unix: i64,
    memo: Option<String>,
    to_pubkey_hex: Option<String>,
    grantor_intent: Option<String>,
    risk_level: Option<u32>,
    ai_percentage: Option<u32>,
    axiom_consent_hash: Option<String>,
) -> Result<String, String> {
    let url = rpc_url(&app);
    let kp = load_keypair(&app)?;

    if amount_kx <= 0.0 {
        return Err("Amount must be greater than 0".to_string());
    }
    let chronos = (amount_kx * CHRONOS_PER_KX as f64) as u128;

    let now = chrono::Utc::now().timestamp();
    if unlock_at_unix <= now {
        return Err("Unlock date must be in the future".to_string());
    }
    // Wallet enforces 1-day minimum (protocol allows 1 second, but we protect users
    // from accidentally creating very short locks).
    const WALLET_MIN_LOCK_SECS: i64 = 86_400; // 1 day
    if unlock_at_unix < now + WALLET_MIN_LOCK_SECS {
        return Err("Unlock date must be at least 24 hours from now.".to_string());
    }

    // Resolve recipient: use provided pubkey hex, or default to self.
    let recipient = match to_pubkey_hex.as_deref() {
        Some(hex) if !hex.is_empty() => {
            let bytes = hex::decode(hex)
                .map_err(|e| format!("Invalid recipient public key (bad hex): {e}"))?;
            chronx_core::types::DilithiumPublicKey(bytes)
        }
        _ => kp.public_key.clone(),
    };

    // Truncate memo to 256 bytes.
    let memo = memo.map(|m| {
        if m.len() > 256 { m[..256].to_string() } else { m }
    });

    // Build tags (max 5 tags, each max 32 chars)
    let mut lock_tags: Vec<String> = vec![];
    if let Some(pct) = ai_percentage {
        if pct > 0 {
            lock_tags.push("ai_managed".to_string());
            lock_tags.push(format!("ai_pct:{}", pct));
            if let Some(risk) = risk_level {
                lock_tags.push(format!("risk:{}", risk));
            }
        }
    }
    if let Some(ref hash) = axiom_consent_hash {
        if !hash.is_empty() {
            lock_tags.push(format!("ax:{}", &hash[..hash.len().min(24)]));
        }
    }
    let tags_opt = if lock_tags.is_empty() { None } else { Some(lock_tags) };

    // grantor_intent → extension_data as JSON (max 1024 bytes)
    let ext_data: Option<Vec<u8>> = grantor_intent
        .as_deref()
        .filter(|s| !s.is_empty())
        .map(|s| {
            let json = format!(r#"{{"beneficiary":"{}"}}"#,
                s.replace('\\', "\\\\").replace('"', "\\\"")
            );
            let bytes = json.into_bytes();
            bytes[..bytes.len().min(1024)].to_vec()
        });

    // Cancellation window: min(time_until_unlock, 7 days)
    let now_unix = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;
    let secs_until_unlock = (unlock_at_unix - now_unix).max(0) as u32;
    let cancel_window = secs_until_unlock.min(604_800u32); // 7 days max
    let cancellation_window = if cancel_window < 60 { None } else { Some(cancel_window) };

    let is_ai = ai_percentage.map_or(false, |p| p > 0);

    let actions = vec![Action::TimeLockCreate {
        recipient,
        amount: chronos,
        unlock_at: unlock_at_unix,
        memo,
        cancellation_window_secs: cancellation_window,
        notify_recipient: None,
        tags: None,
        private: None,
        expiry_policy: None,
        split_policy: None,
        claim_attempts_max: None,
        recurring: None,
        extension_data: None,
        oracle_hint: None,
        jurisdiction_hint: None,
        governance_proposal_id: None,
        client_ref: None,
        recipient_email_hash: None,
        claim_window_secs: None,
        unclaimed_action: None,
        lock_type: if is_ai { Some("M".to_string()) } else { None },
        lock_metadata: None,
        agent_managed: if is_ai { Some(true) } else { None },
        grantor_axiom_consent_hash: axiom_consent_hash.clone(),
        investable_fraction: ai_percentage.filter(|&p| p > 0).map(|p| p as f64 / 100.0),
        risk_level,
        investment_exclusions: None,
        grantor_intent: grantor_intent.clone(),
    }];

    build_sign_mine_submit(&kp, actions, &url).await
}

/// Create an email-based timelock with a secure claim secret.
///
/// Generates a random "KX-XXXX-XXXX-XXXX-XXXX" claim code. BLAKE3(claim_code)
/// is embedded in extension_data (marker 0xC5 + 32 hash bytes) and stored on-chain.
/// The plaintext code is returned to the caller so it can be:
///   1. Emailed to the recipient via notify_email_recipient
///   2. Saved locally in email-history.json for re-sharing if needed
///
/// The recipient enters the code in their wallet to claim via TimeLockClaimWithSecret.
#[tauri::command]
pub async fn create_email_timelock(
    app: AppHandle,
    email: String,
    amount_kx: f64,
    unlock_at_unix: i64,
    memo: Option<String>,
    grantor_intent: Option<String>,
    risk_level: Option<u32>,
    ai_percentage: Option<u32>,
    axiom_consent_hash: Option<String>,
) -> Result<EmailLockResult, String> {
    use chronx_core::account::UnclaimedAction;
    // grantor_intent now stored in dedicated TimeLockCreate field

    let url = rpc_url(&app);
    let kp = load_keypair(&app)?;

    if amount_kx <= 0.0 {
        return Err("Amount must be greater than 0".to_string());
    }
    if !email.contains('@') {
        return Err("Invalid email address".to_string());
    }
    let chronos = (amount_kx * CHRONOS_PER_KX as f64) as u128;

    // unlock_at_unix == 0 means "Send Now" — immediately claimable.
    // Any positive value means "Send Later" — must be in the future.
    let now = chrono::Utc::now().timestamp();
    let unlock_at_unix = if unlock_at_unix <= 0 { now } else { unlock_at_unix };

    // ── Generate claim secret ──────────────────────────────────────────────────
    // 8 random bytes from the OS entropy source → formatted as "KX-XXXX-XXXX-XXXX-XXXX".
    // 64 bits of entropy + PoW rate limiting makes brute force completely infeasible.
    let mut secret_bytes = [0u8; 8];
    rand::rngs::OsRng.fill_bytes(&mut secret_bytes);
    let secret_hex = hex::encode(secret_bytes).to_uppercase();
    let claim_code = format!(
        "KX-{}-{}-{}-{}",
        &secret_hex[0..4],
        &secret_hex[4..8],
        &secret_hex[8..12],
        &secret_hex[12..16],
    );

    // Hash the display string itself — the node hashes claim_secret.as_bytes() on claim.
    let claim_secret_hash = blake3::hash(claim_code.as_bytes());
    let hash_bytes = claim_secret_hash.as_bytes();

    // Encode in extension_data: [0xC5 marker] + [32 bytes of hash].
    // The engine reads this marker and stores the hash in the email_claim_hashes tree.
    let mut extension_data = Vec::with_capacity(33);
    extension_data.push(0xC5u8);
    extension_data.extend_from_slice(hash_bytes);

    // BLAKE3 hash of the recipient's email (for on-chain indexing, no PII on-chain)
    let email_hash = chronx_crypto::blake3_hash(email.as_bytes());

    // Use sender's own pubkey as on-chain recipient.
    // Actual KX delivery happens via TimeLockClaimWithSecret (using the secret code).
    let recipient = kp.public_key.clone();

    let memo = memo.map(|m| {
        if m.len() > 256 { m[..256].to_string() } else { m }
    });

    // Build tags (max 5 tags, each max 32 chars)
    let mut lock_tags: Vec<String> = vec![];
    if let Some(pct) = ai_percentage {
        if pct > 0 {
            lock_tags.push("ai_managed".to_string());
            lock_tags.push(format!("ai_pct:{}", pct));
            if let Some(risk) = risk_level {
                lock_tags.push(format!("risk:{}", risk));
            }
        }
    }
    if let Some(ref hash) = axiom_consent_hash {
        if !hash.is_empty() {
            lock_tags.push(format!("ax:{}", &hash[..hash.len().min(24)]));
        }
    }
    let lp_tags_opt = if lock_tags.is_empty() { None } else { Some(lock_tags) };

    let is_ai = ai_percentage.map_or(false, |p| p > 0);

    // For Type M locks: encrypt the claim_code to MISAI's X25519 public key
    // so MISAI can autonomously claim and manage the funds.
    let lock_metadata = if is_ai {
        match fetch_misai_x25519_pubkey(&url).await {
            Some(pubkey) => match encrypt_claim_for_misai(&claim_code, &pubkey) {
                Ok(hex) => Some(hex),
                Err(e) => {
                    eprintln!("[warn] Failed to encrypt lock_metadata for MISAI: {e}");
                    None // graceful fallback — lock created without metadata
                }
            },
            None => {
                eprintln!("[warn] MISAI X25519 pubkey not available — lock_metadata will be null");
                None // graceful fallback
            }
        }
    } else {
        None
    };

    let actions = vec![Action::TimeLockCreate {
        recipient,
        amount: chronos,
        unlock_at: unlock_at_unix,
        memo,
        cancellation_window_secs: Some(86_400), // 24 hours max
        notify_recipient: Some(true),
        tags: None,
        private: None,
        expiry_policy: None,
        split_policy: None,
        claim_attempts_max: None,
        recurring: None,
        extension_data: Some(extension_data),
        oracle_hint: None,
        jurisdiction_hint: None,
        governance_proposal_id: None,
        client_ref: None,
        recipient_email_hash: Some(email_hash),
        claim_window_secs: Some(259_200), // 72 hours
        unclaimed_action: Some(UnclaimedAction::RevertToSender),
        lock_type: if is_ai { Some("M".to_string()) } else { None },
        lock_metadata,
        agent_managed: if is_ai { Some(true) } else { None },
        grantor_axiom_consent_hash: axiom_consent_hash.clone(),
        investable_fraction: ai_percentage.filter(|&p| p > 0).map(|p| p as f64 / 100.0),
        risk_level,
        investment_exclusions: None,
        grantor_intent: grantor_intent.clone(),
    }];

    let tx_id = build_sign_mine_submit(&kp, actions, &url).await?;
    Ok(EmailLockResult { tx_id, claim_code })
}

/// Fetch all timelocks for this wallet's account.
#[tauri::command]
pub async fn get_timelocks(app: AppHandle) -> Result<Vec<TimeLockInfo>, String> {
    let url = rpc_url(&app);
    let kp = load_keypair(&app)?;
    let b58 = kp.account_id.to_b58();

    eprintln!("[get_timelocks] → chronx_getTimeLockContracts({}) at {url}", &b58[..8.min(b58.len())]);

    let result = rpc_call(&url, "chronx_getTimeLockContracts", serde_json::json!([b58]))
        .await
        .map_err(|e| {
            eprintln!("[get_timelocks] RPC error: {e}");
            format!("RPC failed: {e}")
        })?;

    let raw_str = result.to_string();
    eprintln!("[get_timelocks] raw response (first 500 chars): {}", &raw_str[..raw_str.len().min(500)]);

    let raw: Vec<serde_json::Value> =
        serde_json::from_value(result).map_err(|e| {
            eprintln!("[get_timelocks] parse error: {e}");
            format!("Parsing timelocks: {e}")
        })?;

    eprintln!("[get_timelocks] parsed {} lock(s)", raw.len());

    let locks = raw
        .into_iter()
        .map(|v| parse_timelock_json(&v))
        .collect();

    Ok(locks)
}

/// Shared helper: parse a JSON Value into TimeLockInfo (used by get_timelocks and check_email_timelocks).
fn parse_timelock_json(v: &serde_json::Value) -> TimeLockInfo {
    TimeLockInfo {
        lock_id: v["lock_id"].as_str().unwrap_or("").to_string(),
        sender: v["sender"].as_str().unwrap_or("").to_string(),
        recipient_account_id: v["recipient_account_id"].as_str().unwrap_or("").to_string(),
        amount_kx: v["amount_kx"].as_str().unwrap_or("0").to_string(),
        amount_chronos: v["amount_chronos"].as_str().unwrap_or("0").to_string(),
        unlock_at: v["unlock_at"].as_i64().unwrap_or(0),
        created_at: v["created_at"].as_i64().unwrap_or(0),
        status: v["status"].as_str().unwrap_or("Pending").to_string(),
        memo: v["memo"].as_str().map(|s| s.to_string()),
        claim_secret_hash: v["claim_secret_hash"].as_str().map(|s| s.to_string()),
        cancellation_window_secs: v["cancellation_window_secs"].as_u64().map(|n| n as u32),
        direction: None,
    }
}

// ── Wallet backup / restore ───────────────────────────────────────────────────

/// Export the wallet as a base64-encoded JSON string (the "backup key").
/// The backup key encodes the full keypair JSON — treat it like a private key.
#[tauri::command]
pub async fn export_secret_key(app: AppHandle) -> Result<String, String> {
    let path = keyfile_path(&app);
    let json = std::fs::read_to_string(&path)
        .map_err(|_| "Wallet not found".to_string())?;
    Ok(base64::engine::general_purpose::STANDARD.encode(json.as_bytes()))
}

/// Restore a wallet from a base64 backup key (same format as export).
/// If a wallet already exists and `force` is not true, returns "WALLET_EXISTS_CONFIRM".
/// Returns the base58 account ID on success.
#[tauri::command]
pub async fn restore_wallet(app: AppHandle, backup_key: String, force: Option<bool>) -> Result<String, String> {
    let path = keyfile_path(&app);
    if path.exists() && !force.unwrap_or(false) {
        return Err("WALLET_EXISTS_CONFIRM".to_string());
    }
    let key = backup_key.trim().trim_start_matches('\u{FEFF}');
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(key)
        .map_err(|e| format!("Invalid backup key: {e}"))?;
    let kp: KeyPair = serde_json::from_slice(&bytes)
        .map_err(|_| "Invalid backup key — not a valid wallet file".to_string())?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("Creating wallet dir: {e}"))?;
    }
    let json = serde_json::to_string_pretty(&kp)
        .map_err(|e| format!("Serializing wallet: {e}"))?;
    std::fs::write(&path, &json).map_err(|e| format!("Writing wallet: {e}"))?;
    Ok(kp.account_id.to_b58())
}

// ── URL opener ───────────────────────────────────────────────────────────────

/// Open a URL or mailto: link using the platform-native handler.
#[tauri::command]
pub async fn open_url(app: AppHandle, url: String) -> Result<(), String> {
    use tauri_plugin_opener::OpenerExt;
    app.opener().open_url(&url, None::<&str>).map_err(|e| e.to_string())
}

/// Claim a matured timelock. `lock_id_hex` is the hex TxId of the lock.
#[tauri::command]
pub async fn claim_timelock(app: AppHandle, lock_id_hex: String) -> Result<String, String> {
    let url = rpc_url(&app);
    let kp = load_keypair(&app)?;

    let lock_txid =
        TxId::from_hex(&lock_id_hex).map_err(|e| format!("Invalid lock ID: {e}"))?;

    let actions = vec![Action::TimeLockClaim {
        lock_id: TimeLockId(lock_txid),
    }];

    build_sign_mine_submit(&kp, actions, &url).await
}

/// Claim an email-based time-lock using the plaintext claim code.
///
/// Bob receives the claim code ("KX-XXXX-XXXX-XXXX-XXXX") by email.
/// He enters it in his wallet; this function submits a TimeLockClaimWithSecret
/// transaction. The node verifies BLAKE3(claim_code) matches the stored hash
/// and transfers KX to Bob's account.
#[tauri::command]
pub async fn claim_email_timelock(
    app: AppHandle,
    lock_id_hex: String,
    claim_code: String,
) -> Result<String, String> {
    let url = rpc_url(&app);
    let kp = load_keypair(&app)?;

    let lock_txid =
        TxId::from_hex(&lock_id_hex).map_err(|e| format!("Invalid lock ID: {e}"))?;

    // Pre-validate: fetch lock and check maturity before submitting
    // (sendTransaction is fire-and-forget — it returns TxId before execution,
    //  so we must validate here to avoid false "success" messages)
    if let Ok(result) = rpc_call(&url, "chronx_getTimeLockById", serde_json::json!([lock_id_hex])).await {
        if let Ok(lock) = serde_json::from_value::<serde_json::Value>(result) {
            if !lock.is_null() {
                let now = chrono::Utc::now().timestamp();
                if let Some(unlock_at) = lock["unlock_at"].as_i64() {
                    if now < unlock_at {
                        let dt = chrono::DateTime::from_timestamp(unlock_at, 0)
                            .map(|d| d.format("%b %d, %Y %H:%M UTC").to_string())
                            .unwrap_or_else(|| "unknown".to_string());
                        return Err(format!("This promise hasn't unlocked yet. It unlocks on {dt}."));
                    }
                }
                if let Some(status) = lock["status"].as_str() {
                    if status != "Pending" {
                        if status.contains("Claimed") {
                            return Err("This code has already been claimed.".to_string());
                        }
                        if status.contains("Reverted") {
                            return Err("This code has expired \u{2014} the KX was automatically returned to the sender.".to_string());
                        }
                        return Err(format!("This lock is already {status}."));
                    }
                }
            }
        }
    }

    // Normalize: uppercase and trim so minor formatting differences don't fail.
    let normalized = claim_code.trim().to_uppercase();

    let actions = vec![Action::TimeLockClaimWithSecret {
        lock_id: TimeLockId(lock_txid),
        claim_secret: normalized,
    }];

    build_sign_mine_submit(&kp, actions, &url).await
}

/// Claim one or more timelocks using only a claim code (no Lock ID needed).
///
/// The wallet computes BLAKE3(uppercase(code)) and scans all known locks
/// (email locks for registered claim emails + pending incoming locks) for
/// matching `claim_secret_hash`. All matches are claimed in a single transaction.
#[tauri::command]
pub async fn claim_by_code(app: AppHandle, claim_code: String) -> Result<ClaimByCodeResult, String> {
    let url = rpc_url(&app);
    let kp = load_keypair(&app)?;
    let normalized = claim_code.trim().to_uppercase();

    if normalized.is_empty() {
        return Err("Enter a claim code".to_string());
    }

    // Compute the target hash: BLAKE3(normalized_code)
    let target_hash = hex::encode(blake3::hash(normalized.as_bytes()).as_bytes());

    // Look up locks directly by claim_secret_hash — no email required.
    // getCascadeDetails returns all locks sharing this hash.
    let cascade_result = rpc_call(
        &url,
        "chronx_getCascadeDetails",
        serde_json::json!([target_hash]),
    )
    .await
    .map_err(|e| format!("Failed to look up claim code: {e}"))?;

    let cascade: serde_json::Value = serde_json::from_value(cascade_result)
        .map_err(|e| format!("Invalid cascade response: {e}"))?;

    let locks_arr = cascade["locks"]
        .as_array()
        .ok_or_else(|| "Code not found. Check carefully \u{2014} should be KX-XXXX-XXXX-XXXX-XXXX with no extra characters.".to_string())?;

    if locks_arr.is_empty() {
        return Err("Code not found. Check carefully \u{2014} should be KX-XXXX-XXXX-XXXX-XXXX with no extra characters.".to_string());
    }

    let candidates: Vec<TimeLockInfo> = locks_arr
        .iter()
        .map(|v| parse_timelock_json(v))
        .collect();

    // Filter: Pending locks with matching claim_secret_hash
    let now = chrono::Utc::now().timestamp();
    let all_matches: Vec<&TimeLockInfo> = candidates
        .iter()
        .filter(|l| l.status == "Pending")
        .collect();

    if all_matches.is_empty() {
        // Determine why no pending locks
        let has_claimed = candidates.iter().any(|l| l.status == "Claimed");
        let has_reverted = candidates.iter().any(|l| l.status.contains("Reverted"));
        if has_claimed {
            return Err("This code has already been claimed.".to_string());
        }
        if has_reverted {
            return Err("This code has expired \u{2014} the KX was automatically returned to the sender.".to_string());
        }
        return Err("Code not found. Check carefully \u{2014} should be KX-XXXX-XXXX-XXXX-XXXX with no extra characters.".to_string());
    }

    // Pre-validate maturity: only claim locks that have unlocked.
    // (sendTransaction is fire-and-forget — returns TxId before execution,
    //  so immature claims would silently fail on the node.)
    let total_found = all_matches.len();
    let soonest_unlock = all_matches.iter().map(|l| l.unlock_at).min().unwrap();
    let matches: Vec<&TimeLockInfo> = all_matches.into_iter()
        .filter(|l| now >= l.unlock_at)
        .collect();

    if matches.is_empty() {
        // All matched locks exist but none have matured yet
        let dt = chrono::DateTime::from_timestamp(soonest_unlock, 0)
            .map(|d| d.format("%b %d, %Y %H:%M UTC").to_string())
            .unwrap_or_else(|| "unknown".to_string());
        return Err(format!(
            "Found {} matching promise(s), but none have unlocked yet. The earliest unlocks on {dt}.",
            total_found
        ));
    }

    // Build claim actions for all matching locks
    let actions: Vec<Action> = matches.iter().map(|l| {
        let lock_txid = TxId::from_hex(&l.lock_id).unwrap();
        Action::TimeLockClaimWithSecret {
            lock_id: TimeLockId(lock_txid),
            claim_secret: normalized.clone(),
        }
    }).collect();

    let claimed_count = actions.len();
    let total_chronos: u128 = matches.iter()
        .map(|l| l.amount_chronos.parse::<u128>().unwrap_or(0))
        .sum();
    let lock_ids: Vec<String> = matches.iter().map(|l| l.lock_id.clone()).collect();

    eprintln!("claim_by_code: submitting claim tx, count={claimed_count}, chronos={total_chronos}, locks={lock_ids:?}");
    let tx_id = build_sign_mine_submit(&kp, actions, &url).await
        .map_err(|e| {
            eprintln!("claim_by_code: submit failed: {e}");
            e
        })?;
    eprintln!("claim_by_code: tx submitted: {tx_id}");

    // Store the claim_secret_hash so we can discover sibling locks (e.g. 30d/60d
    // faucet stages) in get_pending_incoming / get_all_promises even if the user
    // hasn't registered a claim email.
    {
        let mut cfg = read_config(&app);
        let hashes = cfg.claimed_hashes.get_or_insert_with(Vec::new);
        if !hashes.contains(&target_hash) {
            hashes.push(target_hash);
            // Keep at most 50 hashes to avoid unbounded growth
            if hashes.len() > 50 {
                hashes.drain(..hashes.len() - 50);
            }
            let _ = write_config(&app, &cfg);
        }
    }

    Ok(ClaimByCodeResult {
        tx_id,
        claimed_count,
        total_chronos: total_chronos.to_string(),
        lock_ids,
    })
}

/// Cancel a timelock that is still within its cancellation window.
/// `lock_id_hex` is the hex TxId of the lock to cancel.
#[tauri::command]
pub async fn cancel_timelock(app: AppHandle, lock_id_hex: String) -> Result<String, String> {
    let url = rpc_url(&app);
    let kp = load_keypair(&app)?;

    let lock_txid =
        TxId::from_hex(&lock_id_hex).map_err(|e| format!("Invalid lock ID: {e}"))?;

    let actions = vec![Action::CancelTimeLock {
        lock_id: TimeLockId(lock_txid),
    }];

    build_sign_mine_submit(&kp, actions, &url).await
}

/// Reclaim an expired email lock whose 72-hour claim window has passed.
/// The node validates that the lock is expired and the sender matches.
/// Returns the TxId hex on success.
#[tauri::command]
pub async fn reclaim_expired_lock(app: AppHandle, lock_id_hex: String) -> Result<String, String> {
    let url = rpc_url(&app);
    let kp = load_keypair(&app)?;

    let lock_txid =
        TxId::from_hex(&lock_id_hex).map_err(|e| format!("Invalid lock ID: {e}"))?;

    let actions = vec![Action::ReclaimExpiredLock {
        lock_id: TimeLockId(lock_txid),
    }];

    build_sign_mine_submit(&kp, actions, &url).await
}

/// Return this wallet's Dilithium2 public key as hex (for sharing with others).
#[tauri::command]
pub async fn export_public_key(app: AppHandle) -> Result<String, String> {
    let kp = load_keypair(&app)?;
    Ok(hex::encode(&kp.public_key.0))
}

/// Read the currently configured node URL.
#[tauri::command]
pub async fn get_node_url(app: AppHandle) -> String {
    rpc_url(&app)
}

/// Persist a new node URL to the wallet config file.
/// Reads the existing config first to preserve all other fields.
#[tauri::command]
pub async fn set_node_url(app: AppHandle, url: String) -> Result<(), String> {
    let mut cfg = read_config(&app);
    cfg.node_url = url;
    write_config(&app, &cfg)
}

/// Generate a fresh wallet keypair and save it (first-run, mainly for Android).
/// Returns the base58 account ID. Errors if a wallet already exists.
#[tauri::command]
pub async fn generate_wallet(app: AppHandle) -> Result<String, String> {
    let path = keyfile_path(&app);
    if path.exists() {
        return Err("Wallet already exists. Import or use the existing wallet.".to_string());
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("Creating wallet dir: {e}"))?;
    }
    let kp = KeyPair::generate();
    let b58 = kp.account_id.to_b58();
    let json = serde_json::to_string_pretty(&kp).map_err(|e| e.to_string())?;
    std::fs::write(&path, json).map_err(|e| format!("Writing wallet: {e}"))?;
    Ok(b58)
}

// ── PIN commands ──────────────────────────────────────────────────────────────

/// Returns true if a PIN has been configured for this wallet.
#[tauri::command]
pub async fn check_pin_set(app: AppHandle) -> bool {
    read_config(&app).pin_hash.is_some()
}

/// Hash the given PIN with SHA-256 and store it in the wallet config.
#[tauri::command]
pub async fn set_pin(app: AppHandle, pin: String) -> Result<(), String> {
    let mut cfg = read_config(&app);
    cfg.pin_hash = Some(hash_pin(&pin));
    write_config(&app, &cfg)
}

/// Returns true if the given PIN matches the stored hash.
#[tauri::command]
pub async fn verify_pin(app: AppHandle, pin: String) -> Result<bool, String> {
    let cfg = read_config(&app);
    Ok(cfg.pin_hash.as_deref() == Some(hash_pin(&pin).as_str()))
}

/// Returns the configured PIN length (4, 6, or 8). Defaults to 4.
#[tauri::command]
pub async fn get_pin_length(app: AppHandle) -> u8 {
    read_config(&app).pin_length.unwrap_or(4)
}

/// Set the PIN length preference (4, 6, or 8).
#[tauri::command]
pub async fn set_pin_length(app: AppHandle, length: u8) -> Result<(), String> {
    if length != 4 && length != 6 && length != 8 {
        return Err("PIN length must be 4, 6, or 8".to_string());
    }
    let mut cfg = read_config(&app);
    cfg.pin_length = Some(length);
    write_config(&app, &cfg)
}

// ── Cold storage ──────────────────────────────────────────────────────────────

/// Generate a new Dilithium2 keypair for cold storage. Returns base64 of the
/// full keypair JSON and the account ID. The private key is NOT saved to disk.
#[tauri::command]
pub async fn generate_cold_wallet() -> Result<ColdWalletResult, String> {
    let kp = KeyPair::generate();
    let account_id = kp.account_id.to_b58();
    let json = serde_json::to_string(&kp).map_err(|e| e.to_string())?;
    let private_key_b64 = base64::engine::general_purpose::STANDARD.encode(json.as_bytes());
    Ok(ColdWalletResult { account_id, private_key_b64 })
}

/// Save a cold wallet account ID to the local address book.
#[tauri::command]
pub async fn save_cold_wallet(app: AppHandle, account_id: String) -> Result<(), String> {
    let mut cfg = read_config(&app);
    let mut wallets = cfg.cold_wallets.unwrap_or_default();
    if !wallets.contains(&account_id) {
        wallets.push(account_id);
    }
    cfg.cold_wallets = Some(wallets);
    write_config(&app, &cfg)
}

/// Get the list of saved cold wallet account IDs.
#[tauri::command]
pub async fn get_cold_wallets(app: AppHandle) -> Vec<String> {
    read_config(&app).cold_wallets.unwrap_or_default()
}

// ── Claim-email commands ──────────────────────────────────────────────────────

/// Return the locally-stored claim email (None if not set).
/// This email is ONLY stored on the user's device, never sent to any server.
#[tauri::command]
pub async fn get_claim_email(app: AppHandle) -> Option<String> {
    read_config(&app).claim_email
}

/// Store (or clear) the claim email in local wallet config.
/// Pass an empty string to clear.
#[tauri::command]
pub async fn set_claim_email(app: AppHandle, email: String) -> Result<(), String> {
    let mut cfg = read_config(&app);
    cfg.claim_email = if email.trim().is_empty() { None } else { Some(email.trim().to_string()) };
    write_config(&app, &cfg)
}

/// Return all locally-stored claim emails (up to 3). Never sent to any server.
#[tauri::command]
pub async fn get_claim_emails(app: AppHandle) -> Vec<String> {
    read_config(&app).claim_emails.unwrap_or_default()
}

/// Store up to 3 claim emails in local wallet config. Pass empty vec to clear all.
#[tauri::command]
pub async fn set_claim_emails(app: AppHandle, emails: Vec<String>) -> Result<(), String> {
    if emails.len() > 3 {
        return Err("Maximum 3 claim emails allowed".to_string());
    }
    let cleaned: Vec<String> = emails
        .iter()
        .map(|e| e.trim().to_string())
        .filter(|e| !e.is_empty())
        .collect();
    let mut cfg = read_config(&app);
    // Keep legacy field in sync with first entry.
    cfg.claim_email = cleaned.first().cloned();
    cfg.claim_emails = if cleaned.is_empty() { None } else { Some(cleaned) };
    write_config(&app, &cfg)
}

/// Return the list of verified email addresses.
#[tauri::command]
pub async fn get_verified_emails(app: AppHandle) -> Vec<String> {
    read_config(&app).verified_emails.unwrap_or_default()
}

/// Send a verification code to an email address via the notify API.
#[tauri::command]
pub async fn send_verify_email(app: AppHandle, email: String, wallet_id: Option<String>) -> Result<String, String> {
    let email = email.trim().to_lowercase();
    if email.is_empty() || !email.contains('@') {
        return Err("Invalid email address".to_string());
    }
    // Use wallet_id from frontend if provided, otherwise read from keyfile
    let wallet_id = if let Some(ref wid) = wallet_id {
        if !wid.is_empty() { wid.clone() } else { String::new() }
    } else {
        match std::fs::read_to_string(keyfile_path(&app)) {
            Ok(s) => {
                let kp: serde_json::Value = serde_json::from_str(&s).map_err(|e| e.to_string())?;
                kp["account_id"].as_str().unwrap_or("").to_string()
            }
            Err(e) => return Err(format!("Cannot read wallet: {e}")),
        }
    };
    if wallet_id.is_empty() {
        return Err("Wallet not initialized — no account ID found".to_string());
    }
    let client = reqwest::Client::new();
    let resp = client
        .post("https://api.chronx.io/verify-email")
        .json(&serde_json::json!({ "email": email, "wallet_id": wallet_id }))
        .send()
        .await
        .map_err(|e| format!("Network error: {e}"))?;
    let status = resp.status();
    let body: serde_json::Value = resp.json().await.map_err(|e| format!("Parse error: {e}"))?;
    if status.is_success() && body.get("sent").and_then(|v| v.as_bool()).unwrap_or(false) {
        Ok("sent".to_string())
    } else {
        let err = body.get("error").and_then(|v| v.as_str()).unwrap_or("Unknown error");
        Err(err.to_string())
    }
}

/// Confirm a verification code and save the email as verified on success.
#[tauri::command]
pub async fn confirm_verify_email(app: AppHandle, email: String, code: String, wallet_id: Option<String>) -> Result<bool, String> {
    let email = email.trim().to_lowercase();
    let code = code.trim().to_uppercase();
    let wallet_id = if let Some(ref wid) = wallet_id {
        if !wid.is_empty() { wid.clone() } else { String::new() }
    } else {
        match std::fs::read_to_string(keyfile_path(&app)) {
            Ok(s) => {
                let kp: serde_json::Value = serde_json::from_str(&s).map_err(|e| e.to_string())?;
                kp["account_id"].as_str().unwrap_or("").to_string()
            }
            Err(e) => return Err(format!("Cannot read wallet: {e}")),
        }
    };
    let client = reqwest::Client::new();
    let resp = client
        .post("https://api.chronx.io/verify-email/confirm")
        .json(&serde_json::json!({ "email": email, "code": code, "wallet_id": wallet_id }))
        .send()
        .await
        .map_err(|e| format!("Network error: {e}"))?;
    let body: serde_json::Value = resp.json().await.map_err(|e| format!("Parse error: {e}"))?;
    let verified = body.get("verified").and_then(|v| v.as_bool()).unwrap_or(false);
    if verified {
        // Save to verified_emails list in config
        let mut cfg = read_config(&app);
        let mut list = cfg.verified_emails.unwrap_or_default();
        if !list.contains(&email) {
            list.push(email.clone());
        }
        cfg.verified_emails = Some(list);
        // Also ensure the email is in claim_emails for monitoring
        let mut claims = cfg.claim_emails.unwrap_or_default();
        if !claims.contains(&email) && claims.len() < 3 {
            claims.push(email);
        }
        cfg.claim_emails = if claims.is_empty() { None } else { Some(claims.clone()) };
        cfg.claim_email = claims.first().cloned();
        write_config(&app, &cfg)?;
        Ok(true)
    } else {
        let err = body.get("error").and_then(|v| v.as_str()).unwrap_or("Invalid or expired code");
        Err(err.to_string())
    }
}

// ── Transaction history ───────────────────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct TxHistoryEntry {
    pub tx_id: String,
    pub tx_type: String,
    pub amount_chronos: Option<String>,
    pub counterparty: Option<String>,
    pub timestamp: i64,
    pub status: String,
    pub unlock_date: Option<i64>,
    #[serde(default)]
    pub cancellation_window_secs: Option<u32>,
    #[serde(default)]
    pub created_at: Option<i64>,
    /// Claim code for pending email sends (so Alice can re-share it if needed).
    #[serde(default)]
    pub claim_code: Option<String>,
    /// BLAKE3(claim_code) hex — locks sharing this hash belong to a Promise Series (cascade).
    #[serde(default)]
    pub claim_secret_hash: Option<String>,
    /// Whether the recipient email is registered (verified) in the system.
    #[serde(default)]
    pub recipient_registered: Option<bool>,
    /// Memo text attached to the transaction (if any).
    #[serde(default)]
    pub memo: Option<String>,
    /// Original sender wallet (for relay-delivered transactions).
    #[serde(default)]
    pub sender_wallet: Option<String>,
    /// Original sender email (for relay-delivered transactions).
    #[serde(default)]
    pub sender_email: Option<String>,
    /// Original sender display name (for relay-delivered transactions).
    #[serde(default)]
    pub sender_display: Option<String>,
}

/// Fetch full transaction history for this wallet.
/// Includes outgoing promises (timelocks), local transfer history, and incoming
/// transactions (transfers received, email claims, timelock claims) via the
/// `chronx_getIncomingTransfers` RPC method. All entries merged and sorted newest-first.
#[tauri::command]
pub async fn get_transaction_history(app: AppHandle) -> Result<Vec<TxHistoryEntry>, String> {
    let url = rpc_url(&app);
    let kp = load_keypair(&app)?;
    let b58 = kp.account_id.to_b58();

    // ── Outgoing timelocks ────────────────────────────────────────────────────
    // Use if-let so a transient RPC failure here doesn't abort the function and
    // hide the incoming-transfers section. Worst case: timelocks show empty.
    let locks: Vec<serde_json::Value> = rpc_call(
        &url, "chronx_getTimeLockContracts", serde_json::json!([b58])
    ).await
        .ok()
        .and_then(|v| serde_json::from_value(v).ok())
        .unwrap_or_default();

    // Load email send history for enrichment (lock_id → (email, claim_code))
    let email_map: std::collections::HashMap<String, (String, Option<String>)> = {
        let path = email_history_path(&app);
        std::fs::read_to_string(&path)
            .ok()
            .and_then(|s| serde_json::from_str::<Vec<EmailSendEntry>>(&s).ok())
            .unwrap_or_default()
            .into_iter()
            .map(|e| (e.lock_id, (e.email, e.claim_code)))
            .collect()
    };

    // ── Batch check recipient registration for all email sends ──────────────
    let mut email_registered: std::collections::HashMap<String, bool> = std::collections::HashMap::new();
    {
        let unique_emails: std::collections::HashSet<String> = email_map.values()
            .map(|(email, _)| email.to_lowercase())
            .collect();
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(5))
            .build()
            .ok();
        if let Some(client) = client {
            for email in &unique_emails {
                let check_url = format!("{}/check-email?email={}", NOTIFY_API_URL, urlencoding(email));
                if let Ok(resp) = client.get(&check_url).send().await {
                    if let Ok(json) = resp.json::<serde_json::Value>().await {
                        let registered = json.get("registered")
                            .and_then(|v| v.as_bool())
                            .unwrap_or(false);
                        email_registered.insert(email.clone(), registered);
                    }
                }
            }
        }
    }

    let mut entries: Vec<TxHistoryEntry> = locks
        .into_iter()
        .filter(|v| v["sender"].as_str() == Some(b58.as_str()))
        .map(|v| {
            let lock_id = v["lock_id"].as_str().unwrap_or("").to_string();
            let amount_chronos = Some(v["amount_chronos"]
                .as_str().unwrap_or("0").to_string());
            let created_at = v["created_at"].as_i64().unwrap_or(0);
            let unlock_at = v["unlock_at"].as_i64().unwrap_or(0);
            let is_email = email_map.contains_key(&lock_id);
            // RPC does not expose cancellation_window_secs, so compute from
            // protocol rules: email locks → 72h, locks ≥ 1 year → 24h.
            let cancellation_window_secs: Option<u32> = if is_email {
                Some(259_200) // 72 hours
            } else if unlock_at - created_at >= 365 * 86_400 {
                Some(86_400) // 24 hours
            } else {
                None
            };
            // Enrich with email and claim_code if this was an email send
            let (tx_type, counterparty, claim_code) =
                if let Some((email, code)) = email_map.get(&lock_id) {
                    ("Email Send".to_string(), Some(email.clone()), code.clone())
                } else {
                    ("Promise Sent".to_string(), v["memo"].as_str().map(|s| s.to_string()), None)
                };
            let raw_status = v["status"].as_str().unwrap_or("Pending").to_string();
            let claim_secret_hash = v["claim_secret_hash"].as_str()
                .filter(|s| !s.is_empty())
                .map(|s| s.to_string());
            let now = chrono::Utc::now().timestamp();
            // Map on-chain status to user-facing labels for email sends
            let status = if tx_type == "Email Send" {
                match raw_status.as_str() {
                    "Pending" => {
                        // If 72h claim window has passed but sweep hasn't run yet
                        if now > created_at + 259_200 {
                            "Expired \u{2014} Reclaiming".to_string()
                        } else {
                            "Pending Claim".to_string()
                        }
                    }
                    "Claimed" => "Claimed".to_string(),
                    "Expired" | "Reverted" => "Expired \u{2014} Reverted".to_string(),
                    "Cancelled" => "Cancelled".to_string(),
                    other => other.to_string(),
                }
            } else {
                raw_status
            };
            let recipient_registered = counterparty.as_ref()
                .and_then(|email| email_registered.get(&email.to_lowercase()))
                .copied();
            TxHistoryEntry {
                tx_id: lock_id,
                tx_type,
                amount_chronos,
                counterparty,
                timestamp: created_at,
                status,
                unlock_date:    Some(v["unlock_at"].as_i64().unwrap_or(0)),
                cancellation_window_secs,
                created_at:     Some(created_at),
                claim_code,
                claim_secret_hash,
                recipient_registered,
                memo: v["memo"].as_str().map(|s| s.to_string()),
                sender_wallet: None,
                sender_email: None,
                sender_display: None,
            }
        })
        .collect();

    // Also include local transfer history (send_transfer appends here)
    let local: Vec<TxHistoryEntry> = {
        let path = transfer_history_path(&app);
        std::fs::read_to_string(&path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    };
    entries.extend(local);

    // ── Incoming transactions (transfers received, claimed timelocks) ────────
    let incoming: Vec<serde_json::Value> = rpc_call(
        &url, "chronx_getIncomingTransfers", serde_json::json!([b58])
    ).await
        .ok()
        .and_then(|v| serde_json::from_value(v).ok())
        .unwrap_or_default();

    for v in incoming {
        let raw_type = v["tx_type"].as_str().unwrap_or("transfer");
        let tx_type = match raw_type {
            "email_claim"    => "Email Claimed".to_string(),
            "timelock_claim" => "Promise Kept".to_string(),
            _                => "Transfer Received".to_string(),
        };
        entries.push(TxHistoryEntry {
            tx_id: v["tx_id"].as_str().unwrap_or("").to_string(),
            tx_type,
            amount_chronos: Some(v["amount_chronos"].as_str().unwrap_or("0").to_string()),
            counterparty: Some(v["from"].as_str().unwrap_or("").to_string()),
            timestamp: v["timestamp"].as_i64().unwrap_or(0),
            status: "Confirmed".to_string(),
            unlock_date: None,
            cancellation_window_secs: None,
            created_at: Some(v["timestamp"].as_i64().unwrap_or(0)),
            claim_code: None,
            claim_secret_hash: None,
            recipient_registered: None,
            memo: v["memo"].as_str().map(|s| s.to_string()),
            sender_wallet: None,
            sender_email: None,
            sender_display: None,
        });
    }

    // ── Outgoing immediate transfers (non-timelock sends) ─────────────────────
    // chronx_getOutgoingTransfers is a node RPC added in v7.x. Graceful fallback
    // if the node doesn't support it yet (returns method-not-found or network error).
    // Deduplicates against local transfer-history.json entries already in `entries`
    // so restoring a wallet never double-counts sends made on this device.
    let local_tx_ids: std::collections::HashSet<String> =
        entries.iter().map(|e| e.tx_id.clone()).collect();

    let outgoing: Vec<serde_json::Value> = rpc_call(
        &url, "chronx_getOutgoingTransfers", serde_json::json!([b58])
    ).await
        .ok()
        .and_then(|v| serde_json::from_value(v).ok())
        .unwrap_or_default();

    for v in outgoing {
        let tx_id = v["tx_id"].as_str().unwrap_or("").to_string();
        if local_tx_ids.contains(&tx_id) {
            continue; // already present from local transfer-history.json
        }
        entries.push(TxHistoryEntry {
            tx_id,
            tx_type: "Transfer Sent".to_string(),
            amount_chronos: Some(v["amount_chronos"].as_str().unwrap_or("0").to_string()),
            counterparty: Some(v["to"].as_str().unwrap_or("").to_string()),
            timestamp: v["timestamp"].as_i64().unwrap_or(0),
            status: "Confirmed".to_string(),
            unlock_date: None,
            cancellation_window_secs: None,
            created_at: Some(v["timestamp"].as_i64().unwrap_or(0)),
            claim_code: None,
            claim_secret_hash: None,
            recipient_registered: None,
            memo: None,
            sender_wallet: None,
            sender_email: None,
            sender_display: None,
        });
    }

    entries.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));
    Ok(entries)
}

// ── App version ───────────────────────────────────────────────────────────────

/// Return the application version from Cargo.toml (mirrors tauri.conf.json version).
#[tauri::command]
pub async fn get_app_version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}

// ── Check for updates ─────────────────────────────────────────────────────────

#[derive(Debug, Serialize, Clone)]
pub struct UpdateInfo {
    pub up_to_date: bool,
    pub current: String,
    pub latest: String,
    pub download_url: String,
    pub release_notes: String,
}

/// Check the GitHub releases API for the latest wallet version.
/// Silent fail: any network or parse error returns up_to_date = true (no error shown).
#[tauri::command]
pub async fn check_for_updates() -> UpdateInfo {
    let current = env!("CARGO_PKG_VERSION").to_string();
    let silent_ok = UpdateInfo {
        up_to_date: true,
        current: current.clone(),
        latest: current.clone(),
        download_url: String::new(),
        release_notes: String::new(),
    };
    let client = match reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .user_agent("ChronX-Wallet")
        .build()
    {
        Ok(c) => c,
        Err(_) => return silent_ok,
    };
    let resp = match client
        .get("https://chronx.io/version.json")
        .send()
        .await
    {
        Ok(r) => r,
        Err(_) => return silent_ok,
    };
    let json: serde_json::Value = match resp.json().await {
        Ok(j) => j,
        Err(_) => return silent_ok,
    };
    // Use ios_version on iOS (missing/null = no listing yet → silent no-update),
    // android_version on Android, version on Windows/Linux.
    let latest = if cfg!(target_os = "ios") {
        match json["ios_version"].as_str() {
            Some(v) => v.to_string(),
            None => return silent_ok, // no iOS App Store listing yet
        }
    } else if cfg!(target_os = "android") {
        json["android_version"].as_str()
            .or_else(|| json["version"].as_str())
            .unwrap_or("")
            .to_string()
    } else {
        json["version"].as_str().unwrap_or("").to_string()
    };
    if latest.is_empty() {
        return silent_ok;
    }
    // Numeric version comparison: compare each segment as integers
    let up_to_date = {
        let cur_parts: Vec<u32> = current.split('.').filter_map(|s| s.parse().ok()).collect();
        let lat_parts: Vec<u32> = latest.split('.').filter_map(|s| s.parse().ok()).collect();
        let len = cur_parts.len().max(lat_parts.len());
        let mut is_newer_or_equal = true;
        for i in 0..len {
            let c = cur_parts.get(i).copied().unwrap_or(0);
            let l = lat_parts.get(i).copied().unwrap_or(0);
            if c > l { break; }        // current is newer → up to date
            if c < l { is_newer_or_equal = false; break; } // latest is newer → update available
        }
        is_newer_or_equal
    };
    // Platform-appropriate download URL
    let download_url = if cfg!(target_os = "ios") {
        String::new() // no App Store URL yet
    } else if cfg!(target_os = "android") {
        "https://play.google.com/store/apps/details?id=com.chronx.wallet".to_string()
    } else {
        "https://chronx.io/dl/chronx-wallet-setup.exe".to_string()
    };
    UpdateInfo { up_to_date, current, latest, download_url, release_notes: String::new() }
}

// ── Notices ───────────────────────────────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Notice {
    pub id: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub body: String,
    #[serde(default)]
    pub severity: String, // "info" | "warning" | "critical" | "reward" | "urgent" | "message"
    #[serde(default)]
    pub date: String,
    /// New API field — "urgent" or "message"
    #[serde(rename = "type", default)]
    pub notice_type: String,
    #[serde(default)]
    pub dismissible: Option<bool>,
    #[serde(default)]
    pub expires: Option<String>,
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub url_label: Option<String>,
}

// ── Email send history ───────────────────────────────────────────────────────

/// Lightweight record saved locally when the user sends KX to an email address.
/// Stores lock_id + email + claim_code so History and Promises tabs can show
/// the code and get_transaction_history can enrich the on-chain entry.
#[derive(Debug, Serialize, Deserialize, Clone)]
struct EmailSendEntry {
    lock_id:    String,
    email:      String,
    /// The "KX-XXXX-XXXX-XXXX-XXXX" claim code. Older entries may not have this field.
    #[serde(default)]
    claim_code: Option<String>,
}

fn email_history_path(app: &AppHandle) -> PathBuf {
    #[cfg(mobile)]
    {
        app.path()
            .app_data_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join("email-history.json")
    }
    #[cfg(not(mobile))]
    {
        let _ = app;
        expand_tilde("~/.chronx/email-history.json")
    }
}

/// Save an email send record locally (called after a successful create_email_timelock).
/// Stores lock_id, recipient email, and claim code so History/Promises can display the code.
/// Idempotent: duplicate lock_ids are silently skipped.
#[tauri::command]
pub async fn save_email_send(
    app: AppHandle,
    lock_id: String,
    email: String,
    claim_code: String,
) -> Result<(), String> {
    let path = email_history_path(&app);
    let mut entries: Vec<EmailSendEntry> = std::fs::read_to_string(&path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default();
    if !entries.iter().any(|e| e.lock_id == lock_id) {
        entries.push(EmailSendEntry {
            lock_id,
            email,
            claim_code: Some(claim_code),
        });
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).ok();
        }
        let json = serde_json::to_string_pretty(&entries).map_err(|e| e.to_string())?;
        std::fs::write(&path, json).map_err(|e| format!("Writing email history: {e}"))?;
    }
    Ok(())
}

fn transfer_history_path(app: &AppHandle) -> PathBuf {
    #[cfg(mobile)]
    {
        app.path()
            .app_data_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join("transfer-history.json")
    }
    #[cfg(not(mobile))]
    {
        let _ = app;
        expand_tilde("~/.chronx/transfer-history.json")
    }
}

fn append_transfer_history(app: &AppHandle, entry: &TxHistoryEntry) {
    let path = transfer_history_path(app);
    let mut entries: Vec<TxHistoryEntry> = std::fs::read_to_string(&path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default();
    entries.push(entry.clone());
    if let Ok(json) = serde_json::to_string(&entries) {
        let _ = std::fs::write(&path, json);
    }
}

fn seen_notices_path(app: &AppHandle) -> PathBuf {
    #[cfg(mobile)]
    {
        app.path()
            .app_data_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join("seen-notices.json")
    }
    #[cfg(not(mobile))]
    {
        let _ = app;
        expand_tilde("~/.chronx/seen-notices.json")
    }
}

fn read_seen_notices(app: &AppHandle) -> Vec<String> {
    let path = seen_notices_path(app);
    std::fs::read_to_string(&path)
        .ok()
        .and_then(|s| serde_json::from_str::<Vec<String>>(&s).ok())
        .unwrap_or_default()
}

/// Fetch active notices from https://api.chronx.io/notices.
#[tauri::command]
pub async fn fetch_notices(app: AppHandle) -> Result<Vec<Notice>, String> {
    let version = app.package_info().version.to_string();
    let url = format!("https://api.chronx.io/notices?version={version}");
    let resp = reqwest::get(&url)
        .await
        .map_err(|e| format!("Network error: {e}"))?;
    let json: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| format!("Parse error: {e}"))?;
    let mut notices: Vec<Notice> =
        serde_json::from_value(json["notices"].clone()).unwrap_or_default();
    // Map new API type field to severity for backward compat with frontend
    for n in &mut notices {
        if n.severity.is_empty() && !n.notice_type.is_empty() {
            n.severity = n.notice_type.clone();
        }
    }
    Ok(notices)
}

/// Return notice IDs that have already been marked as read locally.
#[tauri::command]
pub async fn get_seen_notices(app: AppHandle) -> Vec<String> {
    read_seen_notices(&app)
}

/// Persistently mark a notice as read on this device and report to server.
#[tauri::command]
pub async fn mark_notice_seen(app: AppHandle, id: String) -> Result<(), String> {
    let path = seen_notices_path(&app);
    let mut ids = read_seen_notices(&app);
    if !ids.contains(&id) {
        ids.push(id.clone());
        let json = serde_json::to_string(&ids).map_err(|e| e.to_string())?;
        std::fs::write(&path, json).map_err(|e| e.to_string())?;
    }
    // Best-effort report to server
    let url = format!("https://api.chronx.io/notices/{id}/seen");
    let _ = reqwest::Client::new().post(&url).json(&serde_json::json!({})).send().await;
    Ok(())
}

/// Report notice dismissed to server and persist locally.
#[tauri::command]
pub async fn mark_notice_dismissed(app: AppHandle, id: String) -> Result<(), String> {
    // Store in seen list so it won't show again
    let path = seen_notices_path(&app);
    let mut ids = read_seen_notices(&app);
    if !ids.contains(&id) {
        ids.push(id.clone());
        let json = serde_json::to_string(&ids).map_err(|e| e.to_string())?;
        std::fs::write(&path, json).map_err(|e| e.to_string())?;
    }
    // Best-effort report to server
    let url = format!("https://api.chronx.io/notices/{id}/dismissed");
    let _ = reqwest::Client::new().post(&url).json(&serde_json::json!({})).send().await;
    Ok(())
}

// ── Incoming promises ─────────────────────────────────────────────────────────

/// Fetch all **Pending** incoming timelocks for this wallet's account (max 20).
/// These are locks sent to us that haven't been claimed yet.
/// POST to https://api.chronx.io/notify to trigger an email notification for an email lock.
/// Fires best-effort — errors are logged but not surfaced to the user.
#[tauri::command]
pub async fn notify_email_recipient(
    app: AppHandle,
    email: String,
    amount_kx: f64,
    unlock_at_unix: i64,
    memo: Option<String>,
    claim_code: String,
) -> Result<(), String> {
    // Include sender identity so the recipient knows who sent the KX
    let sender_wallet = load_keypair(&app)
        .map(|kp| kp.account_id.to_b58())
        .ok();
    let sender_email = read_config(&app)
        .claim_emails
        .and_then(|v| v.into_iter().next());
    let client = reqwest::Client::new();
    let body = serde_json::json!({
        "to": email,
        "amount": format!("{:.6}", amount_kx),
        "unlock_at": unlock_at_unix,
        "memo": memo,
        "claim_code": claim_code,
        "sender_email": sender_email,
        "sender_wallet": sender_wallet,
    });
    let res = client
        .post("https://api.chronx.io/notify")
        .json(&body)
        .timeout(std::time::Duration::from_secs(10))
        .send()
        .await
        .map_err(|e| format!("Notify request failed: {e}"))?;
    if !res.status().is_success() {
        let status = res.status();
        let text = res.text().await.unwrap_or_default();
        return Err(format!("Notify API returned {status}: {text}"));
    }
    Ok(())
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct RewardsStatus {
    pub registered: bool,
    pub confirmed: bool,
    pub email: Option<String>,
}

#[tauri::command]
pub async fn register_for_rewards(app: AppHandle, email: String) -> Result<String, String> {
    let wallet_address = load_keypair(&app)
        .map(|kp| kp.account_id.to_string())
        .unwrap_or_default();
    let wallet_version = app.package_info().version.to_string();
    let client = reqwest::Client::new();
    let res = client
        .post("https://api.chronx.io/register")
        .json(&serde_json::json!({
            "email": email,
            "wallet_address": wallet_address,
            "wallet_version": wallet_version,
        }))
        .timeout(std::time::Duration::from_secs(10))
        .send()
        .await
        .map_err(|e| format!("Request failed: {e}"))?;
    let body: serde_json::Value = res.json().await.unwrap_or_default();
    Ok(body["status"].as_str().unwrap_or("ok").to_string())
}

#[tauri::command]
pub async fn check_rewards_status(app: AppHandle) -> Result<RewardsStatus, String> {
    let wallet_address = load_keypair(&app)
        .map(|kp| kp.account_id.to_string())
        .unwrap_or_default();
    if wallet_address.is_empty() {
        return Ok(RewardsStatus { registered: false, confirmed: false, email: None });
    }
    let client = reqwest::Client::new();
    let res = client
        .get(format!("https://api.chronx.io/rewards/status?wallet={}", wallet_address))
        .timeout(std::time::Duration::from_secs(10))
        .send()
        .await
        .map_err(|e| format!("Request failed: {e}"))?;
    let status: RewardsStatus = res.json().await.map_err(|e| format!("Parse failed: {e}"))?;
    Ok(status)
}

#[tauri::command]
pub async fn get_pending_incoming(app: AppHandle) -> Result<Vec<TimeLockInfo>, String> {
    let url = rpc_url(&app);
    let kp = load_keypair(&app)?;
    let b58 = kp.account_id.to_b58();

    let mut seen = std::collections::HashSet::new();
    let mut locks = Vec::new();

    // 1. Direct wallet-to-wallet incoming (recipient_account_id = me)
    let result = rpc_call(&url, "chronx_getPendingIncoming", serde_json::json!([b58]))
        .await
        .map_err(|e| format!("RPC failed: {e}"))?;

    let raw: Vec<serde_json::Value> =
        serde_json::from_value(result).map_err(|e| format!("Parsing incoming locks: {e}"))?;

    for v in raw.into_iter().take(20) {
        let lock = parse_timelock_json(&v);
        // Filter out self-referencing locks (sender == own wallet) — these are
        // outgoing email locks where recipient_account_id == sender, NOT real incoming.
        if lock.sender != b58 && seen.insert(lock.lock_id.clone()) {
            locks.push(lock);
        }
    }

    // 2. Email-based incoming (recipient_email_hash matches our registered emails)
    let cfg = read_config(&app);
    let emails = cfg.claim_emails.unwrap_or_default();
    for email in &emails {
        if email.trim().is_empty() {
            continue;
        }
        let email_lower = email.to_lowercase();
        let hash = blake3::hash(email_lower.as_bytes());
        let hash_hex = hex::encode(hash.as_bytes());

        if let Ok(result) = rpc_call(&url, "chronx_getEmailLocks", serde_json::json!([hash_hex])).await {
            if let Ok(raw) = serde_json::from_value::<Vec<serde_json::Value>>(result) {
                for v in raw {
                    let lock = parse_timelock_json(&v);
                    if seen.insert(lock.lock_id.clone()) {
                        locks.push(lock);
                    }
                }
            }
        }
    }

    // 3. Claimed-hash-based incoming: discover sibling locks from previous
    //    claim_by_code calls (e.g. 30d/60d faucet stages not yet matured).
    for hash_hex in cfg.claimed_hashes.unwrap_or_default() {
        if hash_hex.trim().is_empty() {
            continue;
        }
        if let Ok(result) = rpc_call(&url, "chronx_getCascadeDetails", serde_json::json!([hash_hex])).await {
            if let Ok(cascade) = serde_json::from_value::<serde_json::Value>(result) {
                if let Some(arr) = cascade["locks"].as_array() {
                    for v in arr {
                        let lock = parse_timelock_json(v);
                        // Only include pending locks from other wallets
                        if lock.status == "Pending" && lock.sender != b58 && seen.insert(lock.lock_id.clone()) {
                            locks.push(lock);
                        }
                    }
                }
            }
        }
    }

    Ok(locks)
}

/// Fetch ALL promises for the Promises tab: both outgoing (sender = me) and incoming
/// (recipient = me). Each lock is tagged with direction = "outgoing" or "incoming".
/// Deduplicates by lock_id in case a self-send appears in both sets.
#[tauri::command]
pub async fn get_all_promises(app: AppHandle) -> Result<Vec<TimeLockInfo>, String> {
    let url = rpc_url(&app);
    let kp = load_keypair(&app)?;
    let b58 = kp.account_id.to_b58();

    let mut all = Vec::new();
    let mut seen = std::collections::HashSet::new();

    // 1. Outgoing: locks where sender = my_address, pending only (future unlock date).
    //    chronx_getTimeLockContracts returns ALL outgoing locks regardless of status;
    //    we filter to Pending so the Promises tab only shows time-locked sends that
    //    have not yet been delivered.
    if let Ok(result) = rpc_call(&url, "chronx_getTimeLockContracts", serde_json::json!([b58])).await {
        if let Ok(raw) = serde_json::from_value::<Vec<serde_json::Value>>(result) {
            for v in raw {
                let mut lock = parse_timelock_json(&v);
                if lock.status != "Pending" {
                    continue; // skip delivered/claimed outgoing locks
                }
                lock.direction = Some("outgoing".to_string());
                if seen.insert(lock.lock_id.clone()) {
                    all.push(lock);
                }
            }
        }
    }

    // 2. Incoming: locks where recipient_account_id = my_address (direct wallet locks)
    if let Ok(result) = rpc_call(&url, "chronx_getPendingIncoming", serde_json::json!([b58])).await {
        if let Ok(raw) = serde_json::from_value::<Vec<serde_json::Value>>(result) {
            for v in raw {
                let mut lock = parse_timelock_json(&v);
                // Skip self-referencing locks (own outgoing email sends show up here
                // because email locks use sender's pubkey as recipient)
                if lock.sender == b58 {
                    continue;
                }
                lock.direction = Some("incoming".to_string());
                if seen.insert(lock.lock_id.clone()) {
                    all.push(lock);
                }
            }
        }
    }

    // 3. Incoming email locks: locks sent TO our registered email addresses.
    // Email locks use recipient_email_hash (not recipient_account_id) to identify
    // the recipient, so they won't appear in getPendingIncoming. We query by
    // BLAKE3(lowercase(email)) using getEmailLocks.
    let cfg = read_config(&app);
    let emails = cfg.claim_emails.unwrap_or_default();
    for email in &emails {
        if email.trim().is_empty() {
            continue;
        }
        let email_lower = email.to_lowercase();
        let hash = blake3::hash(email_lower.as_bytes());
        let hash_hex = hex::encode(hash.as_bytes());

        if let Ok(result) = rpc_call(&url, "chronx_getEmailLocks", serde_json::json!([hash_hex])).await {
            if let Ok(raw) = serde_json::from_value::<Vec<serde_json::Value>>(result) {
                for v in raw {
                    let mut lock = parse_timelock_json(&v);
                    lock.direction = Some("incoming".to_string());
                    if seen.insert(lock.lock_id.clone()) {
                        all.push(lock);
                    }
                }
            }
        }
    }

    // 4. Claimed-hash-based incoming: discover sibling locks from previous
    //    claim_by_code calls (e.g. 30d/60d faucet stages not yet matured).
    for hash_hex in cfg.claimed_hashes.unwrap_or_default() {
        if hash_hex.trim().is_empty() {
            continue;
        }
        if let Ok(result) = rpc_call(&url, "chronx_getCascadeDetails", serde_json::json!([hash_hex])).await {
            if let Ok(cascade) = serde_json::from_value::<serde_json::Value>(result) {
                if let Some(arr) = cascade["locks"].as_array() {
                    for v in arr {
                        let mut lock = parse_timelock_json(v);
                        // Only include pending locks from other wallets as incoming
                        if lock.status == "Pending" && lock.sender != b58 {
                            lock.direction = Some("incoming".to_string());
                            if seen.insert(lock.lock_id.clone()) {
                                all.push(lock);
                            }
                        }
                    }
                }
            }
        }
    }

    Ok(all)
}

/// Check the node for any email-addressed timelocks destined for the wallet's registered
/// claim emails. Scans all configured claim_emails (up to 3), deduplicates by lock_id.
/// Returns empty Vec if no claim emails are set in local config.
#[tauri::command]
pub async fn check_email_timelocks(app: AppHandle) -> Result<Vec<TimeLockInfo>, String> {
    let cfg = read_config(&app);
    let emails = cfg.claim_emails.unwrap_or_default();
    if emails.is_empty() {
        return Ok(Vec::new());
    }

    let url = rpc_url(&app);
    let mut all_locks = Vec::new();
    let mut seen_ids = std::collections::HashSet::new();

    for email in &emails {
        if email.trim().is_empty() {
            continue;
        }
        // BLAKE3(lowercase(email)) → 64-char hex
        let email_lower = email.to_lowercase();
        let hash = blake3::hash(email_lower.as_bytes());
        let hash_hex = hex::encode(hash.as_bytes());

        let result = rpc_call(&url, "chronx_getEmailLocks", serde_json::json!([hash_hex]))
            .await
            .map_err(|e| format!("RPC failed: {e}"))?;

        let raw: Vec<serde_json::Value> =
            serde_json::from_value(result).map_err(|e| format!("Parsing email locks: {e}"))?;

        for v in raw {
            let lock = parse_timelock_json(&v);
            if seen_ids.insert(lock.lock_id.clone()) {
                all_locks.push(lock);
            }
        }
    }

    Ok(all_locks)
}

// ── Promise Series commands ──────────────────────────────────────────────────

/// Create a Promise Series: multiple email-based timelocks with ONE shared claim code.
/// All locks are sent in a single transaction so the recipient only needs one code
/// to claim them all.
#[tauri::command]
pub async fn create_email_timelock_series(
    app: AppHandle,
    email: String,
    entries: Vec<SeriesEntryInput>,
) -> Result<EmailSeriesResult, String> {
    use chronx_core::account::UnclaimedAction;

    if entries.is_empty() {
        return Err("At least one entry is required".to_string());
    }
    if entries.len() > 12 {
        return Err("Maximum 12 entries per series".to_string());
    }
    if !email.contains('@') {
        return Err("Invalid email address".to_string());
    }

    let url = rpc_url(&app);
    let kp = load_keypair(&app)?;
    let now = chrono::Utc::now().timestamp();

    // Validate all entries before building any actions.
    // unlock_at_unix <= 0 means "Immediately" — map to `now` (same as single email send).
    for (i, e) in entries.iter().enumerate() {
        if e.amount_kx <= 0.0 {
            return Err(format!("Entry {}: amount must be > 0", i + 1));
        }
        // Skip time validation for "Immediately" stages (unlock_at_unix <= 0)
        if e.unlock_at_unix > 0 {
            if e.unlock_at_unix <= now {
                return Err(format!("Entry {}: unlock date must be in the future", i + 1));
            }
        }
    }

    // Generate ONE claim code shared across all locks in the series.
    let mut secret_bytes = [0u8; 8];
    rand::rngs::OsRng.fill_bytes(&mut secret_bytes);
    let secret_hex = hex::encode(secret_bytes).to_uppercase();
    let claim_code = format!(
        "KX-{}-{}-{}-{}",
        &secret_hex[0..4],
        &secret_hex[4..8],
        &secret_hex[8..12],
        &secret_hex[12..16],
    );

    let claim_secret_hash = blake3::hash(claim_code.as_bytes());
    let hash_bytes = claim_secret_hash.as_bytes();

    // Same extension_data for every lock — this is how the wallet groups them as a series.
    let mut extension_data = Vec::with_capacity(33);
    extension_data.push(0xC5u8);
    extension_data.extend_from_slice(hash_bytes);

    let email_hash = chronx_crypto::blake3_hash(email.as_bytes());
    let recipient = kp.public_key.clone();

    // Build one TimeLockCreate action per entry, all sharing the same extension_data.
    let actions: Vec<Action> = entries
        .iter()
        .map(|e| {
            let chronos = (e.amount_kx * CHRONOS_PER_KX as f64) as u128;
            let memo = e.memo.as_ref().map(|m| {
                if m.len() > 256 { m[..256].to_string() } else { m.clone() }
            });
            // Map unlock_at_unix <= 0 to `now` for "Immediately" stages
            let unlock = if e.unlock_at_unix <= 0 { now } else { e.unlock_at_unix };
            Action::TimeLockCreate {
                recipient: recipient.clone(),
                amount: chronos,
                unlock_at: unlock,
                memo,
                cancellation_window_secs: Some(259_200),
                notify_recipient: Some(true),
                tags: None,
                private: None,
                expiry_policy: None,
                split_policy: None,
                claim_attempts_max: None,
                recurring: None,
                extension_data: Some(extension_data.clone()),
                oracle_hint: None,
                jurisdiction_hint: None,
                governance_proposal_id: None,
                client_ref: None,
                recipient_email_hash: Some(email_hash),
                claim_window_secs: Some(259_200),
                unclaimed_action: Some(UnclaimedAction::RevertToSender),
                lock_type: None,
                lock_metadata: None,
                agent_managed: None,
                grantor_axiom_consent_hash: None,
                investable_fraction: None,
                risk_level: None,
                investment_exclusions: None,
                grantor_intent: None,
            }
        })
        .collect();

    let tx_id = build_sign_mine_submit(&kp, actions, &url).await?;

    // The single tx_id covers all locks. Each lock gets its own lock_id on-chain,
    // but they're all created in this one transaction.
    Ok(EmailSeriesResult {
        tx_ids: vec![tx_id],
        claim_code,
    })
}

/// Claim all locks in a Promise Series using one shared claim code.
/// Each lock_id gets a TimeLockClaimWithSecret action with the same code.
#[tauri::command]
pub async fn claim_email_series(
    app: AppHandle,
    lock_ids: Vec<String>,
    claim_code: String,
) -> Result<Vec<String>, String> {
    if lock_ids.is_empty() {
        return Err("No lock IDs provided".to_string());
    }

    let url = rpc_url(&app);
    let kp = load_keypair(&app)?;
    let normalized = claim_code.trim().to_uppercase();

    let actions: Vec<Action> = lock_ids
        .iter()
        .map(|id| {
            let lock_txid = TxId::from_hex(id).map_err(|e| format!("Invalid lock ID {id}: {e}"))?;
            Ok(Action::TimeLockClaimWithSecret {
                lock_id: TimeLockId(lock_txid),
                claim_secret: normalized.clone(),
            })
        })
        .collect::<Result<_, String>>()?;

    let tx_id = build_sign_mine_submit(&kp, actions, &url).await?;
    Ok(vec![tx_id])
}

/// Cancel all unclaimed locks in a Promise Series in one transaction.
#[tauri::command]
pub async fn cancel_timelock_series(
    app: AppHandle,
    lock_ids: Vec<String>,
) -> Result<Vec<String>, String> {
    if lock_ids.is_empty() {
        return Err("No lock IDs provided".to_string());
    }

    let url = rpc_url(&app);
    let kp = load_keypair(&app)?;

    let actions: Vec<Action> = lock_ids
        .iter()
        .map(|id| {
            let lock_txid = TxId::from_hex(id).map_err(|e| format!("Invalid lock ID {id}: {e}"))?;
            Ok(Action::CancelTimeLock {
                lock_id: TimeLockId(lock_txid),
            })
        })
        .collect::<Result<_, String>>()?;

    let tx_id = build_sign_mine_submit(&kp, actions, &url).await?;
    Ok(vec![tx_id])
}

// ── Deep-link (managed state) ────────────────────────────────────────────────

/// Reads the launch deep-link URL from managed state (set during cold start
/// via `app.deep_link().get_current()`). Returns the raw URL and clears it.
#[tauri::command]
pub async fn get_launch_deep_link(app: AppHandle) -> Option<String> {
    use tauri::Manager;
    let state = app.state::<crate::PendingDeepLink>();
    let mut pending = state.0.lock().ok()?;
    pending.take()
}

// ── Trusted Contacts ─────────────────────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct TrustedContact {
    pub email: String,
    pub wallet: Option<String>,
    pub display_name: Option<String>,
    pub added_at: u64,
}

fn trusted_contacts_path(app: &AppHandle) -> PathBuf {
    #[cfg(mobile)]
    {
        app.path()
            .app_data_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join("trusted_contacts.json")
    }
    #[cfg(not(mobile))]
    {
        let _ = app;
        expand_tilde("~/.chronx/trusted_contacts.json")
    }
}

fn read_trusted_contacts(app: &AppHandle) -> Vec<TrustedContact> {
    let path = trusted_contacts_path(app);
    std::fs::read_to_string(&path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn write_trusted_contacts(app: &AppHandle, contacts: &[TrustedContact]) -> Result<(), String> {
    let path = trusted_contacts_path(app);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("Creating dir: {e}"))?;
    }
    let json = serde_json::to_string_pretty(contacts).map_err(|e| e.to_string())?;
    std::fs::write(&path, json).map_err(|e| format!("Writing trusted contacts: {e}"))
}

#[tauri::command]
pub async fn get_trusted_contacts(app: AppHandle) -> Result<Vec<TrustedContact>, String> {
    Ok(read_trusted_contacts(&app))
}

#[tauri::command]
pub async fn add_trusted_contact(
    app: AppHandle,
    email: String,
    wallet: Option<String>,
) -> Result<(), String> {
    let mut contacts = read_trusted_contacts(&app);
    let email_lower = email.trim().to_lowercase();
    if contacts.iter().any(|c| c.email.to_lowercase() == email_lower) {
        return Ok(()); // already trusted
    }
    contacts.push(TrustedContact {
        email: email.trim().to_string(),
        wallet,
        display_name: None,
        added_at: chrono::Utc::now().timestamp() as u64,
    });
    write_trusted_contacts(&app, &contacts)
}

#[tauri::command]
pub async fn remove_trusted_contact(app: AppHandle, email: String) -> Result<(), String> {
    let mut contacts = read_trusted_contacts(&app);
    let email_lower = email.trim().to_lowercase();
    contacts.retain(|c| c.email.to_lowercase() != email_lower);
    write_trusted_contacts(&app, &contacts)
}

#[tauri::command]
pub async fn is_trusted_contact(app: AppHandle, email: String) -> Result<bool, String> {
    let contacts = read_trusted_contacts(&app);
    let email_lower = email.trim().to_lowercase();
    Ok(contacts.iter().any(|c| c.email.to_lowercase() == email_lower))
}

// ── Poke / Payment Request commands ──────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct PendingPoke {
    pub request_id: String,
    pub from_wallet: String,
    pub from_email: Option<String>,
    pub amount_kx: String,
    pub note: Option<String>,
    pub created_at: String,
    pub expires_at: String,
    pub verified_sender: bool,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct PokeResult {
    pub request_id: String,
    pub expires_at: String,
}

const NOTIFY_API_URL: &str = "https://api.chronx.io";

#[tauri::command]
pub async fn get_pending_pokes(email: String) -> Result<Vec<PendingPoke>, String> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .map_err(|e| e.to_string())?;
    let url = format!("{}/poke/pending/{}", NOTIFY_API_URL, urlencoding(&email));
    let resp = client.get(&url).send().await.map_err(|e| e.to_string())?;
    let pokes: Vec<PendingPoke> = resp.json().await.map_err(|e| e.to_string())?;
    Ok(pokes)
}

fn urlencoding(s: &str) -> String {
    s.replace('%', "%25")
        .replace(' ', "%20")
        .replace('@', "%40")
        .replace('+', "%2B")
        .replace('#', "%23")
        .replace('&', "%26")
        .replace('?', "%3F")
}

#[tauri::command]
pub async fn send_poke_request(
    from_wallet: String,
    from_email: String,
    to_email: String,
    amount_kx: f64,
    note: String,
) -> Result<PokeResult, String> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .map_err(|e| e.to_string())?;
    let body = serde_json::json!({
        "from_wallet": from_wallet,
        "from_email": from_email,
        "to_email": to_email,
        "amount_kx": amount_kx,
        "note": if note.is_empty() { None } else { Some(note) },
    });
    let resp = client
        .post(format!("{}/poke", NOTIFY_API_URL))
        .json(&body)
        .send()
        .await
        .map_err(|e| e.to_string())?;
    if resp.status() == 429 {
        return Err("Rate limit exceeded. Max 3 requests per 24 hours.".to_string());
    }
    if !resp.status().is_success() {
        let text = resp.text().await.unwrap_or_default();
        return Err(format!("Request failed: {text}"));
    }
    let result: PokeResult = resp.json().await.map_err(|e| e.to_string())?;
    Ok(result)
}

#[tauri::command]
pub async fn decline_poke(request_id: String) -> Result<(), String> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .map_err(|e| e.to_string())?;
    let body = serde_json::json!({ "request_id": request_id });
    client
        .post(format!("{}/poke/decline", NOTIFY_API_URL))
        .json(&body)
        .send()
        .await
        .map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub async fn confirm_poke_paid(request_id: String) -> Result<(), String> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .map_err(|e| e.to_string())?;
    let body = serde_json::json!({ "request_id": request_id });
    client
        .post(format!("{}/poke/paid", NOTIFY_API_URL))
        .json(&body)
        .send()
        .await
        .map_err(|e| e.to_string())?;
    Ok(())
}

// ── Blocked senders ─────────────────────────────────────────────────────────

#[tauri::command]
pub async fn get_blocked_senders(app: AppHandle) -> Vec<String> {
    read_config(&app).blocked_senders.unwrap_or_default()
}

#[tauri::command]
pub async fn add_blocked_sender(app: AppHandle, email: String) -> Result<(), String> {
    let mut cfg = read_config(&app);
    let email_lower = email.trim().to_lowercase();
    let mut list = cfg.blocked_senders.unwrap_or_default();
    if !list.iter().any(|e| e.to_lowercase() == email_lower) {
        list.push(email_lower);
    }
    cfg.blocked_senders = Some(list);
    write_config(&app, &cfg)
}

#[tauri::command]
pub async fn is_sender_blocked(app: AppHandle, email: String) -> bool {
    let blocked = read_config(&app).blocked_senders.unwrap_or_default();
    let email_lower = email.trim().to_lowercase();
    blocked.iter().any(|e| e.to_lowercase() == email_lower)
}

// ── Base (L2) wallet address for KX↔USDC conversions ────────────────────────

#[tauri::command]
pub async fn get_base_address(app: AppHandle) -> Option<String> {
    read_config(&app).base_address
}

#[tauri::command]
pub async fn set_base_address(app: AppHandle, address: String, nickname: Option<String>) -> Result<(), String> {
    let mut cfg = read_config(&app);
    let addr = address.trim().to_string();
    cfg.base_address = if addr.is_empty() { None } else { Some(addr) };
    cfg.base_address_nickname = nickname.and_then(|n| {
        let n = n.trim().to_string();
        if n.is_empty() { None } else { Some(n) }
    });
    write_config(&app, &cfg)
}

#[tauri::command]
pub async fn get_base_address_nickname(app: AppHandle) -> Option<String> {
    read_config(&app).base_address_nickname
}

#[tauri::command]
pub async fn get_base_addresses(app: AppHandle) -> Vec<SavedBaseAddress> {
    let cfg = read_config(&app);
    let mut addrs = cfg.base_addresses.unwrap_or_default();
    // Auto-migrate legacy single address if base_addresses is empty
    if addrs.is_empty() {
        if let Some(addr) = cfg.base_address {
            if !addr.trim().is_empty() {
                let nick = cfg.base_address_nickname.unwrap_or_default();
                let nick = if nick.trim().is_empty() { "Saved".to_string() } else { nick };
                addrs.push(SavedBaseAddress { address: addr, nickname: nick });
            }
        }
    }
    addrs
}

#[tauri::command]
pub async fn add_base_address(app: AppHandle, address: String, nickname: String) -> Result<(), String> {
    let mut cfg = read_config(&app);
    let mut addrs = cfg.base_addresses.clone().unwrap_or_default();
    // Auto-migrate legacy
    if addrs.is_empty() {
        if let Some(ref addr) = cfg.base_address {
            if !addr.trim().is_empty() {
                let nick = cfg.base_address_nickname.clone().unwrap_or_default();
                let nick = if nick.trim().is_empty() { "Saved".to_string() } else { nick };
                addrs.push(SavedBaseAddress { address: addr.clone(), nickname: nick });
            }
        }
    }
    if addrs.len() >= 5 {
        return Err("Maximum 5 saved addresses".into());
    }
    let addr = address.trim().to_string();
    let nick = nickname.trim().to_string();
    let nick = if nick.is_empty() { "Saved".to_string() } else { nick };
    // Deduplicate by address
    addrs.retain(|a| a.address.to_lowercase() != addr.to_lowercase());
    addrs.push(SavedBaseAddress { address: addr, nickname: nick });
    cfg.base_addresses = Some(addrs);
    write_config(&app, &cfg)
}

#[tauri::command]
pub async fn delete_base_address(app: AppHandle, address: String) -> Result<(), String> {
    let mut cfg = read_config(&app);
    let mut addrs = cfg.base_addresses.clone().unwrap_or_default();
    let addr = address.trim().to_lowercase();
    addrs.retain(|a| a.address.to_lowercase() != addr);
    cfg.base_addresses = Some(addrs);
    // Also clear legacy if it matches
    if let Some(ref old) = cfg.base_address {
        if old.trim().to_lowercase() == addr {
            cfg.base_address = None;
            cfg.base_address_nickname = None;
        }
    }
    write_config(&app, &cfg)
}

// ── KX → USDC conversion via XChan bridge ────────────────────────────────────

const XCHAN_BRIDGE_WALLET: &str = "FGSemyJdkCU85D4qQNWFd158J44MANAHTAF5Qx974WRR";

#[tauri::command]
pub async fn convert_kx_to_usdc(
    app: AppHandle,
    amount_kx: f64,
    base_address: String,
) -> Result<String, String> {
    let addr = base_address.trim().to_string();
    if !addr.starts_with("0x") || addr.len() != 42 {
        return Err("Invalid Base address".to_string());
    }
    if amount_kx <= 0.0 {
        return Err("Amount must be greater than 0".to_string());
    }

    let kp = load_keypair(&app)?;
    let url = rpc_url(&app);

    let bridge_id = AccountId::from_b58(XCHAN_BRIDGE_WALLET)
        .map_err(|e| format!("Invalid bridge wallet: {e}"))?;

    let chronos = (amount_kx * CHRONOS_PER_KX as f64) as u128;
    let actions = vec![Action::Transfer { to: bridge_id, amount: chronos }];
    let txid = build_sign_mine_submit(&kp, actions, &url).await?;

    // Notify the XChan bridge daemon of the Base destination address
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .map_err(|e| e.to_string())?;
    let sender_kx = kp.account_id.to_b58();
    let _ = client
        .post("https://api.chronx.io/xchan/convert")
        .json(&serde_json::json!({
            "tx_id": txid,
            "kx_address": sender_kx,
            "base_address": addr,
            "amount_kx": amount_kx,
        }))
        .send()
        .await;

    let now = chrono::Utc::now().timestamp();
    append_transfer_history(&app, &TxHistoryEntry {
        tx_id: txid.clone(),
        tx_type: "XChan Convert".to_string(),
        amount_chronos: Some(format!("{}", chronos)),
        counterparty: Some(format!("XChan Bridge → {}", addr)),
        timestamp: now,
        status: "Converting".to_string(),
        unlock_date: None,
        cancellation_window_secs: None,
        created_at: Some(now),
        claim_code: None,
        claim_secret_hash: None,
        recipient_registered: None,
        memo: None,
        sender_wallet: None,
        sender_email: None,
        sender_display: None,
    });

    Ok(txid)
}

// ── Poke detail by request_id ───────────────────────────────────────────────

#[tauri::command]
pub async fn get_poke_by_id(request_id: String) -> Result<PendingPoke, String> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .map_err(|e| e.to_string())?;
    let url = format!("{}/poke/{}", NOTIFY_API_URL, urlencoding(&request_id));
    let resp = client.get(&url).send().await.map_err(|e| e.to_string())?;
    if !resp.status().is_success() {
        let text = resp.text().await.unwrap_or_default();
        return Err(format!("Failed to fetch poke: {text}"));
    }
    let poke: PendingPoke = resp.json().await.map_err(|e| e.to_string())?;
    Ok(poke)
}

// ── Language preference ──────────────────────────────────────────────────────

#[tauri::command]
pub async fn get_language(app: AppHandle) -> String {
    let path = language_path(&app);
    std::fs::read_to_string(&path)
        .ok()
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|| "en".to_string())
}

#[tauri::command]
pub async fn set_language(app: AppHandle, lang: String) -> Result<(), String> {
    let path = language_path(&app);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("Creating dir: {e}"))?;
    }
    std::fs::write(&path, lang.trim()).map_err(|e| format!("Writing language: {e}"))
}

// ── Long Promise: axiom consent hash ─────────────────────────────────────────

/// Fetches the Promise Axioms text from the node RPC and returns BLAKE3 hash.
/// Used by the frontend to record the user's consent to a specific version
/// of the axioms when creating a Long Promise (>1 year).
#[tauri::command]
pub async fn get_axiom_consent_hash(app: AppHandle) -> Result<String, String> {
    let url = rpc_url(&app);
    let rpc_payload = serde_json::json!({
        "jsonrpc": "2.0", "method": "chronx_getPromiseAxioms", "params": [], "id": 1
    });
    let client = reqwest::Client::new();
    let response = client.post(&url).json(&rpc_payload).send().await
        .map_err(|e| e.to_string())?;
    let json: serde_json::Value = response.json().await
        .map_err(|e| e.to_string())?;
    let promise_axioms = json["result"]["promise_axioms"].as_str().unwrap_or("").to_string();
    let trading_axioms = json["result"]["trading_axioms"].as_str().unwrap_or("").to_string();
    let combined = format!("{}{}", promise_axioms, trading_axioms);
    let hash = blake3::hash(combined.as_bytes());
    Ok(hex::encode(hash.as_bytes()))
}

fn language_path(app: &AppHandle) -> PathBuf {
    #[cfg(mobile)]
    {
        app.path()
            .app_data_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join("language.txt")
    }
    #[cfg(not(mobile))]
    {
        let _ = app;
        expand_tilde("~/.chronx/language.txt")
    }
}

/// Create a freeform timelock — the recipient is an arbitrary string
/// (name, organisation, description) rather than a wallet address or email.
/// BLAKE3(freeform_recipient) is stored as `recipient_hash` in tags.
/// The lock is created to the sender's own pubkey (self-lock) with metadata
/// recording the intended freeform recipient.
#[tauri::command]
pub async fn create_freeform_timelock(
    app: AppHandle,
    freeform_recipient: String,
    amount_kx: f64,
    unlock_at_unix: i64,
    memo: Option<String>,
    grantor_intent: Option<String>,
    risk_level: Option<u32>,
    ai_percentage: Option<u32>,
    axiom_consent_hash: Option<String>,
) -> Result<String, String> {
    let url = rpc_url(&app);
    let kp = load_keypair(&app)?;

    if freeform_recipient.trim().is_empty() {
        return Err("Freeform recipient cannot be empty".to_string());
    }
    if amount_kx <= 0.0 {
        return Err("Amount must be greater than 0".to_string());
    }
    let chronos = (amount_kx * CHRONOS_PER_KX as f64) as u128;

    let now = chrono::Utc::now().timestamp();
    if unlock_at_unix <= now {
        return Err("Unlock date must be in the future".to_string());
    }
    const WALLET_MIN_LOCK_SECS: i64 = 86_400; // 1 day
    if unlock_at_unix < now + WALLET_MIN_LOCK_SECS {
        return Err("Unlock date must be at least 24 hours from now.".to_string());
    }

    // Truncate memo to 256 bytes.
    let memo = memo.map(|m| {
        if m.len() > 256 { m[..256].to_string() } else { m }
    });

    // Compute BLAKE3 hash of the freeform recipient string
    let recipient_hash = {
        let hash = blake3::hash(freeform_recipient.as_bytes());
        hex::encode(hash.as_bytes())
    };

    // Truncate display name for tags (max 200 chars)
    let recipient_display = if freeform_recipient.len() > 200 {
        freeform_recipient[..200].to_string()
    } else {
        freeform_recipient.clone()
    };

    // Build tags (max 5 tags, each max 32 chars)
    let mut lock_tags: Vec<String> = vec![];
    lock_tags.push("type:freeform".to_string());
    lock_tags.push(format!("hash:{}", &recipient_hash[..recipient_hash.len().min(27)]));
    if let Some(pct) = ai_percentage {
        if pct > 0 {
            lock_tags.push("ai_managed".to_string());
            lock_tags.push(format!("ai_pct:{}", pct));
            if let Some(risk) = risk_level {
                lock_tags.push(format!("risk:{}", risk));
            }
        }
    }
    let tags = Some(lock_tags);

    // Pack freeform recipient + grantor_intent into extension_data as JSON (max 1024 bytes)
    let ext_data: Option<Vec<u8>> = {
        let mut parts = vec![format!(r#""freeform_recipient":"{}""#,
            recipient_display.replace('\\', "\\\\").replace('"', "\\\""))];
        if let Some(ref note) = grantor_intent {
            if !note.is_empty() {
                let truncated = if note.len() > 1000 { &note[..1000] } else { note.as_str() };
                parts.push(format!(r#""beneficiary":"{}""#,
                    truncated.replace('\\', "\\\\").replace('"', "\\\"")));
            }
        }
        let json = format!("{{{}}}", parts.join(","));
        let bytes = json.into_bytes();
        Some(bytes[..bytes.len().min(1024)].to_vec())
    };

    // Self-lock: recipient is sender's own pubkey
    let recipient = kp.public_key.clone();

    // Cancellation window: min(time_until_unlock, 24 hours)
    let now_unix = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;
    let secs_until_unlock = (unlock_at_unix - now_unix).max(0) as u32;
    let cancel_window = secs_until_unlock.min(604_800u32); // 7 days max
    let cancellation_window = if cancel_window < 60 { None } else { Some(cancel_window) };

    let is_ai = ai_percentage.map_or(false, |p| p > 0);

    let actions = vec![Action::TimeLockCreate {
        recipient,
        amount: chronos,
        unlock_at: unlock_at_unix,
        memo,
        cancellation_window_secs: cancellation_window,
        notify_recipient: None,
        tags: None,
        private: None,
        expiry_policy: None,
        split_policy: None,
        claim_attempts_max: None,
        recurring: None,
        extension_data: None,
        oracle_hint: None,
        jurisdiction_hint: None,
        governance_proposal_id: None,
        client_ref: None,
        recipient_email_hash: None,
        claim_window_secs: None,
        unclaimed_action: None,
        lock_type: if is_ai { Some("M".to_string()) } else { Some("F".to_string()) },
        lock_metadata: None,
        agent_managed: if is_ai { Some(true) } else { None },
        grantor_axiom_consent_hash: axiom_consent_hash.clone(),
        investable_fraction: ai_percentage.filter(|&p| p > 0).map(|p| p as f64 / 100.0),
        risk_level,
        investment_exclusions: None,
        grantor_intent: grantor_intent.clone(),
    }];

    build_sign_mine_submit(&kp, actions, &url).await
}

// ── Whitelist popup: claim-info + whitelist-email ─────────────────────────────

#[derive(Serialize, Deserialize)]
pub struct ClaimInfo {
    pub found: bool,
    #[serde(default)]
    pub email: Option<String>,
    #[serde(default)]
    pub amount_kx: Option<f64>,
}

/// Fetch sender email and amount for a claim code from the notify API.
#[tauri::command]
pub async fn get_claim_info(claim_code: String) -> Result<ClaimInfo, String> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .map_err(|e| e.to_string())?;

    let url = format!(
        "{}/claim-info?code={}",
        NOTIFY_API_URL,
        urlencoding(&claim_code)
    );

    let resp = client
        .get(&url)
        .send()
        .await
        .map_err(|e| format!("Network error: {e}"))?;

    if !resp.status().is_success() {
        return Err(format!("API error: HTTP {}", resp.status()));
    }

    let info: ClaimInfo = resp.json().await.map_err(|e| format!("Parse error: {e}"))?;
    Ok(info)
}

/// Register the recipient's email in the whitelist (verified_emails) on the notify API.
#[tauri::command]
pub async fn whitelist_email(email: String, wallet_address: String) -> Result<bool, String> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .map_err(|e| e.to_string())?;

    let url = format!("{}/whitelist-email", NOTIFY_API_URL);

    let resp = client
        .post(&url)
        .json(&serde_json::json!({
            "email": email,
            "wallet_address": wallet_address,
        }))
        .send()
        .await
        .map_err(|e| format!("Network error: {e}"))?;

    if !resp.status().is_success() {
        return Err(format!("API error: HTTP {}", resp.status()));
    }

    let body: serde_json::Value = resp.json().await.map_err(|e| format!("Parse error: {e}"))?;
    Ok(body.get("success").and_then(|v| v.as_bool()).unwrap_or(false))
}

// ── Avatar & Profile ─────────────────────────────────────────────────────────

#[tauri::command]
pub async fn upload_avatar(
    wallet_address: String,
    image_path: String,
    display_name: Option<String>,
) -> Result<String, String> {
    let file_bytes = std::fs::read(&image_path)
        .map_err(|e| format!("Failed to read image: {e}"))?;

    let ext = image_path.rsplit('.').next().unwrap_or("png").to_lowercase();
    let mime = match ext.as_str() {
        "jpg" | "jpeg" => "image/jpeg",
        "png" => "image/png",
        "gif" => "image/gif",
        "webp" => "image/webp",
        _ => "image/png",
    };

    let file_part = reqwest::multipart::Part::bytes(file_bytes)
        .file_name(format!("avatar.{ext}"))
        .mime_str(mime)
        .map_err(|e| e.to_string())?;

    let mut form = reqwest::multipart::Form::new()
        .text("wallet_address", wallet_address)
        .part("image", file_part);

    if let Some(name) = display_name {
        form = form.text("display_name", name);
    }

    let client = reqwest::Client::new();
    let resp = client
        .post(format!("{}/avatar/upload", NOTIFY_API_URL))
        .multipart(form)
        .timeout(std::time::Duration::from_secs(30))
        .send()
        .await
        .map_err(|e| format!("Upload failed: {e}"))?;

    if !resp.status().is_success() {
        let body: serde_json::Value = resp.json().await.unwrap_or_default();
        return Err(body["error"].as_str().unwrap_or("Upload failed").to_string());
    }

    let body: serde_json::Value = resp.json().await.map_err(|e| format!("Parse error: {e}"))?;
    Ok(body["avatar_url"].as_str().unwrap_or("").to_string())
}

/// Upload avatar from raw base64-encoded image bytes (used by WASM frontend
/// which cannot provide a file path from an HTML file input).
#[tauri::command]
pub async fn upload_avatar_bytes(
    wallet_address: String,
    image_base64: String,
    file_name: String,
    display_name: Option<String>,
) -> Result<String, String> {
    let file_bytes = base64::engine::general_purpose::STANDARD
        .decode(&image_base64)
        .map_err(|e| format!("Invalid base64: {e}"))?;

    let ext = file_name.rsplit('.').next().unwrap_or("png").to_lowercase();
    let mime = match ext.as_str() {
        "jpg" | "jpeg" => "image/jpeg",
        "png" => "image/png",
        "gif" => "image/gif",
        "webp" => "image/webp",
        _ => "image/png",
    };

    let file_part = reqwest::multipart::Part::bytes(file_bytes)
        .file_name(format!("avatar.{ext}"))
        .mime_str(mime)
        .map_err(|e| e.to_string())?;

    let mut form = reqwest::multipart::Form::new()
        .text("wallet_address", wallet_address)
        .part("image", file_part);

    if let Some(name) = display_name {
        form = form.text("display_name", name);
    }

    let client = reqwest::Client::new();
    let resp = client
        .post(format!("{}/avatar/upload", NOTIFY_API_URL))
        .multipart(form)
        .timeout(std::time::Duration::from_secs(30))
        .send()
        .await
        .map_err(|e| format!("Upload failed: {e}"))?;

    if !resp.status().is_success() {
        let body: serde_json::Value = resp.json().await.unwrap_or_default();
        return Err(body["error"].as_str().unwrap_or("Upload failed").to_string());
    }

    let body: serde_json::Value = resp.json().await.map_err(|e| format!("Parse error: {e}"))?;
    Ok(body["avatar_url"].as_str().unwrap_or("").to_string())
}

#[tauri::command]
pub async fn get_avatar_meta(wallet_address: String) -> Result<String, String> {
    let client = reqwest::Client::new();
    let resp = client
        .get(format!("{}/avatar/{}/meta", NOTIFY_API_URL, wallet_address))
        .timeout(std::time::Duration::from_secs(10))
        .send()
        .await
        .map_err(|e| format!("Request failed: {e}"))?;
    let text = resp.text().await.map_err(|e| format!("Read failed: {e}"))?;
    Ok(text)
}

#[tauri::command]
pub async fn update_display_name(
    wallet_address: String,
    display_name: String,
) -> Result<bool, String> {
    let client = reqwest::Client::new();
    let resp = client
        .patch(format!("{}/avatar/{}/name", NOTIFY_API_URL, wallet_address))
        .json(&serde_json::json!({ "display_name": display_name }))
        .timeout(std::time::Duration::from_secs(10))
        .send()
        .await
        .map_err(|e| format!("Request failed: {e}"))?;

    if !resp.status().is_success() {
        let body: serde_json::Value = resp.json().await.unwrap_or_default();
        return Err(body["error"].as_str().unwrap_or("Update failed").to_string());
    }
    Ok(true)
}
