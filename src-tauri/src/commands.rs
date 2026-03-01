use chronx_core::{
    constants::{CHRONOS_PER_KX, POW_INITIAL_DIFFICULTY},
    transaction::{Action, AuthScheme, Transaction, TransactionBody},
    types::{AccountId, TimeLockId, TxId},
};
use chronx_crypto::{hash::tx_id_from_body, mine_pow, KeyPair};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tauri::AppHandle;

const DEFAULT_RPC_URL: &str = "http://127.0.0.1:8545";

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
}

fn read_config(app: &AppHandle) -> WalletConfig {
    let path = config_path(app);
    std::fs::read_to_string(&path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or(WalletConfig { node_url: DEFAULT_RPC_URL.to_string() })
}

fn rpc_url(app: &AppHandle) -> String {
    read_config(app).node_url
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
        return Ok(AccountInfo {
            account_id: b58,
            balance_kx: "0".to_string(),
            balance_chronos: "0".to_string(),
            spendable_kx: "0".to_string(),
            spendable_chronos: "0".to_string(),
            nonce: 0,
        });
    }

    Ok(AccountInfo {
        account_id: result["account_id"]
            .as_str()
            .map(|s| s.to_string())
            .unwrap_or(b58),
        balance_kx: result["balance_kx"].as_str().unwrap_or("0").to_string(),
        balance_chronos: result["balance_chronos"].as_str().unwrap_or("0").to_string(),
        spendable_kx: result["spendable_kx"].as_str().unwrap_or("0").to_string(),
        spendable_chronos: result["spendable_chronos"].as_str().unwrap_or("0").to_string(),
        nonce: result["nonce"].as_u64().unwrap_or(0),
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

/// Create a self-directed timelock commitment.
/// `unlock_at_unix` is a UTC Unix timestamp (seconds).
#[tauri::command]
pub async fn create_timelock(
    app: AppHandle,
    amount_kx: f64,
    unlock_at_unix: i64,
    memo: Option<String>,
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

    // Truncate memo to 256 bytes.
    let memo = memo.map(|m| {
        if m.len() > 256 { m[..256].to_string() } else { m }
    });

    let actions = vec![Action::TimeLockCreate {
        recipient: kp.public_key.clone(),
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
    }];

    build_sign_mine_submit(&kp, actions, &url).await
}

/// Fetch all timelocks for this wallet's account.
#[tauri::command]
pub async fn get_timelocks(app: AppHandle) -> Result<Vec<TimeLockInfo>, String> {
    let url = rpc_url(&app);
    let kp = load_keypair(&app)?;
    let b58 = kp.account_id.to_b58();

    let result = rpc_call(&url, "chronx_getTimeLockContracts", serde_json::json!([b58]))
        .await
        .map_err(|e| format!("RPC failed: {e}"))?;

    let raw: Vec<serde_json::Value> =
        serde_json::from_value(result).map_err(|e| format!("Parsing timelocks: {e}"))?;

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
    let path = config_path(&app);
    let mut cfg = read_config(&app);
    cfg.node_url = url;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("Creating config dir: {e}"))?;
    }
    let json = serde_json::to_string_pretty(&cfg).map_err(|e| e.to_string())?;
    std::fs::write(&path, json).map_err(|e| format!("Writing config: {e}"))
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
