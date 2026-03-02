use base64::Engine as _;
use sha2::{Sha256, Digest};
#[cfg(target_os = "android")]
use tauri::Manager;
use chronx_core::{
    constants::{CHRONOS_PER_KX, MIN_LOCK_DURATION_SECS, POW_INITIAL_DIFFICULTY},
    transaction::{Action, AuthScheme, Transaction, TransactionBody},
    types::{AccountId, TimeLockId, TxId},
};
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
}

fn read_config(app: &AppHandle) -> WalletConfig {
    let path = config_path(app);
    std::fs::read_to_string(&path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or(WalletConfig { node_url: DEFAULT_RPC_URL.to_string(), pin_hash: None })
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
    pub unlock_at: i64,
    pub created_at: i64,
    pub status: String,
    pub memo: Option<String>,
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
    build_sign_mine_submit(&kp, actions, &url).await
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
    if unlock_at_unix < now + MIN_LOCK_DURATION_SECS {
        let mins = MIN_LOCK_DURATION_SECS / 60;
        return Err(format!("Unlock time must be at least {mins} minutes from now (chain minimum)"));
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

/// Create an email-based timelock. The recipient is identified by email hash.
/// Uses the sender's own pubkey as the on-chain recipient (the claim process
/// is handled off-chain). Sets a 72-hour claim window with auto-revert.
#[tauri::command]
pub async fn create_email_timelock(
    app: AppHandle,
    email: String,
    amount_kx: f64,
    unlock_at_unix: i64,
    memo: Option<String>,
) -> Result<String, String> {
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
    if unlock_at_unix < now + MIN_LOCK_DURATION_SECS {
        let mins = MIN_LOCK_DURATION_SECS / 60;
        return Err(format!("Unlock time must be at least {mins} minutes from now"));
    }

    // BLAKE3 hash of the recipient's email
    let email_hash = chronx_crypto::blake3_hash(email.as_bytes());

    // Use sender's own pubkey as on-chain recipient — claim is handled off-chain
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
        extension_data: None,
        oracle_hint: None,
        jurisdiction_hint: None,
        governance_proposal_id: None,
        client_ref: None,
        recipient_email_hash: Some(email_hash),
        claim_window_secs: Some(259_200), // 72 hours
        unclaimed_action: Some(UnclaimedAction::RevertToSender),
    }];

    build_sign_mine_submit(&kp, actions, &url).await
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
        .map(|v| TimeLockInfo {
            lock_id: v["lock_id"].as_str().unwrap_or("").to_string(),
            sender: v["sender"].as_str().unwrap_or("").to_string(),
            recipient_account_id: v["recipient_account_id"].as_str().unwrap_or("").to_string(),
            amount_kx: v["amount_kx"].as_str().unwrap_or("0").to_string(),
            unlock_at: v["unlock_at"].as_i64().unwrap_or(0),
            created_at: v["created_at"].as_i64().unwrap_or(0),
            status: v["status"].as_str().unwrap_or("Pending").to_string(),
            memo: v["memo"].as_str().map(|s| s.to_string()),
        })
        .collect();

    Ok(locks)
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
}

/// Fetch Promise (timelock) history for this wallet.
/// Regular Transfer entries are omitted — the node does not return transfer amounts,
/// so they provide no useful information. Self-sends were also excluded by the previous
/// `action_count < 2` filter (the node counts self-transfers with action_count >= 2).
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

    let mut entries: Vec<TxHistoryEntry> = locks
        .into_iter()
        .filter(|v| v["sender"].as_str() == Some(b58.as_str()))
        .map(|v| {
            let amount_chronos = v["amount_kx"]
                .as_str()
                .and_then(|s| s.parse::<f64>().ok())
                .map(|kx| format!("{}", (kx * CHRONOS_PER_KX as f64) as u128));
            let created_at = v["created_at"].as_i64().unwrap_or(0);
            let cancellation_window_secs = v["cancellation_window_secs"]
                .as_u64().map(|w| w as u32);
            TxHistoryEntry {
                tx_id:          v["lock_id"].as_str().unwrap_or("").to_string(),
                tx_type:        "Promise Sent".to_string(),
                amount_chronos,
                counterparty:   v["memo"].as_str().map(|s| s.to_string()),
                timestamp:      created_at,
                status:         v["status"].as_str().unwrap_or("Pending").to_string(),
                unlock_date:    Some(v["unlock_at"].as_i64().unwrap_or(0)),
                cancellation_window_secs,
                created_at:     Some(created_at),
            }
        })
        .collect();

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

/// Fetch https://chronx.io/version.json and compare with this build's version.
#[tauri::command]
pub async fn check_for_updates() -> Result<UpdateInfo, String> {
    const FALLBACK: &str =
        "Unable to check for updates \u{2014} please visit chronx.io for the latest version";
    let current = env!("CARGO_PKG_VERSION").to_string();
    let resp = reqwest::get("https://chronx.io/version.json")
        .await
        .map_err(|_| FALLBACK.to_string())?;
    let text = resp.text().await.map_err(|_| FALLBACK.to_string())?;
    let json: serde_json::Value =
        serde_json::from_str(&text).map_err(|_| FALLBACK.to_string())?;
    let latest = json["version"].as_str().unwrap_or("").to_string();
    let download_url = json["download_url"]
        .as_str()
        .unwrap_or("https://chronx.io/wallet")
        .to_string();
    let release_notes = json["release_notes"].as_str().unwrap_or("").to_string();
    let up_to_date = latest.is_empty() || latest == current;
    Ok(UpdateInfo { up_to_date, current, latest, download_url, release_notes })
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
        .map(|v| TimeLockInfo {
            lock_id: v["lock_id"].as_str().unwrap_or("").to_string(),
            sender: v["sender"].as_str().unwrap_or("").to_string(),
            recipient_account_id: v["recipient_account_id"].as_str().unwrap_or("").to_string(),
            amount_kx: v["amount_kx"].as_str().unwrap_or("0").to_string(),
            unlock_at: v["unlock_at"].as_i64().unwrap_or(0),
            created_at: v["created_at"].as_i64().unwrap_or(0),
            status: v["status"].as_str().unwrap_or("Pending").to_string(),
            memo: v["memo"].as_str().map(|s| s.to_string()),
        })
        .collect();

    Ok(locks)
}
