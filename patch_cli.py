"""Patch chronx-wallet CLI to add claim-by-code subcommand."""
import re

# ---- 1. Patch main.rs: Add ClaimByCode variant + handler ----
main_rs = '/home/josep/chronx/crates/chronx-wallet/src/main.rs'
with open(main_rs, 'r') as f:
    content = f.read()

if 'ClaimByCode' in content:
    print('ClaimByCode already exists in main.rs, skipping')
else:
    # Add ClaimByCode variant after "Claim { lock_id }" in the enum
    old_claim = '    /// Claim a matured time-lock.\n    Claim {\n        /// Lock ID (TxId hex of the creating transaction).\n        #[arg(long)]\n        lock_id: String,\n    },'
    new_claim = old_claim + """

    /// Claim email lock(s) by claim code (KX-XXXX-...). Finds all matured
    /// locks matching this code and claims them in a single transaction.
    ClaimByCode {
        /// Claim code (e.g. KX-7F3A-9B2C-E1D4-5H6K).
        #[arg(long)]
        claim_code: String,
    },"""
    content = content.replace(old_claim, new_claim)

    # Add handler: insert ClaimByCode match arm after the Claim match arm
    # Find "Command::Claim { lock_id }" handler block and insert after it
    claim_handler_end = '            println!("Claim submitted: {}", tx_id);\n            Ok(())\n        }\n'
    claim_by_code_handler = claim_handler_end + """
        Command::ClaimByCode { claim_code } => {
            let kp = load_keypair(&keyfile)?;
            let code = claim_code.trim().to_uppercase();

            // BLAKE3(claim_code) -> claim_secret_hash
            let code_hash = blake3::hash(code.as_bytes());
            let hash_hex = hex::encode(code_hash.as_bytes());

            // Look up locks by claim_secret_hash
            let details = client.get_cascade_details(&hash_hex).await?;
            if details.locks.is_empty() {
                bail!("No locks found for claim code {}", code);
            }

            let now = chrono::Utc::now().timestamp();
            let mut actions = Vec::new();
            let mut matured_count = 0u32;

            for lock in &details.locks {
                // Only claim Pending locks that are matured
                if lock.status == "Pending" && lock.unlock_at <= now {
                    let lock_txid = TxId::from_hex(&lock.lock_id)
                        .map_err(|e| anyhow::anyhow!("invalid lock id: {e}"))?;
                    actions.push(Action::TimeLockClaimWithSecret {
                        lock_id: TimeLockId(lock_txid),
                        claim_secret: code.clone(),
                    });
                    matured_count += 1;
                }
            }

            if actions.is_empty() {
                let msg = format!(
                    "Found {} lock(s) but none are matured+pending. Pending: {}, Claimed: {}",
                    details.locks.len(), details.pending_count, details.claimed_count
                );
                println!("{}", msg);
                for lock in &details.locks {
                    println!("  {} - status={}, unlock_at={}", lock.lock_id, lock.status, lock.unlock_at);
                }
                return Ok(());
            }

            println!("Claiming {} matured lock(s) (total in cascade: {})...", matured_count, details.lock_count);
            let tx = build_and_sign(&kp, actions, &client).await?;
            let tx_id = client.send_transaction(&tx).await?;
            println!("Claim submitted: {}", tx_id);
            Ok(())
        }
"""

    # There are two occurrences of "Claim submitted" — one for Claim, one we're adding.
    # We need to find the FIRST one (the original Claim handler) and insert after it.
    # Let's be more specific: find the Claim handler by its surrounding context.
    old_block = '            println!("Claim submitted: {}", tx_id);\n            Ok(())\n        }\n\n        Command::Recover {'
    new_block = '            println!("Claim submitted: {}", tx_id);\n            Ok(())\n        }\n\n        Command::ClaimByCode { claim_code } => {\n            let kp = load_keypair(&keyfile)?;\n            let code = claim_code.trim().to_uppercase();\n\n            // BLAKE3(claim_code) -> claim_secret_hash\n            let code_hash = blake3::hash(code.as_bytes());\n            let hash_hex = hex::encode(code_hash.as_bytes());\n\n            // Look up locks by claim_secret_hash\n            let details = client.get_cascade_details(&hash_hex).await?;\n            if details.locks.is_empty() {\n                bail!("No locks found for claim code {}", code);\n            }\n\n            let now = chrono::Utc::now().timestamp();\n            let mut actions = Vec::new();\n            let mut matured_count = 0u32;\n\n            for lock in &details.locks {\n                // Only claim Pending locks that are matured\n                if lock.status == "Pending" && lock.unlock_at <= now {\n                    let lock_txid = TxId::from_hex(&lock.lock_id)\n                        .map_err(|e| anyhow::anyhow!("invalid lock id: {e}"))?;\n                    actions.push(Action::TimeLockClaimWithSecret {\n                        lock_id: TimeLockId(lock_txid),\n                        claim_secret: code.clone(),\n                    });\n                    matured_count += 1;\n                }\n            }\n\n            if actions.is_empty() {\n                let msg = format!(\n                    "Found {} lock(s) but none are matured+pending. Pending: {}, Claimed: {}",\n                    details.locks.len(), details.pending_count, details.claimed_count\n                );\n                println!("{}", msg);\n                for lock in &details.locks {\n                    println!("  {} - status={}, unlock_at={}", lock.lock_id, lock.status, lock.unlock_at);\n                }\n                return Ok(());\n            }\n\n            println!("Claiming {} matured lock(s) (total in cascade: {})...", matured_count, details.lock_count);\n            let tx = build_and_sign(&kp, actions, &client).await?;\n            let tx_id = client.send_transaction(&tx).await?;\n            println!("Claim submitted: {}", tx_id);\n            Ok(())\n        }\n\n        Command::Recover {'

    if old_block in content:
        content = content.replace(old_block, new_block, 1)
        print('Added ClaimByCode handler to main.rs')
    else:
        print('ERROR: Could not find insertion point for ClaimByCode handler')

    with open(main_rs, 'w') as f:
        f.write(content)
    print('main.rs patched')

