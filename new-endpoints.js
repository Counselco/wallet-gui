
// ── Request KX rate limiters (in-memory) ─────────────────────────────────────
const checkEmailRateLimits = new Map(); // ip -> { timestamps: [] }

// Clean up stale check-email rate limit entries every hour
setInterval(() => {
  const now = Date.now();
  for (const [ip, bucket] of checkEmailRateLimits) {
    bucket.timestamps = bucket.timestamps.filter(t => t > now - 60000);
    if (bucket.timestamps.length === 0) checkEmailRateLimits.delete(ip);
  }
}, 3600000);

// Check email registration status (enhanced with rate limiting)
app.get("/check-email-v2", async (req, res) => {
  // Rate limit: 10 req/min/IP
  const ip = req.headers["x-forwarded-for"] || (req.socket && req.socket.remoteAddress) || "unknown";
  const now = Date.now();
  const bucket = checkEmailRateLimits.get(ip);
  if (bucket) {
    bucket.timestamps = bucket.timestamps.filter(t => t > now - 60000);
    if (bucket.timestamps.length >= 10) {
      return res.status(429).json({ error: "Rate limit exceeded. Max 10 requests per minute." });
    }
    bucket.timestamps.push(now);
  } else {
    checkEmailRateLimits.set(ip, { timestamps: [now] });
  }

  try {
    const email = req.query.email;
    if (!email) return res.status(400).json({ error: "email required" });
    const pool = await getDb();
    const normalizedEmail = email.trim().toLowerCase();

    // Check verified_emails table
    const [veRows] = await pool.execute(
      "SELECT wallet_address FROM verified_emails WHERE email = ? LIMIT 1",
      [normalizedEmail]
    );
    if (veRows.length > 0) {
      return res.json({ registered: true });
    }

    // Check claim_registrations (registered status)
    const [crRows] = await pool.execute(
      "SELECT id FROM claim_registrations WHERE LOWER(email) = ? AND status = 'registered' LIMIT 1",
      [normalizedEmail]
    );
    if (crRows.length > 0) {
      return res.json({ registered: true });
    }

    // Check wallet_registry (confirmed)
    const [wrRows] = await pool.execute(
      "SELECT wallet_address FROM wallet_registry WHERE LOWER(email) = ? AND confirmed = 1 LIMIT 1",
      [normalizedEmail]
    );
    if (wrRows.length > 0) {
      return res.json({ registered: true });
    }

    res.json({ registered: false });
  } catch (err) {
    console.error("check-email error:", err);
    res.json({ registered: false });
  }
});


// ── Request KX endpoints (new poke system v2) ────────────────────────────────

