"""Patch CLI to output total KX claimed, and fix relay to use it."""

# ---- 1. Fix CLI: add amount_kx to CascadeLock and output total ----
RPC_RS = '/home/josep/chronx/crates/chronx-wallet/src/rpc_client.rs'
with open(RPC_RS, 'r') as f:
    content = f.read()

# Add amount_kx field to CascadeLock
old_lock = '''pub struct CascadeLock {
    pub lock_id: String,
    pub amount_chronos: String,
    pub unlock_at: i64,
    pub status: String,
}'''
new_lock = '''pub struct CascadeLock {
    pub lock_id: String,
    pub amount_chronos: String,
    pub amount_kx: String,
    pub unlock_at: i64,
    pub status: String,
}'''
content = content.replace(old_lock, new_lock)

with open(RPC_RS, 'w') as f:
    f.write(content)
print('rpc_client.rs: Added amount_kx to CascadeLock')

# ---- 2. Fix CLI: output total KX claimed ----
MAIN_RS = '/home/josep/chronx/crates/chronx-wallet/src/main.rs'
with open(MAIN_RS, 'r') as f:
    content = f.read()

# Replace the claim-by-code handler to track total KX
old_handler = '''            let now = chrono::Utc::now().timestamp();
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

            println!("Claiming {} matured lock(s) (total in cascade: {})...", matured_count, details.lock_count);'''

new_handler = '''            let now = chrono::Utc::now().timestamp();
            let mut actions = Vec::new();
            let mut matured_count = 0u32;
            let mut total_kx_claimed: f64 = 0.0;

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
                    total_kx_claimed += lock.amount_kx.parse::<f64>().unwrap_or(0.0);
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

            println!("Claiming {} matured lock(s) totalling {} KX (total in cascade: {})...", matured_count, total_kx_claimed, details.lock_count);'''

if old_handler in content:
    content = content.replace(old_handler, new_handler)
    print('main.rs: Updated ClaimByCode to output total KX claimed')
else:
    print('ERROR: Could not find old handler in main.rs')

with open(MAIN_RS, 'w') as f:
    f.write(content)

# ---- 3. Fix Notify API: parse total from CLI output, use for transfer ----
NOTIFY_JS = '/opt/chronx-notify/index.js'
with open(NOTIFY_JS, 'r') as f:
    content = f.read()

old_relay = '''    // Brief pause to let the node process the claim tx
    await new Promise(resolve => setTimeout(resolve, 3000));

    // Step 2: Forward KX to the recipient's verified wallet
    console.log(`[RELAY] Forwarding ${amountKx} KX to ${recipientWallet}...`);
    const transferResult = execSync(
      `${CLI} --keyfile ${KEYFILE} transfer --to "${recipientWallet}" --amount ${amountKx}`,
      { timeout: 120000, encoding: 'utf8' }
    );'''

new_relay = '''    // Parse total KX claimed from CLI output (format: "Claiming N lock(s) totalling X KX")
    const totalMatch = claimResult.match(/totalling\\s+([\\d.]+)\\s+KX/);
    const claimedKx = totalMatch ? parseFloat(totalMatch[1]) : parseFloat(amountKx);
    console.log(`[RELAY] Claimed total: ${claimedKx} KX`);

    // Brief pause to let the node process the claim tx
    await new Promise(resolve => setTimeout(resolve, 3000));

    // Step 2: Forward ALL claimed KX to the recipient's verified wallet
    console.log(`[RELAY] Forwarding ${claimedKx} KX to ${recipientWallet}...`);
    const transferResult = execSync(
      `${CLI} --keyfile ${KEYFILE} transfer --to "${recipientWallet}" --amount ${claimedKx}`,
      { timeout: 120000, encoding: 'utf8' }
    );'''

if old_relay in content:
    content = content.replace(old_relay, new_relay)
    print('index.js: Updated relay to forward total claimed KX')
else:
    print('ERROR: Could not find old relay code in index.js')

# Also update the success log to show claimedKx instead of amountKx
old_log = '''          autoDeliverToVerifiedWallet(claim_code, recipientWallet, amount)
            .then(result => {
              if (result.success) {
                console.log(`[RELAY] Auto-delivered ${amount} KX to ${recipientWallet} (tx: ${result.tx_id})`);'''
new_log = '''          autoDeliverToVerifiedWallet(claim_code, recipientWallet, amount)
            .then(result => {
              if (result.success) {
                console.log(`[RELAY] Auto-delivered to ${recipientWallet} (tx: ${result.tx_id})`);'''

content = content.replace(old_log, new_log)

with open(NOTIFY_JS, 'w') as f:
    f.write(content)
print('Notify API patched.')
