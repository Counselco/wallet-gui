use chronx_core::{
    constants::{CHRONOS_PER_KX, POW_INITIAL_DIFFICULTY},
    transaction::{Action, AuthScheme, Transaction, TransactionBody},
    types::{AccountId, TxId},
};
use chronx_crypto::{hash::tx_id_from_body, mine_pow, KeyPair};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

const RPC_URL: &str = "http://127.0.0.1:8545";
const KEYFILE_DEFAULT: &str = "~/.chronx/wallet.json";

// ── Types returned to the frontend ───────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct AccountInfo {
    pub account_id: String,
    pub balance_kx: String,
    pub balance_chronos: String,
    pub nonce: u64,
}

// ── Helpers ───────────────────────────────────────────────────────────────────

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

async fn rpc_call(
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
        .post(RPC_URL)
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("Node unreachable ({RPC_URL}): {e}"))?;

    let json: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| format!("Bad RPC response: {e}"))?;

    if let Some(err) = json.get("error") {
        return Err(format!("RPC error: {err}"));
    }

    Ok(json["result"].clone())
}

fn load_keypair() -> Result<KeyPair, String> {
    let path = expand_tilde(KEYFILE_DEFAULT);
    let json = std::fs::read_to_string(&path).map_err(|_| {
        format!(
            "Wallet not found at {}. Run: chronx-wallet.exe keygen",
            path.display()
        )
    })?;
    serde_json::from_str::<KeyPair>(&json)
        .map_err(|e| format!("Corrupt keyfile: {e}"))
}

// ── Tauri commands ────────────────────────────────────────────────────────────

/// Returns true if the node RPC is reachable.
#[tauri::command]
pub async fn check_node() -> bool {
    rpc_call("chronx_getGenesisInfo", serde_json::json!([])).await.is_ok()
}

/// Load the local keyfile and return account info from the node.
#[tauri::command]
pub async fn get_account_info() -> Result<AccountInfo, String> {
    let kp = load_keypair()?;
    let b58 = kp.account_id.to_b58();

    let result = rpc_call("chronx_getAccount", serde_json::json!([b58]))
        .await
        .map_err(|e| format!("RPC failed: {e}"))?;

    // Account may not yet exist on-chain (no transactions sent).
    if result.is_null() {
        return Ok(AccountInfo {
            account_id: b58,
            balance_kx: "0".to_string(),
            balance_chronos: "0".to_string(),
            nonce: 0,
        });
    }

    Ok(AccountInfo {
        account_id: result["account_id"]
            .as_str()
            .map(|s| s.to_string())
            .unwrap_or(b58),
        balance_kx: result["balance_kx"]
            .as_str()
            .unwrap_or("0")
            .to_string(),
        balance_chronos: result["balance_chronos"]
            .as_str()
            .unwrap_or("0")
            .to_string(),
        nonce: result["nonce"].as_u64().unwrap_or(0),
    })
}

/// Build, sign, mine PoW, and submit a Transfer transaction.
/// Returns the hex TxId on success.
#[tauri::command]
pub async fn send_transfer(to: String, amount_kx: f64) -> Result<String, String> {
    let kp = load_keypair()?;
    let account_id_b58 = kp.account_id.to_b58();

    let to_id = AccountId::from_b58(&to)
        .map_err(|e| format!("Invalid recipient address: {e}"))?;

    if amount_kx <= 0.0 {
        return Err("Amount must be greater than 0".to_string());
    }
    let chronos = (amount_kx * CHRONOS_PER_KX as f64) as u128;

    // Fetch current nonce.
    let nonce = {
        let res = rpc_call("chronx_getAccount", serde_json::json!([account_id_b58]))
            .await
            .map_err(|e| format!("Failed to fetch nonce: {e}"))?;
        if res.is_null() { 0u64 } else { res["nonce"].as_u64().unwrap_or(0) }
    };

    // Fetch DAG tips.
    let tips_hex: Vec<String> = {
        let res = rpc_call("chronx_getDagTips", serde_json::json!([]))
            .await
            .map_err(|e| format!("Failed to fetch DAG tips: {e}"))?;
        serde_json::from_value(res)
            .map_err(|e| format!("Parsing DAG tips: {e}"))?
    };
    let tips: Vec<TxId> = tips_hex
        .iter()
        .map(|h| TxId::from_hex(h).map_err(|e| format!("Bad tip hex: {e}")))
        .collect::<Result<_, _>>()?;

    let actions = vec![Action::Transfer { to: to_id, amount: chronos }];
    let timestamp = chrono::Utc::now().timestamp();

    let body = TransactionBody {
        parents: &tips,
        timestamp,
        nonce,
        from: &kp.account_id,
        actions: &actions,
        auth_scheme: &AuthScheme::SingleSig,
    };
    let body_bytes = bincode::serialize(&body)
        .map_err(|e| format!("Serialising tx body: {e}"))?;

    // Mine PoW off the async executor.
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
    };

    let tx_bytes = bincode::serialize(&tx)
        .map_err(|e| format!("Serialising tx: {e}"))?;
    let tx_hex = hex::encode(&tx_bytes);

    let result = rpc_call("chronx_sendTransaction", serde_json::json!([tx_hex]))
        .await
        .map_err(|e| format!("Sending transaction: {e}"))?;

    result
        .as_str()
        .map(|s| s.to_string())
        .ok_or_else(|| "No TxId in response".to_string())
}