// POST /request-kx — send a payment request
app.post("/request-kx", async (req, res) => {
  try {
    const { from_email, from_wallet, to_email, amount_kx, note, from_name } = req.body;
    if (!from_wallet || !to_email || !amount_kx) {
      return res.status(400).json({ error: "Missing required fields: from_wallet, to_email, amount_kx" });
    }
    if (parseFloat(amount_kx) <= 0) {
      return res.status(400).json({ error: "Amount must be positive" });
    }
    if (!to_email || !/^[^\s@]+@[^\s@]+\.[^\s@]+$/.test(to_email)) {
      return res.status(400).json({ error: "Invalid recipient email" });
    }

    const pool = await getDb();

    // Check blocked_senders — reject if the recipient has blocked this sender
    const recipientEmail = to_email.trim().toLowerCase();
    // Find all wallets associated with this email
    const [recipientWallets] = await pool.execute(
      "SELECT wallet_address FROM verified_emails WHERE email = ? UNION SELECT wallet_address FROM wallet_registry WHERE LOWER(email) = ? AND confirmed = 1",
      [recipientEmail, recipientEmail]
    );
    const recipientWalletIds = recipientWallets.map(r => r.wallet_address);
    if (recipientWalletIds.length > 0) {
      const placeholders = recipientWalletIds.map(() => "?").join(",");
      const [blockedRows] = await pool.execute(
        "SELECT id FROM blocked_senders WHERE blocker_wallet IN (" + placeholders + ") AND blocked_wallet = ? LIMIT 1",
        [...recipientWalletIds, from_wallet]
      );
      if (blockedRows.length > 0) {
        return res.status(403).json({ error: "You have been blocked by this recipient." });
      }
    }

    // Rate limit: max 10 requests per wallet per 24h (DB-based)
    const [dayCount] = await pool.execute(
      "SELECT COUNT(*) AS cnt FROM poke_requests WHERE from_wallet = ? AND created_at > DATE_SUB(NOW(), INTERVAL 24 HOUR)",
      [from_wallet]
    );
    if (dayCount[0].cnt >= 10) {
      return res.status(429).json({ error: "Rate limit exceeded. Max 10 requests per 24 hours." });
    }

    // Rate limit: max 3 requests per wallet per recipient per 7 days
    const [weekCount] = await pool.execute(
      "SELECT COUNT(*) AS cnt FROM poke_requests WHERE from_wallet = ? AND to_email = ? AND created_at > DATE_SUB(NOW(), INTERVAL 7 DAY)",
      [from_wallet, to_email]
    );
    if (weekCount[0].cnt >= 3) {
      return res.status(429).json({ error: "Rate limit exceeded. Max 3 requests to the same recipient per 7 days." });
    }

    const { v4: uuidv4 } = require("uuid");
    const requestId = uuidv4();
    const expiresAt = new Date(Date.now() + 72 * 3600 * 1000);

    await pool.execute(
      "INSERT INTO poke_requests (id, from_wallet, from_email, from_name, to_email, amount_kx, note, status, expires_at) VALUES (?, ?, ?, ?, ?, ?, ?, 'pending', ?)",
      [requestId, from_wallet, from_email || null, from_name || null, to_email, amount_kx, note || null, expiresAt]
    );

    // Send email notification
    if (process.env.RESEND_API_KEY) {
      try {
        const senderLabel = from_name || from_email || from_wallet.substring(0, 8) + "...";
        const noteHtml = note
          ? '<tr><td style="padding:0 40px 24px;"><div style="background:#111;border-left:3px solid #C9A84C;padding:14px 18px;border-radius:4px;"><p style="color:#888;font-size:11px;text-transform:uppercase;letter-spacing:1.5px;margin:0 0 6px;">Message</p><p style="color:#ddd;font-size:15px;font-style:italic;margin:0;">&ldquo;' + note.replace(/</g, "&lt;") + '&rdquo;</p></div></td></tr>'
          : "";
        const payLink = "https://chronx.io/poke.html?action=pay&request_id=" + requestId + "&amount=" + amount_kx + "&email=" + encodeURIComponent(from_email || "") + "&note=" + encodeURIComponent(note || "");
        const declineLink = "https://chronx.io/poke.html?action=decline&request_id=" + requestId;

        const html = '<!DOCTYPE html><html><head><meta charset="utf-8"></head>' +
          '<body style="margin:0;padding:0;background:#0d0d0d;font-family:Arial,sans-serif;">' +
          '<table width="100%" cellpadding="0" cellspacing="0" style="background:#0d0d0d;">' +
          '<tr><td align="center" style="padding:40px 20px;">' +
          '<table width="560" cellpadding="0" cellspacing="0" style="background:#1a1a1a;border-radius:12px;border:1px solid #C9A84C;">' +
          '<tr><td style="padding:32px 40px 0;">' +
          '<img src="https://chronx.io/img/chronx-logo.png" alt="ChronX" width="120" style="display:block;margin-bottom:24px;">' +
          '<h1 style="color:#C9A84C;font-size:22px;margin:0 0 8px;">Payment Request</h1>' +
          '<p style="color:#aaa;font-size:14px;margin:0 0 24px;"><strong>' + senderLabel + '</strong> is requesting KX from you.</p>' +
          '</td></tr>' +
          '<tr><td style="padding:0 40px;">' +
          '<div style="text-align:center;padding:24px 0;">' +
          '<p style="color:#C9A84C;font-size:42px;font-weight:bold;margin:0;">' + formatKx(amount_kx) + '</p>' +
          '</div>' +
          '</td></tr>' +
          noteHtml +
          '<tr><td style="padding:16px 40px 32px;text-align:center;">' +
          '<a href="' + payLink + '" style="display:inline-block;background:#C9A84C;color:#000;text-decoration:none;font-weight:bold;font-size:15px;padding:14px 32px;border-radius:6px;margin-right:12px;">PAY NOW</a>' +
          '&nbsp;&nbsp;' +
          '<a href="' + declineLink + '" style="display:inline-block;background:#dc2626;color:#fff;text-decoration:none;font-weight:bold;font-size:15px;padding:14px 32px;border-radius:6px;">Decline</a>' +
          '</td></tr>' +
          '<tr><td style="padding:0 40px 32px;border-top:1px solid #333;">' +
          '<p style="color:#666;font-size:12px;margin:24px 0 0;">This request expires in 72 hours. You can safely ignore it if you don\'t want to pay.</p>' +
          '<p style="color:#666;font-size:12px;margin:8px 0 0;">ChronX &mdash; The Future Payment Protocol &nbsp;|&nbsp; Zero fees. Always.</p>' +
          '</td></tr>' +
          '</table></td></tr></table></body></html>';

        await resend.emails.send({
          from: "ChronX <noreply@chronx.io>",
          to: [to_email],
          subject: senderLabel + " is requesting " + formatKx(amount_kx),
          html,
        });
      } catch (emailErr) {
        console.error("/request-kx email error:", emailErr);
      }
    }

    console.log("[REQUEST-KX] Created request " + requestId + " from " + from_wallet.substring(0, 8) + " to " + to_email);
    res.json({ request_id: requestId, status: "sent" });
  } catch (err) {
    console.error("/request-kx error:", err);
    res.status(500).json({ error: "Internal server error" });
  }
});

