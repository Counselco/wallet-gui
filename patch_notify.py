"""Patch Notify API to add relay auto-delivery for verified recipients."""

NOTIFY_JS = '/opt/chronx-notify/index.js'

with open(NOTIFY_JS, 'r') as f:
    content = f.read()

# ---- 1. Add getVerifiedWalletAddress + autoDeliverToVerifiedWallet after isEmailVerified ----

new_functions = '''
// Return the wallet address for a verified email, or null.
async function getVerifiedWalletAddress(email) {
  try {
    const pool = await getDb();
    const normalizedEmail = email.trim().toLowerCase();
    // Check verified_emails table first
    const [rows] = await pool.execute(
      'SELECT wallet_address FROM verified_emails WHERE email = ? LIMIT 1',
      [normalizedEmail]
    );
    if (rows.length > 0) return rows[0].wallet_address;
    // Fallback: wallet_registry (rewards confirmed)
    const [regRows] = await pool.execute(
      'SELECT wallet_address FROM wallet_registry WHERE LOWER(email) = ? AND confirmed = 1 LIMIT 1',
      [normalizedEmail]
    );
    if (regRows.length > 0) return regRows[0].wallet_address;
    return null;
  } catch (err) {
    console.error('getVerifiedWalletAddress error:', err);
    return null;
  }
}

// Relay auto-delivery: claim email lock by code, then forward KX to verified recipient.
// Returns { success, tx_id, error }
async function autoDeliverToVerifiedWallet(claimCode, recipientWallet, amountKx) {
  const CLI = process.env.WALLET_CLI;
  const KEYFILE = process.env.RELAY_KEYFILE;
  if (!CLI || !KEYFILE) {
    console.error('[RELAY] WALLET_CLI or RELAY_KEYFILE not configured');
    return { success: false, error: 'Relay not configured' };
  }
  try {
    // Step 1: Claim the email lock(s) using claim-by-code
    console.log(`[RELAY] Claiming locks for code ${claimCode.substring(0, 7)}...`);
    const claimResult = execSync(
      `${CLI} --keyfile ${KEYFILE} claim-by-code --claim-code "${claimCode}"`,
      { timeout: 120000, encoding: 'utf8' }
    );
    console.log(`[RELAY] Claim result: ${claimResult.trim()}`);

    // Brief pause to let the node process the claim tx
    await new Promise(resolve => setTimeout(resolve, 3000));

    // Step 2: Forward KX to the recipient's verified wallet
    console.log(`[RELAY] Forwarding ${amountKx} KX to ${recipientWallet}...`);
    const transferResult = execSync(
      `${CLI} --keyfile ${KEYFILE} transfer --to "${recipientWallet}" --amount ${amountKx}`,
      { timeout: 120000, encoding: 'utf8' }
    );
    console.log(`[RELAY] Transfer result: ${transferResult.trim()}`);

    // Extract tx_id from "Submitted: <hex>"
    const match = transferResult.match(/Submitted:\\s+([0-9a-f]+)/);
    const txId = match ? match[1] : 'unknown';

    return { success: true, tx_id: txId };
  } catch (err) {
    console.error(`[RELAY] Auto-delivery failed:`, err.message || err);
    return { success: false, error: err.message || String(err) };
  }
}

'''

# Insert after the isEmailVerified function (before "// ── Endpoints")
marker = '// ── Endpoints ───────────────────────────────────────────────────────────────'
if 'autoDeliverToVerifiedWallet' not in content:
    content = content.replace(marker, new_functions + marker)
    print('Added getVerifiedWalletAddress + autoDeliverToVerifiedWallet functions')
else:
    print('autoDeliverToVerifiedWallet already exists, skipping')

# ---- 2. Update the verified && !isFuture branch in /notify to trigger relay delivery ----

old_verified_branch = """      if (verified && !isFuture) {
        // Verified recipient + immediately claimable \u2014 send claim code email
        // (Node does NOT auto-deliver email locks; recipient must claim with code)
        html = buildEmail(amount, unlock_at, memo, claim_code);
        subject = `You've received ${amount} KX on ChronX`;
        console.log(`[NOTIFY] Verified recipient ${to} \u2014 sending claim code (no auto-delivery)`);"""

new_verified_branch = """      if (verified && !isFuture && claim_code) {
        // Verified recipient + immediately claimable \u2014 relay auto-delivery
        const recipientWallet = await getVerifiedWalletAddress(to);
        if (recipientWallet) {
          // Fire-and-forget: relay claims lock and forwards KX in background
          autoDeliverToVerifiedWallet(claim_code, recipientWallet, amount)
            .then(result => {
              if (result.success) {
                console.log(`[RELAY] Auto-delivered ${amount} KX to ${recipientWallet} (tx: ${result.tx_id})`);
              } else {
                console.error(`[RELAY] Auto-delivery failed for ${to}: ${result.error}`);
              }
            });
          // Send "auto-added to your wallet" email
          html = buildVerifiedRecipientEmail(amount, req.body.sender_email || null, memo);
          subject = `${amount} KX added to your ChronX wallet`;
          console.log(`[NOTIFY] Verified recipient ${to} \u2014 relay auto-delivery triggered`);
        } else {
          // Verified but no wallet address found \u2014 fallback to claim code email
          html = buildEmail(amount, unlock_at, memo, claim_code);
          subject = `You've received ${amount} KX on ChronX`;
          console.log(`[NOTIFY] Verified ${to} but no wallet address \u2014 sending claim code`);
        }"""

if 'relay auto-delivery' not in content:
    content = content.replace(old_verified_branch, new_verified_branch)
    print('Updated verified+immediate branch for relay auto-delivery')
else:
    print('Relay auto-delivery branch already exists, skipping')

with open(NOTIFY_JS, 'w') as f:
    f.write(content)

print('Notify API patched successfully.')