# ---- 2. Patch rpc_client.rs: Add get_cascade_details method + types ----
rpc_rs = '/home/josep/chronx/crates/chronx-wallet/src/rpc_client.rs'
with open(rpc_rs, 'r') as f:
    rpc_content = f.read()

if 'get_cascade_details' in rpc_content:
    print('get_cascade_details already exists in rpc_client.rs, skipping')
else:
    # Add method before the final closing brace of impl WalletRpcClient
    cascade_method = """
    /// Get cascade details by claim_secret_hash (hex).
    pub async fn get_cascade_details(&self, claim_secret_hash: &str) -> anyhow::Result<CascadeDetails> {
        let result = self
            .call("chronx_getCascadeDetails", serde_json::json!([claim_secret_hash]))
            .await?;
        let details: CascadeDetails =
            serde_json::from_value(result).context("parsing cascade details")?;
        Ok(details)
    }
"""

    cascade_types = """

/// Minimal cascade details for the CLI (mirrors RpcCascadeDetails).
#[derive(Debug, serde::Deserialize)]
pub struct CascadeDetails {
    pub claim_secret_hash: String,
    pub lock_count: u32,
    pub total_chronos: String,
    pub total_kx: String,
    pub pending_count: u32,
    pub claimed_count: u32,
    pub locks: Vec<CascadeLock>,
}

#[derive(Debug, serde::Deserialize)]
pub struct CascadeLock {
    pub lock_id: String,
    pub amount_chronos: String,
    pub unlock_at: i64,
    pub status: String,
}
"""

    # Find last } in the impl block
    last_brace = rpc_content.rfind('}')
    rpc_content = rpc_content[:last_brace] + cascade_method + rpc_content[last_brace:] + cascade_types

    with open(rpc_rs, 'w') as f:
        f.write(rpc_content)
    print('rpc_client.rs patched')

print('All patches applied.')