// GET /request-kx/pending?email=xxx — get pending requests for an email
app.get("/request-kx/pending", async (req, res) => {
  try {
    const email = req.query.email;
    if (!email) return res.status(400).json({ error: "email required" });

    const pool = await getDb();

    // Auto-expire old requests
    await pool.execute(
      "UPDATE poke_requests SET status = 'expired' WHERE to_email = ? AND status = 'pending' AND expires_at < NOW()",
      [email]
    );

    const [rows] = await pool.execute(
      "SELECT id AS request_id, from_email, from_name, from_wallet, amount_kx, note, created_at FROM poke_requests WHERE to_email = ? AND status = 'pending' ORDER BY created_at DESC",
      [email]
    );
    res.json(rows);
  } catch (err) {
    console.error("/request-kx/pending error:", err);
    res.status(500).json({ error: "Internal server error" });
  }
});

// POST /request-kx/decline — decline a request
app.post("/request-kx/decline", async (req, res) => {
  try {
    const { request_id } = req.body;
    if (!request_id) return res.status(400).json({ error: "request_id required" });

    const pool = await getDb();
    await pool.execute(
      "UPDATE poke_requests SET status = 'declined', responded_at = NOW() WHERE id = ? AND status = 'pending'",
      [request_id]
    );
    res.json({ status: "declined" });
  } catch (err) {
    console.error("/request-kx/decline error:", err);
    res.status(500).json({ error: "Internal server error" });
  }
});

// POST /request-kx/block — block a sender and decline all pending from them
app.post("/request-kx/block", async (req, res) => {
  try {
    const { from_wallet, blocked_by_wallet, blocked_email } = req.body;
    if (!from_wallet || !blocked_by_wallet) {
      return res.status(400).json({ error: "from_wallet and blocked_by_wallet required" });
    }

    const pool = await getDb();

    // Insert into blocked_senders (ignore duplicates)
    await pool.execute(
      "INSERT IGNORE INTO blocked_senders (blocker_wallet, blocked_wallet, blocked_email) VALUES (?, ?, ?)",
      [blocked_by_wallet, from_wallet, blocked_email || null]
    );

    // Decline all pending requests from the blocked sender to the blocker
    // Find the blocker's emails from verified_emails and wallet_registry
    const [blockerEmails] = await pool.execute(
      "SELECT email FROM verified_emails WHERE wallet_address = ? UNION SELECT email FROM wallet_registry WHERE wallet_address = ? AND confirmed = 1",
      [blocked_by_wallet, blocked_by_wallet]
    );
    if (blockerEmails.length > 0) {
      const emails = blockerEmails.map(r => r.email);
      const placeholders = emails.map(() => "?").join(",");
      await pool.execute(
        "UPDATE poke_requests SET status = 'blocked', responded_at = NOW() WHERE from_wallet = ? AND to_email IN (" + placeholders + ") AND status = 'pending'",
        [from_wallet, ...emails]
      );
    }

    console.log("[REQUEST-KX] Blocked sender " + from_wallet.substring(0, 8) + " by " + blocked_by_wallet.substring(0, 8));
    res.json({ status: "blocked" });
  } catch (err) {
    console.error("/request-kx/block error:", err);
    res.status(500).json({ error: "Internal server error" });
  }
});

// GET /request-kx/blocked?wallet=xxx — get blocked senders for a wallet
app.get("/request-kx/blocked", async (req, res) => {
  try {
    const wallet = req.query.wallet;
    if (!wallet) return res.status(400).json({ error: "wallet required" });

    const pool = await getDb();
    const [rows] = await pool.execute(
      "SELECT blocked_wallet, blocked_email, created_at FROM blocked_senders WHERE blocker_wallet = ? ORDER BY created_at DESC",
      [wallet]
    );
    res.json(rows);
  } catch (err) {
    console.error("/request-kx/blocked error:", err);
    res.status(500).json({ error: "Internal server error" });
  }
});

// POST /request-kx/unblock — unblock a sender
app.post("/request-kx/unblock", async (req, res) => {
  try {
    const { blocker_wallet, blocked_wallet } = req.body;
    if (!blocker_wallet || !blocked_wallet) {
      return res.status(400).json({ error: "blocker_wallet and blocked_wallet required" });
    }

    const pool = await getDb();
    await pool.execute(
      "DELETE FROM blocked_senders WHERE blocker_wallet = ? AND blocked_wallet = ?",
      [blocker_wallet, blocked_wallet]
    );

    console.log("[REQUEST-KX] Unblocked sender " + blocked_wallet.substring(0, 8) + " by " + blocker_wallet.substring(0, 8));
    res.json({ status: "unblocked" });
  } catch (err) {
    console.error("/request-kx/unblock error:", err);
    res.status(500).json({ error: "Internal server error" });
  }
});
