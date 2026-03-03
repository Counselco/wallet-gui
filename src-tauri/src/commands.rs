use base64::Engine as _;
use sha2::{Sha256, Digest};
#[cfg(target_os = "android")]
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
    #[cfg(target_os = "android")]
    {
        app.path()
            .app_data_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join("wallet.json")
    }
    #[cfg(not(target_os = "android"))]
    {
        let _ = app; // unused on desktop
        expand_tilde("~/.chronx/wallet.json")
    }
}

fn config_path(app: &AppHandle) -> PathBuf {
    #[cfg(target_os = "android")]
    {
        app.path()
            .app_data_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join("wallet-config.json")
    }
    #[cfg(not(target_os = "android"))]
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
    /// Legacy single claim email (kept for backward compat with old config files).
    #[serde(default)]
    claim_email: Option<String>,
    /// Up to 3 claim email addresses stored locally for incoming email-lock discovery.
    #[serde(default)]
    claim_emails: Option<Vec<String>>,
}

fn read_config(app: &AppHandle) -> WalletConfig {
    let path = config_path(app);
    let mut cfg: WalletConfig = std::fs::read_to_string(&path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or(WalletConfig {
            node_url: DEFAULT_RPC_URL.to_string(),
            pin_hash: None,
            claim_email: None,
            claim_emails: None,
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
}

/// Returned by `create_email_timelock` — carries both the on-chain TxId and
/// the human-readable claim code that should be emailed to the recipient.
#[derive(Debug, Serialize, Clone)]
pub struct EmailLockResult {
    pub tx_id: String,
    pub claim_code: String,
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
        .timeout(std::time::Duration::from_secs(5))
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

    let json: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| format!("Bad RPC response: {e}"))?;

    if let Some(err) = json.get("error") {
        return Err(format!("RPC error: {err}"));
    }

    Ok(json["result"].clone())
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

    let actions = vec![Action::TimeLockCreate {
        recipient,
        amount: chronos,
        unlock_at: unlock_at_unix,
        memo,
        cancellation_window_secs: None,
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
) -> Result<EmailLockResult, String> {
    use chronx_core::account::UnclaimedAction;

    let url = rpc_url(&app);
    let kp = load_keypair(&app)?;

    if amount_kx <= 0.0 {
        return Err("Amount must be greater than 0".to_string());
    }
    if !email.contains('@') {
        return Err("Invalid email address".to_string());
    }
    let chronos = (amount_kx * CHRONOS_PER_KX as f64) as u128;

    let now = chrono::Utc::now().timestamp();
    if unlock_at_unix <= now {
        return Err("Unlock date must be in the future".to_string());
    }
    // Wallet enforces 1-day minimum for email locks too.
    const WALLET_MIN_LOCK_SECS: i64 = 86_400; // 1 day
    if unlock_at_unix < now + WALLET_MIN_LOCK_SECS {
        return Err("Unlock date must be at least 24 hours from now.".to_string());
    }

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

    let actions = vec![Action::TimeLockCreate {
        recipient,
        amount: chronos,
        unlock_at: unlock_at_unix,
        memo,
        cancellation_window_secs: Some(259_200), // 72 hours — sender can cancel anytime
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

/// Restore a wallet from a base64 backup key. Errors if a wallet already exists.
/// Returns the base58 account ID on success.
#[tauri::command]
pub async fn restore_wallet(app: AppHandle, backup_key: String) -> Result<String, String> {
    let path = keyfile_path(&app);
    if path.exists() {
        return Err("A wallet already exists on this device.".to_string());
    }
    let json_bytes = base64::engine::general_purpose::STANDARD
        .decode(backup_key.trim())
        .map_err(|_| "Invalid backup key — could not decode".to_string())?;
    let json = String::from_utf8(json_bytes)
        .map_err(|_| "Invalid backup key — not valid UTF-8".to_string())?;
    let kp: KeyPair = serde_json::from_str(&json)
        .map_err(|_| "Invalid backup key — not a valid wallet file".to_string())?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("Creating wallet dir: {e}"))?;
    }
    std::fs::write(&path, &json).map_err(|e| format!("Writing wallet: {e}"))?;
    Ok(kp.account_id.to_b58())
}

// ── URL opener ───────────────────────────────────────────────────────────────

/// Open a URL or mailto: link using the platform-native handler.
#[tauri::command]
pub async fn open_url(url: String) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("cmd")
            .args(["/c", "start", "", &url])
            .spawn()
            .map_err(|e| e.to_string())?;
    }
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg(&url)
            .spawn()
            .map_err(|e| e.to_string())?;
    }
    #[cfg(target_os = "linux")]
    {
        std::process::Command::new("xdg-open")
            .arg(&url)
            .spawn()
            .map_err(|e| e.to_string())?;
    }
    #[cfg(target_os = "android")]
    {
        let _ = url;
    }
    Ok(())
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

    // Gather candidate locks from multiple sources
    let mut candidates: Vec<TimeLockInfo> = Vec::new();
    let mut seen = std::collections::HashSet::new();

    // Source 1: email locks for registered claim emails
    let cfg = read_config(&app);
    let emails = cfg.claim_emails.unwrap_or_default();
    for email in &emails {
        if email.trim().is_empty() { continue; }
        let hash_hex = hex::encode(blake3::hash(email.to_lowercase().as_bytes()).as_bytes());
        if let Ok(result) = rpc_call(&url, "chronx_getEmailLocks", serde_json::json!([hash_hex])).await {
            if let Ok(raw) = serde_json::from_value::<Vec<serde_json::Value>>(result) {
                for v in raw {
                    let lock = parse_timelock_json(&v);
                    if seen.insert(lock.lock_id.clone()) {
                        candidates.push(lock);
                    }
                }
            }
        }
    }

    // Source 2: pending incoming locks for this wallet
    let b58 = kp.account_id.to_b58();
    if let Ok(result) = rpc_call(&url, "chronx_getPendingIncoming", serde_json::json!([b58])).await {
        if let Ok(raw) = serde_json::from_value::<Vec<serde_json::Value>>(result) {
            for v in raw {
                let lock = parse_timelock_json(&v);
                if seen.insert(lock.lock_id.clone()) {
                    candidates.push(lock);
                }
            }
        }
    }

    // Source 3: this wallet's own timelocks (for self-sends)
    if let Ok(result) = rpc_call(&url, "chronx_getTimeLockContracts", serde_json::json!([b58])).await {
        if let Ok(raw) = serde_json::from_value::<Vec<serde_json::Value>>(result) {
            for v in raw {
                let lock = parse_timelock_json(&v);
                if seen.insert(lock.lock_id.clone()) {
                    candidates.push(lock);
                }
            }
        }
    }

    // Filter: Pending locks with matching claim_secret_hash
    let matches: Vec<&TimeLockInfo> = candidates.iter()
        .filter(|l| l.status == "Pending"
            && l.claim_secret_hash.as_deref() == Some(target_hash.as_str()))
        .collect();

    if matches.is_empty() {
        return Err("No matching locks found for this code. Make sure your claim email is set in Settings.".to_string());
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

    let tx_id = build_sign_mine_submit(&kp, actions, &url).await?;

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

    // ── Promise (timelock) transactions only ─────────────────────────────────
    let tl_result = rpc_call(
        &url, "chronx_getTimeLockContracts", serde_json::json!([b58])
    ).await.map_err(|e| format!("RPC failed: {e}"))?;

    let locks: Vec<serde_json::Value> =
        serde_json::from_value(tl_result).map_err(|e| format!("Parsing history: {e}"))?;

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
            // Map on-chain status to user-facing labels for email sends
            let status = if tx_type == "Email Send" {
                match raw_status.as_str() {
                    "Pending" => "Pending Claim".to_string(),
                    "Claimed" => "Claimed".to_string(),
                    "Expired" | "Reverted" => "Expired \u{2014} Reverted".to_string(),
                    other => other.to_string(),
                }
            } else {
                raw_status
            };
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
    let incoming_result = rpc_call(
        &url, "chronx_getIncomingTransfers", serde_json::json!([b58])
    ).await;

    if let Ok(incoming_val) = incoming_result {
        let incoming: Vec<serde_json::Value> =
            serde_json::from_value(incoming_val).unwrap_or_default();
        for v in incoming {
            let raw_type = v["tx_type"].as_str().unwrap_or("transfer");
            let tx_type = match raw_type {
                "email_claim"   => "Email Claimed".to_string(),
                "timelock_claim" => "Promise Kept".to_string(),
                _               => "Transfer Received".to_string(),
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
            });
        }
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
        .get("https://api.github.com/repos/Counselco/wallet-gui/releases/latest")
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
    let tag = json["tag_name"].as_str().unwrap_or("");
    let latest = tag.trim_start_matches('v').to_string();
    if latest.is_empty() {
        return silent_ok;
    }
    let up_to_date = latest == current;
    let download_url = "https://chronx.io/dl/chronx-wallet-setup.exe".to_string();
    UpdateInfo { up_to_date, current, latest, download_url, release_notes: String::new() }
}

// ── Notices ───────────────────────────────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Notice {
    pub id: String,
    pub title: String,
    pub body: String,
    pub severity: String, // "info" | "warning" | "critical"
    pub date: String,
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
    #[cfg(target_os = "android")]
    {
        app.path()
            .app_data_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join("email-history.json")
    }
    #[cfg(not(target_os = "android"))]
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
    #[cfg(target_os = "android")]
    {
        app.path()
            .app_data_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join("transfer-history.json")
    }
    #[cfg(not(target_os = "android"))]
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
    #[cfg(target_os = "android")]
    {
        app.path()
            .app_data_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join("seen-notices.json")
    }
    #[cfg(not(target_os = "android"))]
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

/// Fetch all server notices from https://chronx.io/notices.json.
#[tauri::command]
pub async fn fetch_notices() -> Result<Vec<Notice>, String> {
    let resp = reqwest::get("https://chronx.io/notices.json")
        .await
        .map_err(|e| format!("Network error: {e}"))?;
    let json: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| format!("Parse error: {e}"))?;
    let notices: Vec<Notice> =
        serde_json::from_value(json["notices"].clone()).unwrap_or_default();
    Ok(notices)
}

/// Return notice IDs that have already been marked as read locally.
#[tauri::command]
pub async fn get_seen_notices(app: AppHandle) -> Vec<String> {
    read_seen_notices(&app)
}

/// Persistently mark a notice as read on this device.
#[tauri::command]
pub async fn mark_notice_seen(app: AppHandle, id: String) -> Result<(), String> {
    let path = seen_notices_path(&app);
    let mut ids = read_seen_notices(&app);
    if !ids.contains(&id) {
        ids.push(id);
        let json = serde_json::to_string(&ids).map_err(|e| e.to_string())?;
        std::fs::write(&path, json).map_err(|e| e.to_string())?;
    }
    Ok(())
}

// ── Incoming promises ─────────────────────────────────────────────────────────

/// Fetch all **Pending** incoming timelocks for this wallet's account (max 20).
/// These are locks sent to us that haven't been claimed yet.
/// POST to https://api.chronx.io/notify to trigger an email notification for an email lock.
/// Fires best-effort — errors are logged but not surfaced to the user.
#[tauri::command]
pub async fn notify_email_recipient(
    email: String,
    amount_kx: f64,
    unlock_at_unix: i64,
    memo: Option<String>,
    claim_code: String,
) -> Result<(), String> {
    let client = reqwest::Client::new();
    // The API includes claim_code in the email body and then FORGETS it — never stored in DB.
    let body = serde_json::json!({
        "to": email,
        "amount": format!("{:.6}", amount_kx),
        "unlock_at": unlock_at_unix,
        "memo": memo,
        "claim_code": claim_code,
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

    let result = rpc_call(&url, "chronx_getPendingIncoming", serde_json::json!([b58]))
        .await
        .map_err(|e| format!("RPC failed: {e}"))?;

    let raw: Vec<serde_json::Value> =
        serde_json::from_value(result).map_err(|e| format!("Parsing incoming locks: {e}"))?;

    let locks = raw
        .into_iter()
        .take(20)
        .map(|v| parse_timelock_json(&v))
        .collect();

    Ok(locks)
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
    for (i, e) in entries.iter().enumerate() {
        if e.amount_kx <= 0.0 {
            return Err(format!("Entry {}: amount must be > 0", i + 1));
        }
        if e.unlock_at_unix <= now {
            return Err(format!("Entry {}: unlock date must be in the future", i + 1));
        }
        if e.unlock_at_unix < now + 86_400 {
            return Err(format!("Entry {}: unlock date must be at least 24 hours from now", i + 1));
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
            Action::TimeLockCreate {
                recipient: recipient.clone(),
                amount: chronos,
                unlock_at: e.unlock_at_unix,
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

// ── Deep-link claim code ─────────────────────────────────────────────────────

#[tauri::command]
pub async fn get_pending_deep_link(_app: AppHandle) -> Option<String> {
    let path = expand_tilde("~/.chronx/pending-deep-link.txt");
    if path.exists() {
        let code = std::fs::read_to_string(&path).ok()?;
        let _ = std::fs::remove_file(&path); // consume it
        let code = code.trim().to_string();
        if code.is_empty() { None } else { Some(code) }
    } else {
        None
    }
}
