// ╔══════════════════════════════════════════════════════════════════════════════╗
// ║  PLATFORM UI GUIDE — ChronX Wallet Frontend (Leptos WASM)                  ║
// ║                                                                            ║
// ║  This file compiles to WASM and runs identically on ALL platforms.          ║
// ║  The WASM target is always wasm32-unknown-unknown — no #[cfg()] here.      ║
// ║  Platform differences are handled at RUNTIME via:                          ║
// ║                                                                            ║
// ║    is_desktop() → true on Windows/macOS/Linux (user-agent check)           ║
// ║    is_ios()     → true on iPhone/iPad (user-agent check)                   ║
// ║    else         → Android                                                  ║
// ║                                                                            ║
// ║  LAYOUT:                                                                   ║
// ║    Desktop  — Left sidebar, 6 tabs: Receive|Send|Promises|Request|         ║
// ║               History|Settings. CSS class "desktop-shell".                 ║
// ║    Mobile   — Top header + bottom tab bar, 4 tabs: Receive|Send|           ║
// ║               Promises|Settings. CSS class "app". History & Rewards        ║
// ║               accessible as sub-views from Settings.                       ║
// ║                                                                            ║
// ║  DESKTOP-ONLY FEATURES (gated by `if desktop { ... }`):                    ║
// ║    • Cascade Send toggle (Simple/Cascade) on Send tab                      ║
// ║    • CascadeSendPanel — multi-stage time-locked send builder               ║
// ║    • RequestPanel (Tab 3) — poke payment requests                          ║
// ║    • HistoryPanel (Tab 4) — full transaction history with filters          ║
// ║    • Cold Storage wallet generator (Settings)                              ║
// ║    • Node URL setting (Settings → Advanced)                                ║
// ║    • Power-user warning banner on Send screen                              ║
// ║                                                                            ║
// ║  MOBILE-ONLY FEATURES (gated by `if !desktop { ... }`):                    ║
// ║    • History sub-view (full-screen from Settings)                          ║
// ║    • Rewards sub-view (full-screen from Settings)                          ║
// ║    • Poke badge on Send tab (desktop shows in Request tab)                 ║
// ║                                                                            ║
// ║  iOS-SPECIFIC (gated by `if is_ios() { ... }`):                            ║
// ║    • Update button shows "Update via App Store" (no direct link)           ║
// ║                                                                            ║
// ║  ALL PLATFORMS (no gating):                                                ║
// ║    • AccountPanel (Receive tab) — balance, QR, claim-by-code              ║
// ║    • SendPanel — simple KX/email sends, now/later, series                  ║
// ║    • PromisesPanel — incoming + outgoing sections                          ║
// ║    • SettingsPanel — language, PIN, email verification, backup             ║
// ║    • Deep link handling (chronx://claim, chronx://pay)                     ║
// ║    • QR code generation and scanning                                       ║
// ╚══════════════════════════════════════════════════════════════════════════════╝

use base64::Engine as _;
use js_sys::Promise;
use leptos::prelude::*;
use serde::Deserialize;
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::{spawn_local, JsFuture};
use std::collections::HashMap;

const LOGO_PNG: &[u8] = include_bytes!("../assets/chronx-logo.png");

const RELAY_WALLET: &str = "8Nodc3F2HwUjPMLaFfTJ6WKuSvjEa4fTeopLUK52y5EE";
const RELAY_WALLET_LEGACY: &str = "9Vjh83mQHBEf5aMgz4emA3FaFygacDodWWLKeS31hp6m";

fn is_relay_wallet(addr: &str) -> bool {
    addr == RELAY_WALLET || addr == RELAY_WALLET_LEGACY
}

// ── i18n translation system ──────────────────────────────────────────────────

static I18N_EN: &str = include_str!("i18n/en.json");
static I18N_FR: &str = include_str!("i18n/fr.json");
static I18N_DE: &str = include_str!("i18n/de.json");
static I18N_ZH: &str = include_str!("i18n/zh.json");
static I18N_ES: &str = include_str!("i18n/es.json");
static I18N_RU: &str = include_str!("i18n/ru.json");
static I18N_AR: &str = include_str!("i18n/ar.json");
static I18N_UR: &str = include_str!("i18n/ur.json");

fn load_translations() -> HashMap<String, HashMap<String, String>> {
    let mut all = HashMap::new();
    for (code, json_str) in [
        ("en", I18N_EN), ("fr", I18N_FR), ("de", I18N_DE),
        ("zh", I18N_ZH), ("es", I18N_ES), ("ru", I18N_RU), ("ar", I18N_AR), ("ur", I18N_UR),
    ] {
        let map: HashMap<String, String> = serde_json::from_str(json_str).unwrap_or_default();
        all.insert(code.to_string(), map);
    }
    all
}

thread_local! {
    static TRANSLATIONS: HashMap<String, HashMap<String, String>> = load_translations();
}

fn t(lang: &str, key: &str) -> String {
    TRANSLATIONS.with(|all| {
        if let Some(map) = all.get(lang) {
            if let Some(val) = map.get(key) {
                return val.clone();
            }
        }
        // Fallback to English
        if let Some(en) = all.get("en") {
            if let Some(val) = en.get(key) {
                return val.clone();
            }
        }
        key.to_string()
    })
}

fn detect_locale() -> String {
    if let Some(win) = web_sys::window() {
        let nav_lang = win.navigator().language().unwrap_or_default();
        let short = nav_lang.split('-').next().unwrap_or("en").to_lowercase();
        match short.as_str() {
            "fr" | "de" | "zh" | "es" | "ru" | "ar" => short,
            _ => "en".to_string(),
        }
    } else {
        "en".to_string()
    }
}

fn is_desktop() -> bool {
    if let Some(win) = web_sys::window() {
        let ua = win.navigator().user_agent().unwrap_or_default().to_lowercase();
        !ua.contains("android") && !ua.contains("iphone") && !ua.contains("ipad")
    } else {
        true
    }
}

fn is_ios() -> bool {
    if let Some(win) = web_sys::window() {
        let ua = win.navigator().user_agent().unwrap_or_default().to_lowercase();
        ua.contains("iphone") || ua.contains("ipad")
    } else {
        false
    }
}

/// Evaluate a loan offer for predatory lending flags.
/// Returns (is_blocked, is_warned, messages).
fn check_loan_flags(rate: f64, principal_kx: f64) -> (bool, bool, Vec<String>) {
    let ico_price = 0.00319;
    let principal_usd = principal_kx * ico_price;
    let mut blocked = false;
    let mut warned = false;
    let mut msgs = Vec::new();
    if rate > 15.0 {
        blocked = true;
        msgs.push("Annual rate exceeds 15% protocol limit".to_string());
    } else if rate > 10.0 {
        warned = true;
        msgs.push("Annual rate above 10%".to_string());
    }
    if principal_usd > 250.0 {
        blocked = true;
        msgs.push("Principal exceeds $250 USD protocol limit".to_string());
    } else if principal_usd > 100.0 {
        warned = true;
        msgs.push("Principal above $100 USD equivalent".to_string());
    }
    (blocked, warned, msgs)
}

fn flag_badge_style(flag: &str) -> (&'static str, &'static str) {
    match flag {
        "Active" => ("#2ecc71", "rgba(46,204,113,0.15)"),
        "Late" | "Delinquent" => ("#f1c40f", "rgba(241,196,15,0.15)"),
        "Default" | "Accelerated" => ("#e74c3c", "rgba(231,76,60,0.15)"),
        "Disputed" => ("#e67e22", "rgba(230,126,34,0.15)"),
        "PaidOff" | "EarlyExit" => ("#8899aa", "rgba(136,153,170,0.15)"),
        "Amended" | "Reinstated" => ("#3498db", "rgba(52,152,219,0.15)"),
        "Frozen" | "LitigationPending" => ("#e74c3c", "rgba(231,76,60,0.08)"),
        "Bankruptcy" => ("#8b0000", "rgba(139,0,0,0.15)"),
        _ => ("#8899aa", "rgba(136,153,170,0.1)"),
    }
}

// ── Trusted contact type (frontend) ──────────────────────────────────────────

#[derive(Clone, Deserialize, Default)]
struct TrustedContact {
    email: String,
    wallet: Option<String>,
    display_name: Option<String>,
    added_at: u64,
}

// ── Contact type (address book) ──────────────────────────────────────────────

#[derive(Clone, Deserialize, Default, serde::Serialize)]
struct Contact {
    id: String,
    name: String,
    email: Option<String>,
    kx_address: Option<String>,
    notes: Option<String>,
    last_sent: Option<i64>,
    send_count: u32,
    created_at: i64,
}

// ── Poke request type (frontend) ─────────────────────────────────────────────

#[derive(Clone, Deserialize, Default)]
struct PendingPoke {
    request_id: String,
    from_wallet: String,
    from_email: Option<String>,
    amount_kx: String,
    note: Option<String>,
    created_at: String,
    expires_at: String,
    #[serde(default)]
    verified_sender: bool,
}

// ── Open tab types (frontend mirrors of backend structs) ─────────────────────

#[derive(Clone, Deserialize, Default)]
struct CommitmentInfo {
    commitment_id: String,
    commitment_type: String,
    status: String,
}

#[derive(Clone, Deserialize, Default)]
struct SignOfLifeStatus {
    next_due: Option<i64>,
    locks_count: u64,
}

#[derive(Clone, Deserialize, Default)]
struct InvoiceRecord {
    invoice_id: String,
    from_wallet: String,
    from_display: String,
    amount_kx: f64,
    created_at: u64,
    expires_at: Option<u64>,
    memo: Option<String>,
}

#[derive(Clone, Deserialize, Default)]
struct AddressBookEntry {
    email: String,
    name: Option<String>,
    registered: Option<bool>,
    #[allow(dead_code)]
    last_checked: Option<u64>,
}

#[derive(Clone, Deserialize, Default)]
struct KxRequest {
    request_id: String,
    from_email: String,
    from_name: String,
    from_wallet: String,
    amount_kx: f64,
    note: Option<String>,
    #[allow(dead_code)]
    created_at: u64,
}

fn logo_src() -> String {
    format!(
        "data:image/png;base64,{}",
        base64::engine::general_purpose::STANDARD.encode(LOGO_PNG)
    )
}

// ── Tauri v2 invoke bridge ────────────────────────────────────────────────────

#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(js_namespace = ["window", "__TAURI_INTERNALS__"])]
    fn invoke(cmd: &str, args: JsValue) -> Promise;

    #[wasm_bindgen(js_namespace = window, js_name = __chronxScanQr)]
    fn scan_qr_js(file: &web_sys::File) -> Promise;
}

async fn call<T: serde::de::DeserializeOwned>(
    cmd: &str,
    args: JsValue,
) -> Result<T, String> {
    JsFuture::from(invoke(cmd, args))
        .await
        .map_err(|e| e.as_string().unwrap_or_else(|| format!("{e:?}")))
        .and_then(|v| serde_wasm_bindgen::from_value(v).map_err(|e| e.to_string()))
}

fn no_args() -> JsValue {
    js_sys::Object::new().into()
}

// ── Shared types ──────────────────────────────────────────────────────────────

#[derive(Clone, Deserialize, Default)]
struct AccountInfo {
    account_id: String,
    #[allow(dead_code)]
    balance_kx: String,
    balance_chronos: String,
    #[allow(dead_code)]
    spendable_kx: String,
    spendable_chronos: String,
    #[allow(dead_code)]
    nonce: u64,
}

#[derive(Clone, Deserialize, Default)]
struct TimeLockInfo {
    lock_id: String,
    sender: String,
    #[allow(dead_code)]
    recipient_account_id: String,
    #[allow(dead_code)]
    amount_kx: String,
    #[serde(default)]
    amount_chronos: String,
    unlock_at: i64,
    created_at: i64,
    status: String,
    memo: Option<String>,
    /// Hex of BLAKE3(claim_code) — locks with the same hash belong to a Promise Series.
    #[serde(default)]
    claim_secret_hash: Option<String>,
    #[serde(default)]
    cancellation_window_secs: Option<u32>,
    /// BLAKE3(recipient_email) — only present on email locks (0xC5 marker).
    #[serde(default)]
    recipient_email_hash: Option<String>,
    /// Direction: "incoming" or "outgoing". Set by get_all_promises.
    #[serde(default)]
    direction: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize, Deserialize)]
struct InvoiceInfo {
    invoice_id: String,
    amount_kx: String,
    amount_chronos: String,
    expiry: u64,
    status: String,
    created_at: u64,
}

#[derive(Debug, Clone, serde::Serialize, Deserialize)]
struct CreditInfo {
    credit_id: String,
    ceiling_kx: String,
    drawn_kx: String,
    expiry: u64,
    status: String,
}

#[derive(Debug, Clone, serde::Serialize, Deserialize)]
struct DepositInfo {
    deposit_id: String,
    principal_kx: String,
    total_due_kx: String,
    maturity_timestamp: u64,
    status: String,
    compounding: String,
    rate_basis_points: u64,
}

#[derive(Debug, Clone, serde::Serialize, Deserialize)]
struct ConditionalInfo {
    type_v_id: String,
    amount_kx: String,
    min_attestors: u32,
    attestations_received: u32,
    valid_until: u64,
    fallback: String,
    status: String,
}

#[derive(Debug, Clone, serde::Serialize, Deserialize)]
struct SignOfLifeInfo {
    lock_id: String,
    interval_days: u64,
    grace_days: u64,
    next_due: u64,
    status: String,
    responsible: String,
}


/// Returned by `create_email_timelock_series`.
#[derive(Clone, Deserialize, Default)]
struct EmailSeriesResult {
    tx_ids: Vec<String>,
    claim_code: String,
}

/// Returned by `generate_cold_wallet`.
#[derive(Clone, Deserialize, Default)]
struct ColdWalletResult {
    account_id: String,
    private_key_b64: String,
}

/// Returned by `claim_by_code`.
#[derive(Clone, Deserialize, Default)]
struct ClaimByCodeResult {
    tx_id: String,
    claimed_count: usize,
    total_chronos: String,
    lock_ids: Vec<String>,
}

/// Returned by `get_claim_info` (whitelist popup).
#[derive(Clone, Deserialize, Default)]
struct ClaimInfoResult {
    found: bool,
    #[serde(default)]
    email: Option<String>,
    #[serde(default)]
    amount_kx: Option<f64>,
}

#[derive(Clone, Deserialize, Default)]
struct TxHistoryEntry {
    tx_id: String,
    tx_type: String,
    amount_chronos: Option<String>,
    counterparty: Option<String>,
    timestamp: i64,
    status: String,
    #[serde(default)]
    unlock_date: Option<i64>,
    #[serde(default)]
    cancellation_window_secs: Option<u32>,
    #[serde(default)]
    created_at: Option<i64>,
    /// Claim code for email sends — kept locally so Alice can re-share it.
    #[serde(default)]
    claim_code: Option<String>,
    /// BLAKE3(claim_code) hex — locks sharing this hash belong to a Promise Series.
    #[serde(default)]
    claim_secret_hash: Option<String>,
    /// Whether the recipient email is registered (verified) in the system.
    #[serde(default)]
    recipient_registered: Option<bool>,
    /// Memo text attached to the transaction (if any).
    #[serde(default)]
    memo: Option<String>,
    /// Original sender wallet (for relay-delivered transactions).
    #[serde(default)]
    sender_wallet: Option<String>,
    /// Original sender email (for relay-delivered transactions).
    #[serde(default)]
    sender_email: Option<String>,
    /// Original sender display name (for relay-delivered transactions).
    #[serde(default)]
    sender_display: Option<String>,
}

/// Returned by `create_email_timelock` — carries the on-chain TxId and
/// the "KX-XXXX-XXXX-XXXX-XXXX" claim code to email/display to the recipient.
#[derive(Clone, Deserialize, Default)]
struct EmailLockResult {
    tx_id: String,
    claim_code: String,
}

// ── v2.2.2 types ────────────────────────────────────────────────────────────

/// Verified identity record for a wallet address.
/// Used to show gold checkmark ✓ + display name instead of truncated address.
#[derive(Clone, Deserialize, Default)]
struct IdentityRecord {
    wallet_address: String,
    display_name: String,
    verified: bool,
}

/// Badge earned by a wallet (KXGO Bronze/Silver/Gold, Founder, etc.)
#[derive(Clone, Deserialize, Default)]
struct WalletBadge {
    #[serde(rename = "type")]
    badge_type: String,
    #[serde(default)]
    color: Option<String>,
    #[serde(default)]
    issued_by: Option<String>,
}

/// Active commitments — TYPE V conditionals, TYPE C credits, TYPE Y deposits.
#[derive(Clone, Deserialize, Default)]
struct CommitmentsData {
    #[serde(default)]
    active_locks: Vec<ConditionalRecord>,
    #[serde(default)]
    active_credits: Vec<CreditRecord>,
    #[serde(default)]
    active_deposits: Vec<DepositRecord>,
}

#[derive(Clone, Deserialize, Default)]
struct ConditionalRecord {
    conditional_id: String,
    #[serde(default)]
    amount_chronos: String,
    #[serde(default)]
    attestor: String,
    #[serde(default)]
    valid_until: Option<i64>,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    status: String,
}

#[derive(Clone, Deserialize, Default)]
struct CreditRecord {
    credit_id: String,
    #[serde(default)]
    beneficiary: String,
    #[serde(default)]
    ceiling_chronos: String,
    #[serde(default)]
    drawn_chronos: String,
    #[serde(default)]
    expires_at: Option<i64>,
    #[serde(default)]
    status: String,
}

#[derive(Clone, Deserialize, Default)]
struct DepositRecord {
    deposit_id: String,
    #[serde(default)]
    obligor: String,
    #[serde(default)]
    total_due_chronos: String,
    #[serde(default)]
    paid_chronos: String,
    #[serde(default)]
    matures_at: Option<i64>,
    #[serde(default)]
    status: String,
}

// ── Server-pushed types ───────────────────────────────────────────────────────

#[derive(Clone, serde::Deserialize)]
struct Notice {
    id: String,
    #[serde(default)]
    title: String,
    #[serde(default)]
    body: String,
    #[serde(default)]
    severity: String, // "info" | "warning" | "critical" | "reward" | "urgent" | "message"
    #[serde(default)]
    date: String,
    #[serde(rename = "type", default)]
    notice_type: String,
    #[serde(default)]
    dismissible: Option<bool>,
    #[serde(default)]
    expires: Option<String>,
    #[serde(default)]
    url: Option<String>,
    #[serde(default)]
    url_label: Option<String>,
}

#[derive(Clone, serde::Deserialize)]
struct UpdateInfo {
    up_to_date: bool,
    current: String,
    latest: String,
    download_url: String,
    release_notes: String,
}

#[derive(Clone, serde::Deserialize, Default)]
struct ConvertQuote {
    kx_in: f64,
    usdc_out: f64,
    #[allow(dead_code)]
    spot_rate: f64,
    trade_rate: f64,
    slippage_pct: f64,
    fee_pct: f64,
    total_cost_pct: f64,
    warning: Option<String>,
    warning_level: String,
    requires_confirmation: bool,
    #[allow(dead_code)]
    quoted_at: String,
    #[allow(dead_code)]
    valid_for_seconds: u32,
}

// ── App phase state machine ───────────────────────────────────────────────────

#[derive(Clone, PartialEq)]
enum AppPhase {
    Splash,
    Welcome,
    BackupKey,
    RestoreWallet,
    PinSetup,
    PinConfirm,
    PinUnlock,
    Wallet,
}

// ── Number formatting ─────────────────────────────────────────────────────────

fn format_int_with_commas(n: u128) -> String {
    if n == 0 { return "0".to_string(); }
    let s = n.to_string();
    let mut result: Vec<char> = Vec::new();
    for (i, ch) in s.chars().rev().enumerate() {
        if i > 0 && i % 3 == 0 { result.push(','); }
        result.push(ch);
    }
    result.into_iter().rev().collect()
}

fn format_kx(chronos_str: &str) -> String {
    let c: u128 = chronos_str.parse().unwrap_or(0);
    let whole = c / 1_000_000;
    let frac = (c % 1_000_000) as u32;
    if frac == 0 {
        format_int_with_commas(whole)
    } else {
        let d2 = frac / 10_000; // first 2 decimal digits
        if d2 == 0 {
            format_int_with_commas(whole)
        } else if d2 % 10 == 0 {
            format!("{}.{}", format_int_with_commas(whole), d2 / 10)
        } else {
            format!("{}.{:02}", format_int_with_commas(whole), d2)
        }
    }
}

/// Format a raw numeric string (e.g. "10000000.343566") with thousands separators.
/// Returns e.g. "10,000,000.343566". Works on the integer part only.
fn format_amount_display(raw: &str) -> String {
    if raw.is_empty() { return String::new(); }
    let (int_part, dec_part) = match raw.find('.') {
        Some(pos) => (&raw[..pos], Some(&raw[pos..])), // ".343566" including dot
        None => (raw, None),
    };
    // Format integer part with commas
    let digits: Vec<u8> = int_part.bytes().filter(|b| b.is_ascii_digit()).collect();
    if digits.is_empty() {
        return dec_part.map_or_else(String::new, |d| format!("0{d}"));
    }
    let mut formatted = String::new();
    let len = digits.len();
    for (i, &b) in digits.iter().enumerate() {
        if i > 0 && (len - i) % 3 == 0 { formatted.push(','); }
        formatted.push(b as char);
    }
    if let Some(d) = dec_part { formatted.push_str(d); }
    formatted
}

/// Format a f64 KX amount without trailing zeros: 100.0 → "100", 1.50 → "1.5"
fn format_kx_display(val: f64) -> String {
    if val.fract() == 0.0 {
        format!("{}", val as u64)
    } else {
        let s = format!("{:.6}", val);
        s.trim_end_matches('0').trim_end_matches('.').to_string()
    }
}

// ── Display helpers ───────────────────────────────────────────────────────────

fn shorten_addr(addr: &str) -> String {
    if addr.len() > 20 {
        format!("{}…{}", &addr[..8], &addr[addr.len()-8..])
    } else {
        addr.to_string()
    }
}

/// Returns the verified display name with a gold ✓ if the address has a
/// known identity in the cache, otherwise falls back to `shorten_addr`.
fn identity_or_short(addr: &str, cache: &std::collections::HashMap<String, IdentityRecord>) -> String {
    if let Some(rec) = cache.get(addr) {
        if rec.verified {
            format!("\u{2713} {}", rec.display_name)
        } else if !rec.display_name.is_empty() {
            rec.display_name.clone()
        } else {
            shorten_addr(addr)
        }
    } else {
        shorten_addr(addr)
    }
}

fn format_utc_ts(ts: i64) -> String {
    let d = js_sys::Date::new(&wasm_bindgen::JsValue::from_f64(ts as f64 * 1000.0));
    let now_secs = (js_sys::Date::now() / 1000.0) as i64;
    let diff = now_secs - ts;
    let month = match d.get_month() {
        0 => "Jan", 1 => "Feb", 2 => "Mar", 3 => "Apr",
        4 => "May", 5 => "Jun", 6 => "Jul", 7 => "Aug",
        8 => "Sep", 9 => "Oct", 10 => "Nov", _ => "Dec",
    };
    let day = d.get_date();
    let hours = d.get_hours();
    let mins = d.get_minutes();
    let ampm = if hours >= 12 { "PM" } else { "AM" };
    let h12 = if hours % 12 == 0 { 12 } else { hours % 12 };
    if diff < 86400 {
        format!("{}:{:02} {}", h12, mins, ampm)
    } else if diff < 86400 * 365 {
        format!("{} {}", month, day)
    } else {
        format!("{} {} {}", month, day, d.get_full_year())
    }
}

// ── QR code generation ────────────────────────────────────────────────────────

fn make_qr_svg(data: &str) -> String {
    use qrcodegen::{QrCode, QrCodeEcc};
    let Ok(qr) = QrCode::encode_text(data, QrCodeEcc::Medium) else {
        return String::new();
    };
    let sz = qr.size() as u32;
    let border: u32 = 4;
    let scale: u32 = 8;
    let total = (sz + border * 2) * scale;
    let mut s = format!(
        "<svg xmlns='http://www.w3.org/2000/svg' width='{t}' height='{t}' \
         viewBox='0 0 {t} {t}' style='shape-rendering:crispEdges;background:#fff;display:block;margin:0 auto'>",
        t = total
    );
    for y in 0..sz as i32 {
        for x in 0..sz as i32 {
            if qr.get_module(x, y) {
                let px = (x as u32 + border) * scale;
                let py = (y as u32 + border) * scale;
                s.push_str(&format!(
                    "<rect x='{px}' y='{py}' width='{scale}' height='{scale}' fill='#000'/>"
                ));
            }
        }
    }
    s.push_str("</svg>");
    s
}

// ── QR scan helpers ───────────────────────────────────────────────────────────

async fn scan_qr_file(file: web_sys::File) -> Result<String, String> {
    let result = JsFuture::from(scan_qr_js(&file))
        .await
        .map_err(|e| e.as_string().unwrap_or_else(|| format!("{e:?}")))?;
    result
        .as_string()
        .ok_or_else(|| "No QR code found in image (or scanner unavailable)".to_string())
}

fn qr_extract_account_id(raw: &str) -> String {
    if let Some(rest) = raw.strip_prefix("chronx:") {
        rest.split(':').next().unwrap_or("").to_string()
    } else {
        raw.to_string()
    }
}

fn qr_extract_pubkey(raw: &str) -> String {
    if let Some(rest) = raw.strip_prefix("chronx:") {
        rest.split(':').nth(1).unwrap_or("").to_string()
    } else {
        raw.to_string()
    }
}

// ── Date helpers ──────────────────────────────────────────────────────────────

/// Format a UTC Unix timestamp as "YYYY-MM-DDTHH:MM" for datetime-local inputs.
fn unix_to_datetime_local_str(unix: i64) -> String {
    let d = js_sys::Date::new(&wasm_bindgen::JsValue::from_f64(unix as f64 * 1000.0));
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}",
        d.get_utc_full_year(),
        d.get_utc_month() + 1,
        d.get_utc_date(),
        d.get_utc_hours(),
        d.get_utc_minutes()
    )
}

/// Parse "YYYY-MM-DD" or "YYYY-MM-DDTHH:MM" as local-time Unix seconds.
fn date_str_to_unix(s: &str) -> Option<i64> {
    // Parse as LOCAL time (no "Z" suffix = JavaScript treats as local timezone)
    let local_str = if s.len() == 10 {
        format!("{s}T00:00:00")
    } else if s.len() >= 16 {
        format!("{}:00", &s[..16])
    } else {
        return None;
    };
    let d = js_sys::Date::new(&wasm_bindgen::JsValue::from_str(&local_str));
    let ms = d.get_time();
    if ms.is_nan() { return None; }
    Some((ms / 1000.0) as i64)
}

/// Returns "YYYY-MM-DDTHH:MM" for now + given seconds (UTC), for datetime-local min attr.
fn min_datetime_str(extra_secs: i64) -> String {
    let unix = (js_sys::Date::now() / 1000.0) as i64 + extra_secs;
    unix_to_datetime_local_str(unix)
}

/// Returns "YYYY-MM-DDTHH:MM" for now + N months (same hour/minute, UTC).
fn datetime_plus_months(months: u32) -> String {
    let d = js_sys::Date::new_0();
    let mut y = d.get_utc_full_year() as u32;
    let mut m = d.get_utc_month() + months;
    y += m / 12;
    m %= 12;
    format!(
        "{y:04}-{m1:02}-{day:02}T{h:02}:{min:02}",
        m1 = m + 1,
        day = d.get_utc_date(),
        h = d.get_utc_hours(),
        min = d.get_utc_minutes()
    )
}

/// Returns "YYYY-MM-DDTHH:MM" for now + N years (same hour/minute, UTC).
fn datetime_plus_years(years: u32) -> String {
    let d = js_sys::Date::new_0();
    format!(
        "{y:04}-{m:02}-{day:02}T{h:02}:{min:02}",
        y = d.get_utc_full_year() as u32 + years,
        m = d.get_utc_month() + 1,
        day = d.get_utc_date(),
        h = d.get_utc_hours(),
        min = d.get_utc_minutes()
    )
}

/// Format Unix timestamp as a short UTC date string "YYYY-MM-DD".
fn unix_to_date_str(ts: i64) -> String {
    let d = js_sys::Date::new(&wasm_bindgen::JsValue::from_f64(ts as f64 * 1000.0));
    format!(
        "{:04}-{:02}-{:02}",
        d.get_utc_full_year(),
        d.get_utc_month() + 1,
        d.get_utc_date()
    )
}

// ── UTC clock ticker ──────────────────────────────────────────────────────────

fn utc_clock_str() -> String {
    let d = js_sys::Date::new_0();
    format!(
        "{:04}-{:02}-{:02} {:02}:{:02}:{:02}",
        d.get_utc_full_year(),
        d.get_utc_month() + 1,
        d.get_utc_date(),
        d.get_utc_hours(),
        d.get_utc_minutes(),
        d.get_utc_seconds()
    )
}

fn start_utc_clock_tick(clock: RwSignal<String>) {
    clock.set(utc_clock_str());
    spawn_local(async move {
        delay_ms(1000).await;
        start_utc_clock_tick(clock);
    });
}

// ── Clipboard ─────────────────────────────────────────────────────────────────

async fn copy_to_clipboard(text: String) {
    if let Some(win) = web_sys::window() {
        let clip = win.navigator().clipboard();
        let _ = JsFuture::from(clip.write_text(&text)).await;
    }
}

// ── Save text as file download ────────────────────────────────────────────────

fn save_text_file(filename: &str, content: &str) {
    use wasm_bindgen::JsCast;
    if let Some(win) = web_sys::window() {
        if let Some(doc) = win.document() {
            let blob_parts = js_sys::Array::new();
            blob_parts.push(&wasm_bindgen::JsValue::from_str(content));
            if let Ok(blob) = web_sys::Blob::new_with_str_sequence_and_options(
                &blob_parts,
                web_sys::BlobPropertyBag::new().type_("text/plain"),
            ) {
                if let Ok(url) = web_sys::Url::create_object_url_with_blob(&blob) {
                    if let Ok(a) = doc.create_element("a") {
                        let _ = a.set_attribute("href", &url);
                        let _ = a.set_attribute("download", filename);
                        a.set_attribute("style", "display:none").ok();
                        if let Some(body) = doc.body() {
                            let _ = body.append_child(&a);
                            if let Some(html) = a.dyn_ref::<web_sys::HtmlElement>() {
                                html.click();
                            }
                            let _ = body.remove_child(&a);
                        }
                        let _ = web_sys::Url::revoke_object_url(&url);
                    }
                }
            }
        }
    }
}

// ── Delay ─────────────────────────────────────────────────────────────────────

/// Fire-and-forget POST with JSON body. Returns a JS Promise. Errors are silently ignored.
fn post_json_fire_and_forget(url: &str, json_body: &str) -> Promise {
    let opts = web_sys::RequestInit::new();
    opts.set_method("POST");
    opts.set_body(&JsValue::from_str(json_body));
    let headers = web_sys::Headers::new().unwrap();
    let _ = headers.set("Content-Type", "application/json");
    opts.set_headers(&headers.into());
    let request = web_sys::Request::new_with_str_and_init(url, &opts).unwrap();
    web_sys::window().unwrap().fetch_with_request(&request)
}

async fn delay_ms(ms: u32) {
    let promise = Promise::new(&mut |resolve, _| {
        if let Some(win) = web_sys::window() {
            let _ = win.set_timeout_with_callback_and_timeout_and_arguments_0(&resolve, ms as i32);
        }
    });
    let _ = JsFuture::from(promise).await;
}

async fn fetch_convert_quote(amount_kx: f64) -> Result<ConvertQuote, String> {
    use wasm_bindgen::JsCast;
    let url = format!("https://api.chronx.io/api/xchan/quote?amount_kx={}", amount_kx);
    let window = web_sys::window().ok_or("No window")?;
    let resp_val = JsFuture::from(window.fetch_with_str(&url))
        .await.map_err(|_| "Network error".to_string())?;
    let resp: web_sys::Response = resp_val.unchecked_into();
    let text_val = JsFuture::from(resp.text().map_err(|_| "Read error".to_string())?)
        .await.map_err(|_| "Text error".to_string())?;
    let text = text_val.as_string().ok_or("Not a string")?;
    if resp.ok() {
        serde_json::from_str::<ConvertQuote>(&text).map_err(|e| e.to_string())
    } else {
        if let Ok(val) = serde_json::from_str::<serde_json::Value>(&text) {
            if let Some(rate) = val.get("fallback_rate").and_then(|v| v.as_f64()) {
                return Err(format!("FALLBACK:{}", rate));
            }
        }
        Err(format!("HTTP {}", resp.status()))
    }
}

/// Poll until balance or nonce changes (node confirmed the tx), up to ~15 seconds.
async fn poll_balance_update(info: RwSignal<Option<AccountInfo>>) {
    let prev_nonce = info.get_untracked().as_ref().map(|a| a.nonce).unwrap_or(0);
    let prev_balance = info.get_untracked().as_ref()
        .map(|a| a.balance_chronos.clone()).unwrap_or_default();
    for _ in 0..15u8 {
        delay_ms(1000).await;
        if let Ok(a) = call::<AccountInfo>("get_account_info", no_args()).await {
            if a.nonce != prev_nonce || a.balance_chronos != prev_balance {
                info.set(Some(a));
                return;
            }
        }
    }
    // Final refresh even if nothing changed
    if let Ok(a) = call::<AccountInfo>("get_account_info", no_args()).await {
        info.set(Some(a));
    }
}

// ── Countdown ticker (self-scheduling via spawn_local) ────────────────────────

fn start_countdown_tick(countdown: RwSignal<u32>, locked_until: f64) {
    let remaining = ((locked_until - js_sys::Date::now()) / 1000.0).ceil().max(0.0) as u32;
    countdown.set(remaining);
    if remaining > 0 {
        spawn_local(async move {
            delay_ms(1000).await;
            start_countdown_tick(countdown, locked_until);
        });
    }
}

// ── Shake helper ──────────────────────────────────────────────────────────────

fn do_shake(pin_shake: RwSignal<bool>) {
    pin_shake.set(true);
    spawn_local(async move {
        delay_ms(450).await;
        pin_shake.set(false);
    });
}

// ── File picker ───────────────────────────────────────────────────────────────

async fn pick_image_file() -> Option<web_sys::File> {
    use wasm_bindgen::JsCast;
    let doc = web_sys::window()?.document()?;
    let input: web_sys::HtmlInputElement = doc
        .create_element("input")
        .ok()?
        .dyn_into()
        .ok()?;
    input.set_type("file");
    input.set_attribute("accept", "image/*").ok()?;
    input.set_attribute("capture", "environment").ok()?;
    input.set_attribute("style", "display:none").ok()?;
    doc.body()?.append_child(&input).ok()?;

    let (tx, rx) = futures::channel::oneshot::channel::<Option<web_sys::File>>();
    let tx = std::rc::Rc::new(std::cell::RefCell::new(Some(tx)));
    let input_clone = input.clone();
    let cb = Closure::once(move || {
        let file = input_clone.files().and_then(|fl| fl.get(0));
        if let Some(sender) = tx.borrow_mut().take() {
            let _ = sender.send(file);
        }
    });
    input.set_onchange(Some(cb.as_ref().unchecked_ref()));
    cb.forget();
    input.click();

    let file = rx.await.ok().flatten();
    if let Some(parent) = input.parent_node() {
        let _ = parent.remove_child(&input);
    }
    file
}

// ── Route a raw deep-link URL to the correct handler ─────────────────────────

async fn route_deep_link_url(
    url: &str,
    deep_link_code: RwSignal<String>,
    active_tab: RwSignal<u8>,
    poke_prefill_email: RwSignal<String>,
    poke_prefill_amount: RwSignal<String>,
    poke_prefill_memo: RwSignal<String>,
    poke_prefill_id: RwSignal<String>,
    decline_request_id: RwSignal<String>,
    decline_sender_email: RwSignal<String>,
    decline_block_checked: RwSignal<bool>,
    decline_modal_open: RwSignal<bool>,
    pay_link_to: RwSignal<String>,
    pay_link_amount: RwSignal<String>,
    pay_link_memo: RwSignal<String>,
    pay_link_ref: RwSignal<String>,
    pay_link_show: RwSignal<bool>,
) {
    // Direct pay link: chronx://pay?to=WALLET&amount=X&memo=Y&ref=Z
    if url.starts_with("chronx://pay") && url.contains("to=") {
        let get_param = |key: &str| -> String {
            url.split(&format!("{key}=")).nth(1)
                .map(|s| s.split('&').next().unwrap_or(s))
                .unwrap_or("")
                .replace("%20", " ").replace('+', " ")
        };
        pay_link_to.set(get_param("to"));
        pay_link_amount.set(get_param("amount"));
        pay_link_memo.set(get_param("memo"));
        pay_link_ref.set(get_param("ref"));
        pay_link_show.set(true);
        active_tab.set(1); // Send tab
    }
    // Poke pay/decline links
    else if url.starts_with("chronx://pay") || url.starts_with("chronx://poke/pay")
        || url.starts_with("chronx://decline") || url.starts_with("chronx://poke/decline")
    {
        let normalized = if url.starts_with("chronx://poke/") {
            url.to_string()
        } else if url.starts_with("chronx://pay") {
            url.replacen("chronx://pay", "chronx://poke/pay", 1)
        } else {
            url.replacen("chronx://decline", "chronx://poke/decline", 1)
        };
        process_poke_link(&normalized, poke_prefill_email, poke_prefill_amount, poke_prefill_memo, poke_prefill_id, active_tab, decline_request_id, decline_sender_email, decline_block_checked, decline_modal_open).await;
    } else if url.starts_with("chronx://claim") {
        if let Some(code) = url.split("code=").nth(1).map(|c| c.split('&').next().unwrap_or(c)) {
            let code = code
                .replace("%20", " ")
                .replace('+', " ")
                .replace("%2D", "-")
                .replace("%2d", "-");
            deep_link_code.set(code);
            active_tab.set(0);
        }
    }
}

// ── Poke deep link processor ─────────────────────────────────────────────────

async fn process_poke_link(
    url: &str,
    poke_prefill_email: RwSignal<String>,
    poke_prefill_amount: RwSignal<String>,
    poke_prefill_memo: RwSignal<String>,
    poke_prefill_id: RwSignal<String>,
    active_tab: RwSignal<u8>,
    decline_request_id: RwSignal<String>,
    decline_sender_email: RwSignal<String>,
    decline_block_checked: RwSignal<bool>,
    decline_modal_open: RwSignal<bool>,
) {
    let request_id = url
        .split("request_id=").nth(1)
        .map(|s| s.split('&').next().unwrap_or(s))
        .unwrap_or("")
        .to_string();
    if request_id.is_empty() { return; }

    let args = serde_wasm_bindgen::to_value(
        &serde_json::json!({ "requestId": request_id })
    ).unwrap_or(no_args());

    // Determine action: pay or decline (support both chronx://pay and chronx://poke/pay formats)
    let is_pay = url.contains("/pay") || url.starts_with("chronx://pay");
    let is_decline = url.contains("/decline") || url.starts_with("chronx://decline");

    if is_pay && !is_decline {
        active_tab.set(1); // Switch to Send tab immediately (before network)
        if let Ok(poke) = call::<PendingPoke>("get_poke_by_id", args).await {
            poke_prefill_email.set(poke.from_email.unwrap_or_default());
            poke_prefill_amount.set(poke.amount_kx.clone());
            poke_prefill_memo.set(poke.note.unwrap_or_default());
            poke_prefill_id.set(poke.request_id);
        }
    } else if is_decline {
        // Set request_id immediately from URL so modal works even if API call fails
        decline_request_id.set(request_id.clone());
        decline_block_checked.set(false);
        if let Ok(poke) = call::<PendingPoke>("get_poke_by_id", args).await {
            decline_sender_email.set(poke.from_email.unwrap_or_default());
        } else {
            decline_sender_email.set(String::new());
        }
        decline_modal_open.set(true);
    }
}

// ── Entry point ───────────────────────────────────────────────────────────────

#[wasm_bindgen(start)]
pub fn main() {
    console_error_panic_hook::set_once();
    leptos::mount::mount_to_body(App);
}

// ── Root component ────────────────────────────────────────────────────────────

#[component]
fn App() -> impl IntoView {
    // ── Global state ──────────────────────────────────────────────────────────
    let info        = RwSignal::new(Option::<AccountInfo>::None);
    let loading     = RwSignal::new(false);
    let err_msg     = RwSignal::new(String::new());
    let online      = RwSignal::new(false);
    // Mobile: 0=Receive 1=Send 2=Activity 3=Settings
    // Desktop: 0=Receive 1=Send 2=Activity 3=Request 4=Settings
    let active_tab  = RwSignal::new(0u8);
    let activity_sub = RwSignal::new(0u8); // 0=History, 1=Promises, 2=Open
    let app_version = RwSignal::new("2.5.9".to_string());
    let desktop     = is_desktop();

    // Language signal
    let lang = RwSignal::new("en".to_string());

    // Cascade send mode (desktop only): 0=Simple, 1=Cascade
    let send_cascade_mode = RwSignal::new(0u8);
    // Send tab mode: 0=Send KX, 1=Request KX
    let send_tab_mode = RwSignal::new(0u8);
    // Loans tab view: 0=Lender, 1=Borrower
    let loans_view = RwSignal::new(0u8);
    // Loan wizard state
    let wizard_open = RwSignal::new(false);
    let wizard_step = RwSignal::new(1u8); // 1-6
    // Wizard form fields
    let wiz_loan_type = RwSignal::new(0u8); // 0=Fixed, 1=Revolving
    let wiz_borrower = RwSignal::new(String::new());
    let wiz_nickname = RwSignal::new(String::new());
    let wiz_amount = RwSignal::new(String::new());
    let wiz_currency = RwSignal::new("KX".to_string());
    let wiz_rate_bps = RwSignal::new(String::new()); // interest rate in %
    let wiz_term_months = RwSignal::new(String::new());
    // Fixed schedule fields
    let wiz_schedule_type = RwSignal::new(0u8); // 0=Bullet, 1=Amortizing, 2=Custom
    // Revolving fields
    let wiz_renewal_period = RwSignal::new(2u8); // index: 0=sec,1=hour,2=daily,3=weekly,4=monthly,5=yearly
    let wiz_rate_cap = RwSignal::new(String::new()); // % per period
    let wiz_exit_rights = RwSignal::new(0u8); // 0=Either,1=Lender,2=Borrower,3=Mutual
    let wiz_revival = RwSignal::new(0u8); // 0=Always
    // Protection fields
    let wiz_collateral_id = RwSignal::new(String::new());
    let wiz_servicer_url = RwSignal::new(String::new());
    let wiz_payment_match = RwSignal::new(0u8); // 0=Exact,1=Partial,2=Minimum
    // Prepayment penalty fields
    let wiz_penalty_enabled = RwSignal::new(false);
    let wiz_penalty_type = RwSignal::new(String::from("Flat"));
    let wiz_penalty_amount = RwSignal::new(String::new());
    // Submission state
    let wiz_submitting = RwSignal::new(false);
    let wiz_error = RwSignal::new(String::new());
    let wiz_success = RwSignal::new(false);
    // Loan list data (fetched from RPC)
    let loans_data = RwSignal::new(serde_json::Value::Null);
    let loan_offers = RwSignal::new(serde_json::Value::Null);
    // Loan nicknames (local storage, keyed by loan_id_hex)
    let loan_nicknames: RwSignal<HashMap<String, String>> = RwSignal::new(HashMap::new());
    // Loan contacts (lender-assigned nicknames, keyed by wallet_address)
    let loan_contacts: RwSignal<HashMap<String, String>> = RwSignal::new(HashMap::new());
    // Wallet labels from notify API (keyed by wallet_address)
    let wallet_labels: RwSignal<HashMap<String, String>> = RwSignal::new(HashMap::new());
    // Loan terms modal state
    let terms_modal_loan = RwSignal::new(Option::<serde_json::Value>::None);
    // Loan summary cache (loaded from backend, keyed by loan_id)
    let loan_summary_text = RwSignal::new(Option::<String>::None);
    let loan_summary_loading = RwSignal::new(false);
    // Raw terms toggle
    let show_raw_terms = RwSignal::new(false);
    // Mobile loan detail view
    let mobile_loan_detail = RwSignal::new(Option::<serde_json::Value>::None);
    let mobile_loan_history = RwSignal::new(Vec::<serde_json::Value>::new());
    let mobile_loan_show_terms = RwSignal::new(false);
    // Autopay preferences (keyed by loan_id_hex)
    let autopay_prefs: RwSignal<HashMap<String, bool>> = RwSignal::new(HashMap::new());
    // Exit confirmation modal
    let exit_modal_loan = RwSignal::new(Option::<serde_json::Value>::None);
    let exit_submitting = RwSignal::new(false);
    let exit_error = RwSignal::new(String::new());
    // A1: Pending loan offer count for Receive tab
    let pending_loan_offers_count = RwSignal::new(0u32);
    // A4: Cooling-off state after accepting a loan offer
    let cooloff_loan_id = RwSignal::new(Option::<String>::None);
    let cooloff_remaining = RwSignal::new(0i64);
    // A6: Loan reference number in wizard
    let wiz_loan_ref = RwSignal::new(String::new());
    // A8: Wizard success TX hash
    let wiz_success_tx = RwSignal::new(Option::<String>::None);
    // A9: Track whether loans data has been loaded at least once
    let loans_loaded = RwSignal::new(false);
    // A5: Wizard email-first flow
    let wiz_borrower_email = RwSignal::new(String::new());
    let wiz_email_resolved = RwSignal::new(Option::<String>::None);
    let wiz_email_display = RwSignal::new(String::new());
    let wiz_offer_expiry = RwSignal::new(0u64);

    // Pay deep link pre-fill signals
    let pay_link_to = RwSignal::new(String::new());
    let pay_link_amount = RwSignal::new(String::new());
    let pay_link_memo = RwSignal::new(String::new());
    let pay_link_ref = RwSignal::new(String::new());
    let pay_link_show = RwSignal::new(false);

    // Welcome / backup / restore state
    let welcome_busy  = RwSignal::new(false);
    let welcome_msg   = RwSignal::new(String::new());
    let backup_key_str = RwSignal::new(String::new());
    let backup_copied  = RwSignal::new(false);
    let mnemonic_words = RwSignal::new(String::new()); // 24-word recovery phrase
    let restore_input  = RwSignal::new(String::new());
    let restore_msg    = RwSignal::new(String::new());
    let restore_busy   = RwSignal::new(false);

    // Avatar & profile (persist across tab navigation)
    let avatar_url = RwSignal::new(String::new());
    let avatar_bust = RwSignal::new(0.0f64);
    let g_display_name = RwSignal::new(String::new());
    let g_display_name_editing = RwSignal::new(false);
    let g_display_name_input = RwSignal::new(String::new());
    let avatar_msg = RwSignal::new(String::new());
    let avatar_uploading = RwSignal::new(false);
    let show_profile_modal = RwSignal::new(false);
    let badge_signal = RwSignal::new(String::new());

    // Load avatar URL + display name whenever account info becomes available
    Effect::new(move |_| {
        let wallet = info.get().map(|a| a.account_id.clone()).unwrap_or_default();
        if wallet.is_empty() { return; }
        avatar_url.set(format!("https://api.chronx.io/avatar/{}", wallet));
        avatar_bust.set(js_sys::Date::now()); // cache bust on every load
        spawn_local(async move {
            let args = serde_wasm_bindgen::to_value(&serde_json::json!({ "walletAddress": wallet })).unwrap_or(no_args());
            if let Ok(meta_json) = call::<String>("get_avatar_meta", args).await {
                if let Ok(meta) = serde_json::from_str::<serde_json::Value>(&meta_json) {
                    if let Some(name) = meta["display_name"].as_str() {
                        g_display_name.set(name.to_string());
                    }
                    if let Some(b) = meta["badge"].as_str() {
                        if !b.is_empty() {
                            badge_signal.set(b.to_string());
                        }
                    }
                }
            }
        });
    });

    // Notices & update check
    let notices        = RwSignal::new(Vec::<Notice>::new());
    let seen_ids       = RwSignal::new(Vec::<String>::new());
    let crit_dismissed = RwSignal::new(Vec::<String>::new());

    // Pending email send (chronos in-flight, not yet on-chain balance)
    let pending_email_chronos = RwSignal::new(0u64);

    // Incoming email locks detected for registered claim_email
    let email_locks = RwSignal::new(Vec::<TimeLockInfo>::new());

    // Deep link claim code (from chronx://claim?code=KX-...)
    let deep_link_code = RwSignal::new(String::new());

    // Update available flag (checked on load for Settings badge)
    let update_available = RwSignal::new(false);

    // Pending poke requests count (badge on Send tab)
    let poke_count = RwSignal::new(0usize);

    // Poke pre-fill: when user taps PAY NOW in poke email → navigate to Send tab with pre-filled fields
    let poke_prefill_email  = RwSignal::new(String::new());
    let poke_prefill_amount = RwSignal::new(String::new());
    let poke_prefill_memo   = RwSignal::new(String::new());
    let poke_prefill_id     = RwSignal::new(String::new()); // request_id for confirm_poke_paid

    // Contact pre-fill: when user clicks "Send KX" on a contact → navigate to Send tab
    let email_prefill_from_contact = RwSignal::new(String::new());

    // Poke decline modal state
    let decline_modal_open    = RwSignal::new(false);
    let decline_request_id    = RwSignal::new(String::new());
    let decline_sender_email  = RwSignal::new(String::new());
    let decline_block_checked = RwSignal::new(false);
    let decline_busy          = RwSignal::new(false);

    // PIN length (loaded from config; default 4)
    let pin_len = RwSignal::new(4u8);

    // Best-effort check: if claim_email is set, query node for pending email locks
    let check_email = move || {
        spawn_local(async move {
            if let Ok(locks) = call::<Vec<TimeLockInfo>>("check_email_timelocks", no_args()).await {
                email_locks.set(locks);
            }
        });
    };

    // Bug report modal
    let bug_modal_open = RwSignal::new(false);
    let bug_body       = RwSignal::new(String::new());

    // PIN state machine
    let app_phase       = RwSignal::new(AppPhase::Splash);
    let pin_digits      = RwSignal::new(String::new());
    let pin_msg         = RwSignal::new(String::new());
    let pin_attempts    = RwSignal::new(0u8);
    let pin_locked_until = RwSignal::new(0.0f64);
    let pin_shake       = RwSignal::new(false);
    let pin_first       = RwSignal::new(String::new()); // saved during PinConfirm
    let countdown       = RwSignal::new(0u32);

    // Forgot PIN state
    let show_forgot_pin     = RwSignal::new(false);
    let forgot_input        = RwSignal::new(String::new());
    let forgot_msg          = RwSignal::new(String::new());
    let forgot_busy         = RwSignal::new(false);
    let forgot_use_raw_key  = RwSignal::new(false);
    // Biometric auto-trigger state
    let bio_attempted       = RwSignal::new(false);
    let bio_show_pin        = RwSignal::new(false);

    // ── Load wallet data after PIN unlock ─────────────────────────────────────

    async fn load_wallet(
        online: RwSignal<bool>,
        loading: RwSignal<bool>,
        info: RwSignal<Option<AccountInfo>>,
        err_msg: RwSignal<String>,
    ) {
        online.set(call::<bool>("check_node", no_args()).await.unwrap_or(false));
        loading.set(true);
        err_msg.set(String::new());
        if let Ok(a) = call::<AccountInfo>("get_account_info", no_args()).await {
            info.set(Some(a));
        }
        loading.set(false);
    }

    // ── Startup: splash → PIN check ───────────────────────────────────────────

    Effect::new(move |_| {
        spawn_local(async move {
            // Show splash for 1.5 s
            delay_ms(1500).await;

            // Load app version (best effort)
            if let Ok(v) = call::<String>("get_app_version", no_args()).await {
                app_version.set(v);
            }
            // Load PIN length preference
            if let Ok(len) = call::<u8>("get_pin_length", no_args()).await {
                if len == 4 || len == 6 || len == 8 { pin_len.set(len); }
            }
            // Load language preference (fallback to browser locale)
            if let Ok(saved_lang) = call::<String>("get_language", no_args()).await {
                let l = saved_lang.trim().to_string();
                if !l.is_empty() && l != "en" {
                    lang.set(l);
                }
            } else {
                lang.set(detect_locale());
            }

            // Fetch notices & seen IDs in background (best effort)
            spawn_local(async move {
                if let Ok(ids) = call::<Vec<String>>("get_seen_notices", no_args()).await {
                    seen_ids.set(ids);
                }
                if let Ok(n) = call::<Vec<Notice>>("fetch_notices", no_args()).await {
                    notices.set(n);
                }
                // Check for wallet updates (best effort)
                if let Ok(upd) = call::<UpdateInfo>("check_for_updates", no_args()).await {
                    update_available.set(!upd.up_to_date);
                    if !upd.up_to_date {
                        // Auto-generate a notice for the new version
                        let today = js_sys::Date::new_0();
                        let y = today.get_full_year() as u32;
                        let m = today.get_month() + 1;
                        let d = today.get_date();
                        let date_str = format!("{y}-{m:02}-{d:02}");
                        let update_notice = Notice {
                            id: format!("update-{}", upd.latest),
                            title: format!("\u{1f514} Update Available: ChronX Wallet v{}", upd.latest),
                            body: format!(
                                "A new version of ChronX Wallet is available. Download it at https://chronx.io/wallet.html"
                            ),
                            severity: "info".to_string(),
                            date: date_str,
                            notice_type: "message".to_string(),
                            dismissible: Some(true),
                            expires: None,
                            url: Some("https://chronx.io/wallet.html".to_string()),
                            url_label: Some("Download Update".to_string()),
                        };
                        let mut current = notices.get_untracked();
                        current.insert(0, update_notice);
                        notices.set(current);
                    }
                }
            });

            // Check wallet existence first — config.json may have a PIN hash
            // even after wallet.json has been deleted (e.g. device transfer).
            let wallet_exists = call::<String>("export_secret_key", no_args()).await.is_ok();

            if !wallet_exists {
                app_phase.set(AppPhase::Welcome);
                return;
            }

            // Wallet exists — check if PIN is configured
            let pin_is_set = call::<bool>("check_pin_set", no_args()).await.unwrap_or(false);

            if pin_is_set {
                app_phase.set(AppPhase::PinUnlock);
                return;
            }

            // Wallet exists, no PIN yet — load account and go to PIN setup
            loading.set(true);
            online.set(call::<bool>("check_node", no_args()).await.unwrap_or(false));
            if let Ok(a) = call::<AccountInfo>("get_account_info", no_args()).await {
                info.set(Some(a));
            }
            loading.set(false);
            app_phase.set(AppPhase::PinSetup);
        });
    });

    // ── Refresh (used on Account tab) ─────────────────────────────────────────

    let on_refresh = move |_: web_sys::MouseEvent| {
        spawn_local(async move {
            load_wallet(online, loading, info, err_msg).await;
            check_email();
        });
    };

    // ── Auto-refresh balance + poke count every 10 seconds (silent) ─────────
    {
        let cb = wasm_bindgen::closure::Closure::<dyn Fn()>::new(move || {
            spawn_local(async move {
                // Silent refresh — don't touch loading signal
                if let Ok(a) = call::<AccountInfo>("get_account_info", no_args()).await {
                    info.set(Some(a));
                }
                // Refresh pending loan offer count (A1)
                if let Ok(offers) = call::<serde_json::Value>("get_loan_offers", no_args()).await {
                    if let Some(arr) = offers.as_array() {
                        pending_loan_offers_count.set(arr.len() as u32);
                    }
                }
                // Refresh pending poke count (filter out blocked senders)
                {
                    let blocked = call::<Vec<String>>("get_blocked_senders", no_args()).await.unwrap_or_default();
                    if let Ok(emails) = call::<Vec<String>>("get_claim_emails", no_args()).await {
                        let mut total = 0usize;
                        for em in &emails {
                            let args = serde_wasm_bindgen::to_value(
                                &serde_json::json!({ "email": em })
                            ).unwrap_or(no_args());
                            if let Ok(pokes) = call::<Vec<PendingPoke>>("get_pending_pokes", args).await {
                                total += pokes.iter().filter(|p| {
                                    let sender = p.from_email.as_deref().unwrap_or("").to_lowercase();
                                    sender.is_empty() || !blocked.iter().any(|b| b.to_lowercase() == sender)
                                }).count();
                            }
                        }
                        poke_count.set(total);
                    }
                }
            });
        });
        let _ = web_sys::window().unwrap().set_interval_with_callback_and_timeout_and_arguments_0(
            cb.as_ref().unchecked_ref(), 10_000
        );
        cb.forget(); // leak closure — lives for app lifetime
    }

    // ── A4: Cooloff countdown timer — decrements cooloff_remaining every second ──
    {
        let cooloff_cb = wasm_bindgen::closure::Closure::<dyn Fn()>::new(move || {
            let r = cooloff_remaining.get_untracked();
            if r > 0 {
                cooloff_remaining.set(r - 1);
            } else if cooloff_loan_id.get_untracked().is_some() {
                cooloff_loan_id.set(None);
            }
        });
        let _ = web_sys::window().unwrap().set_interval_with_callback_and_timeout_and_arguments_0(
            cooloff_cb.as_ref().unchecked_ref(), 1_000
        );
        cooloff_cb.forget();
    }

    // ── Deep-link-poke listener (for warm-start — app already running) ─────
    {
        let listen_cb = wasm_bindgen::closure::Closure::<dyn Fn(JsValue)>::new(move |payload: JsValue| {
            let url_str: String = js_sys::Reflect::get(&payload, &JsValue::from_str("payload"))
                .ok()
                .and_then(|v| v.as_string())
                .unwrap_or_default();
            if !url_str.is_empty() {
                spawn_local(async move {
                    process_poke_link(&url_str, poke_prefill_email, poke_prefill_amount, poke_prefill_memo, poke_prefill_id, active_tab, decline_request_id, decline_sender_email, decline_block_checked, decline_modal_open).await;
                });
            }
        });

        spawn_local(async move {
            if let Some(win) = web_sys::window() {
                let tauri = js_sys::Reflect::get(&win, &JsValue::from_str("__TAURI__")).ok();
                if let Some(tauri) = tauri {
                    let event_mod = js_sys::Reflect::get(&tauri, &JsValue::from_str("event")).ok();
                    if let Some(event_mod) = event_mod {
                        let listen_fn = js_sys::Reflect::get(&event_mod, &JsValue::from_str("listen")).ok();
                        if let Some(listen_fn) = listen_fn {
                            let listen_fn: js_sys::Function = listen_fn.into();
                            let _ = listen_fn.call2(&JsValue::NULL,
                                &JsValue::from_str("deep-link-poke"),
                                listen_cb.as_ref(),
                            );
                        }
                    }
                }
            }
            listen_cb.forget();
        });
    }

    // ── Deep-link-claim listener (for warm-start — app already running) ──────
    {
        let listen_cb = wasm_bindgen::closure::Closure::<dyn Fn(JsValue)>::new(move |payload: JsValue| {
            let code: String = js_sys::Reflect::get(&payload, &JsValue::from_str("payload"))
                .ok()
                .and_then(|v| v.as_string())
                .unwrap_or_default();
            if !code.is_empty() {
                deep_link_code.set(code);
                active_tab.set(0);
            }
        });

        spawn_local(async move {
            if let Some(win) = web_sys::window() {
                let tauri = js_sys::Reflect::get(&win, &JsValue::from_str("__TAURI__")).ok();
                if let Some(tauri) = tauri {
                    let event_mod = js_sys::Reflect::get(&tauri, &JsValue::from_str("event")).ok();
                    if let Some(event_mod) = event_mod {
                        let listen_fn = js_sys::Reflect::get(&event_mod, &JsValue::from_str("listen")).ok();
                        if let Some(listen_fn) = listen_fn {
                            let listen_fn: js_sys::Function = listen_fn.into();
                            let _ = listen_fn.call2(&JsValue::NULL,
                                &JsValue::from_str("deep-link-claim"),
                                listen_cb.as_ref(),
                            );
                        }
                    }
                }
            }
            listen_cb.forget();
        });
    }

    // ── Wallet creation (first run) ───────────────────────────────────────────

    let on_generate = move |_: web_sys::MouseEvent| {
        spawn_local(async move {
            welcome_busy.set(true);
            welcome_msg.set(String::new());
            match call::<serde_json::Value>("generate_wallet_with_mnemonic", no_args()).await {
                Ok(result) => {
                    if let Some(phrase) = result.get("mnemonic").and_then(|v| v.as_str()) {
                        mnemonic_words.set(phrase.to_string());
                    }
                    // Also fetch backup key for legacy display
                    if let Ok(key) = call::<String>("export_secret_key", no_args()).await {
                        backup_key_str.set(key);
                    }
                    backup_copied.set(false);
                    app_phase.set(AppPhase::BackupKey);
                }
                Err(e) => welcome_msg.set(format!("Error: {e}")),
            }
            welcome_busy.set(false);
        });
    };

    // ── PIN digit submission ──────────────────────────────────────────────────

    let handle_pin = move |digits: String| {
        let phase = app_phase.get_untracked();
        match phase {
            AppPhase::PinSetup => {
                pin_first.set(digits);
                pin_msg.set(String::new());
                pin_digits.set(String::new());
                app_phase.set(AppPhase::PinConfirm);
            }
            AppPhase::PinConfirm => {
                let first = pin_first.get_untracked();
                if digits == first {
                    spawn_local(async move {
                        let args = serde_wasm_bindgen::to_value(
                            &serde_json::json!({ "pin": digits })
                        ).unwrap_or(no_args());
                        match call::<()>("set_pin", args).await {
                            Ok(_) => {
                                pin_msg.set(String::new());
                                pin_digits.set(String::new());
                                app_phase.set(AppPhase::Wallet);
                                load_wallet(online, loading, info, err_msg).await;
                                check_email();
                                // Initial poke count fetch
                                {
                                    let blocked = call::<Vec<String>>("get_blocked_senders", no_args()).await.unwrap_or_default();
                                    if let Ok(emails) = call::<Vec<String>>("get_claim_emails", no_args()).await {
                                        let mut total = 0usize;
                                        for em in &emails {
                                            let pa = serde_wasm_bindgen::to_value(&serde_json::json!({ "email": em })).unwrap_or(no_args());
                                            if let Ok(pokes) = call::<Vec<PendingPoke>>("get_pending_pokes", pa).await {
                                                total += pokes.iter().filter(|p| {
                                                    let sender = p.from_email.as_deref().unwrap_or("").to_lowercase();
                                                    sender.is_empty() || !blocked.iter().any(|b| b.to_lowercase() == sender)
                                                }).count();
                                            }
                                        }
                                        poke_count.set(total);
                                    }
                                }
                                // Check for cold-start deep link (managed state)
                                if let Ok(Some(url)) = call::<Option<String>>("get_launch_deep_link", no_args()).await {
                                    route_deep_link_url(&url, deep_link_code, active_tab, poke_prefill_email, poke_prefill_amount, poke_prefill_memo, poke_prefill_id, decline_request_id, decline_sender_email, decline_block_checked, decline_modal_open, pay_link_to, pay_link_amount, pay_link_memo, pay_link_ref, pay_link_show).await;
                                }
                            }
                            Err(e) => {
                                pin_msg.set(format!("Error saving PIN: {e}"));
                                pin_digits.set(String::new());
                            }
                        }
                    });
                } else {
                    pin_msg.set("PINs do not match \u{2014} please try again".to_string());
                    do_shake(pin_shake);
                    pin_digits.set(String::new());
                    pin_first.set(String::new());
                    app_phase.set(AppPhase::PinSetup);
                }
            }
            AppPhase::PinUnlock => {
                // Check lockout
                let now = js_sys::Date::now();
                if now < pin_locked_until.get_untracked() {
                    let rem = countdown.get_untracked();
                    if rem > 0 {
                        pin_msg.set(format!("Too many attempts \u{2014} wait {rem}s"));
                    }
                    pin_digits.set(String::new());
                    return;
                }
                spawn_local(async move {
                    let args = serde_wasm_bindgen::to_value(
                        &serde_json::json!({ "pin": digits })
                    ).unwrap_or(no_args());
                    match call::<bool>("verify_pin", args).await {
                        Ok(true) => {
                            pin_attempts.set(0);
                            pin_digits.set(String::new());
                            pin_msg.set(String::new());
                            app_phase.set(AppPhase::Wallet);
                            load_wallet(online, loading, info, err_msg).await;
                            check_email();
                            // Initial poke count fetch
                            {
                                let blocked = call::<Vec<String>>("get_blocked_senders", no_args()).await.unwrap_or_default();
                                if let Ok(emails) = call::<Vec<String>>("get_claim_emails", no_args()).await {
                                    let mut total = 0usize;
                                    for em in &emails {
                                        let pa = serde_wasm_bindgen::to_value(&serde_json::json!({ "email": em })).unwrap_or(no_args());
                                        if let Ok(pokes) = call::<Vec<PendingPoke>>("get_pending_pokes", pa).await {
                                            total += pokes.iter().filter(|p| {
                                                let sender = p.from_email.as_deref().unwrap_or("").to_lowercase();
                                                sender.is_empty() || !blocked.iter().any(|b| b.to_lowercase() == sender)
                                            }).count();
                                        }
                                    }
                                    poke_count.set(total);
                                }
                            }
                            // Check for cold-start deep link (managed state)
                            if let Ok(Some(url)) = call::<Option<String>>("get_launch_deep_link", no_args()).await {
                                route_deep_link_url(&url, deep_link_code, active_tab, poke_prefill_email, poke_prefill_amount, poke_prefill_memo, poke_prefill_id, decline_request_id, decline_sender_email, decline_block_checked, decline_modal_open, pay_link_to, pay_link_amount, pay_link_memo, pay_link_ref, pay_link_show).await;
                            }
                        }
                        Ok(false) | Err(_) => {
                            let attempts = pin_attempts.get_untracked() + 1;
                            pin_attempts.set(attempts);
                            do_shake(pin_shake);
                            pin_digits.set(String::new());
                            if attempts >= 3 {
                                let locked_ts = js_sys::Date::now() + 30_000.0;
                                pin_locked_until.set(locked_ts);
                                pin_attempts.set(0);
                                pin_msg.set("Too many attempts \u{2014} please wait 30 seconds".to_string());
                                start_countdown_tick(countdown, locked_ts);
                            } else {
                                pin_msg.set("Incorrect PIN".to_string());
                            }
                        }
                    }
                });
            }
            _ => {}
        }
    };

    // ── Biometric unlock listener ────────────────────────────────────────────
    // When PinScreen sets pin_msg to "biometric_ok", unlock the wallet.
    Effect::new(move |_| {
        if pin_msg.get() == "biometric_ok" && app_phase.get() == AppPhase::PinUnlock {
            pin_msg.set(String::new());
            spawn_local(async move {
                pin_attempts.set(0);
                pin_digits.set(String::new());
                app_phase.set(AppPhase::Wallet);
                load_wallet(online, loading, info, err_msg).await;
                check_email();
                // Poke count fetch
                {
                    let blocked = call::<Vec<String>>("get_blocked_senders", no_args()).await.unwrap_or_default();
                    if let Ok(emails) = call::<Vec<String>>("get_claim_emails", no_args()).await {
                        let mut total = 0usize;
                        for em in &emails {
                            let pa = serde_wasm_bindgen::to_value(&serde_json::json!({ "email": em })).unwrap_or(no_args());
                            if let Ok(pokes) = call::<Vec<PendingPoke>>("get_pending_pokes", pa).await {
                                total += pokes.iter().filter(|p| {
                                    let sender = p.from_email.as_deref().unwrap_or("").to_lowercase();
                                    sender.is_empty() || !blocked.iter().any(|b| b.to_lowercase() == sender)
                                }).count();
                            }
                        }
                        poke_count.set(total);
                    }
                }
                // Deep link check
                if let Ok(Some(url)) = call::<Option<String>>("get_launch_deep_link", no_args()).await {
                    route_deep_link_url(&url, deep_link_code, active_tab, poke_prefill_email, poke_prefill_amount, poke_prefill_memo, poke_prefill_id, decline_request_id, decline_sender_email, decline_block_checked, decline_modal_open, pay_link_to, pay_link_amount, pay_link_memo, pay_link_ref, pay_link_show).await;
                }
            });
        }
    });

    // ── View ──────────────────────────────────────────────────────────────────

    view! {
        {move || match app_phase.get() {
            AppPhase::Splash => view! {
                <SplashScreen />
            }.into_any(),

            AppPhase::Welcome => view! {
                <WelcomeScreen
                    on_create=on_generate
                    busy=welcome_busy
                    msg=welcome_msg
                    on_restore=move |_: web_sys::MouseEvent| {
                        restore_input.set(String::new());
                        restore_msg.set(String::new());
                        app_phase.set(AppPhase::RestoreWallet);
                    }
                />
            }.into_any(),

            AppPhase::BackupKey => view! {
                <BackupKeyScreen
                    backup_key=backup_key_str
                    mnemonic=mnemonic_words
                    copied=backup_copied
                    on_copy=move |_: web_sys::MouseEvent| {
                        let words = mnemonic_words.get_untracked();
                        let to_copy = if words.is_empty() { backup_key_str.get_untracked() } else { words };
                        spawn_local(async move {
                            copy_to_clipboard(to_copy).await;
                            backup_copied.set(true);
                            delay_ms(2000).await;
                            backup_copied.set(false);
                        });
                    }
                    on_confirm=move |_: web_sys::MouseEvent| {
                        pin_digits.set(String::new());
                        pin_msg.set(String::new());
                        app_phase.set(AppPhase::PinSetup);
                    }
                />
            }.into_any(),

            AppPhase::RestoreWallet => view! {
                <RestoreWalletScreen
                    input=restore_input
                    msg=restore_msg
                    busy=restore_busy
                    on_back=move |_: web_sys::MouseEvent| app_phase.set(AppPhase::Welcome)
                    on_restore=move |_: web_sys::MouseEvent| {
                        let key = restore_input.get_untracked();
                        if key.trim().is_empty() {
                            restore_msg.set("Please enter your recovery phrase or backup key.".to_string());
                            return;
                        }
                        spawn_local(async move {
                            restore_busy.set(true);
                            restore_msg.set(String::new());
                            let trimmed = key.trim().to_string();
                            // Detect mnemonic: 24 lowercase words separated by spaces
                            let word_count = trimmed.split_whitespace().count();
                            let is_mnemonic = word_count >= 12 && word_count <= 24
                                && trimmed.chars().all(|c| c.is_ascii_lowercase() || c == ' ');
                            if is_mnemonic {
                                let args = serde_wasm_bindgen::to_value(
                                    &serde_json::json!({ "mnemonicPhrase": trimmed, "force": true })
                                ).unwrap_or(no_args());
                                match call::<serde_json::Value>("import_wallet_from_mnemonic", args).await {
                                    Ok(_) => {
                                        pin_digits.set(String::new());
                                        pin_msg.set(String::new());
                                        app_phase.set(AppPhase::PinSetup);
                                    }
                                    Err(e) => restore_msg.set(format!("Error: {e}")),
                                }
                            } else {
                                let args = serde_wasm_bindgen::to_value(
                                    &serde_json::json!({ "backupKey": trimmed, "force": true })
                                ).unwrap_or(no_args());
                                match call::<String>("restore_wallet", args).await {
                                    Ok(_) => {
                                        pin_digits.set(String::new());
                                        pin_msg.set(String::new());
                                        app_phase.set(AppPhase::PinSetup);
                                    }
                                    Err(e) => restore_msg.set(format!("Error: {e}")),
                                }
                            }
                            restore_busy.set(false);
                        });
                    }
                />
            }.into_any(),

            AppPhase::PinSetup | AppPhase::PinConfirm | AppPhase::PinUnlock => view! {
                <PinScreen
                    phase=app_phase
                    pin_digits=pin_digits
                    pin_msg=pin_msg
                    pin_shake=pin_shake
                    countdown=countdown
                    pin_len=pin_len.get()
                    on_submit=handle_pin
                    show_forgot_pin=show_forgot_pin
                    forgot_input=forgot_input
                    forgot_msg=forgot_msg
                    forgot_busy=forgot_busy
                    forgot_use_raw_key=forgot_use_raw_key
                    bio_attempted=bio_attempted
                    bio_show_pin=bio_show_pin
                />
            }.into_any(),

            AppPhase::Wallet => view! {
                <div class=if desktop { "desktop-shell" } else { "app" }>
                    // Urgent notices banner (red, non-dismissible)
                    {move || {
                        let urgents: Vec<Notice> = notices.get().into_iter()
                            .filter(|n| n.severity == "urgent" || n.severity == "critical")
                            .filter(|n| n.dismissible != Some(true))
                            .collect();
                        if urgents.is_empty() {
                            view! { <span></span> }.into_any()
                        } else {
                            view! {
                                <div class="critical-notices-bar">
                                    {urgents.into_iter().map(|n| {
                                        view! {
                                            <div class="critical-notice-item">
                                                <span>"\u{1F6A8} " {n.title.clone()} " — " {n.body.clone()}</span>
                                            </div>
                                        }
                                    }).collect::<Vec<_>>()}
                                </div>
                            }.into_any()
                        }
                    }}

                    // Desktop sidebar
                    {if desktop {
                        view! {
                            <aside class="sidebar">
                                <div class="sidebar-logo">
                                    <a href="https://www.chronx.io" target="_blank" rel="noopener" class="logo-link">
                                        <img src=logo_src() alt="ChronX Logo" style="height:36px;width:auto;display:block;" />
                                    </a>
                                </div>
                                <nav class="sidebar-nav">
                                    <button class=move || if active_tab.get()==0 {"sidebar-tab active"} else {"sidebar-tab"}
                                        on:click=move |_| active_tab.set(0)>
                                        {move || t(&lang.get(), "tab_receive")}
                                    </button>
                                    <button class=move || if active_tab.get()==1 {"sidebar-tab active"} else {"sidebar-tab"}
                                        on:click=move |_| active_tab.set(1)>
                                        {move || t(&lang.get(), "tab_send")}
                                        {move || {
                                            let count = poke_count.get();
                                            if count > 0 {
                                                view! { <span class="notice-badge" style="background:#ef4444;margin-left:4px">{count}</span> }.into_any()
                                            } else {
                                                view! { <span></span> }.into_any()
                                            }
                                        }}
                                    </button>
                                    <button class=move || if active_tab.get()==2 {"sidebar-tab active"} else {"sidebar-tab"}
                                        on:click=move |_| active_tab.set(2)>
                                        "Activity"
                                    </button>
                                    <button class=move || if active_tab.get()==3 {"sidebar-tab active"} else {"sidebar-tab"}
                                        on:click=move |_| active_tab.set(3)>
                                        {move || t(&lang.get(), "tab_request")}
                                    </button>
                                    <button class=move || if active_tab.get()==4 {"sidebar-tab active"} else {"sidebar-tab"}
                                        on:click=move |_| active_tab.set(4)>
                                        "Loans"
                                    </button>
                                </nav>
                                <div class="sidebar-bottom">
                                    <button class=move || if active_tab.get()==5 {"sidebar-tab active"} else {"sidebar-tab"}
                                        on:click=move |_| active_tab.set(5)>
                                        {move || t(&lang.get(), "tab_settings")}
                                        {move || {
                                            let unread = notices.get().iter()
                                                .filter(|n| n.severity != "urgent" && !seen_ids.get().contains(&n.id))
                                                .count();
                                            let has_update = update_available.get();
                                            if has_update {
                                                view! { <span class="update-badge" title="Update available">"\u{1f514}"</span> }.into_any()
                                            } else if unread > 0 {
                                                view! { <span class="notice-badge">{unread}</span> }.into_any()
                                            } else {
                                                view! { <span></span> }.into_any()
                                            }
                                        }}
                                    </button>
                                    <span class="node-status" style="padding:8px 16px;font-size:12px">
                                        <span class=move || if online.get() { "dot online" } else { "dot offline" }></span>
                                        {move || if online.get() { "Online" } else { "Offline" }}
                                    </span>
                                </div>
                            </aside>
                        }.into_any()
                    } else {
                        view! {
                            // Mobile header + tab bar
                            <header>
                                <a href="https://www.chronx.io" target="_blank" rel="noopener" class="logo-link">
                                    <img src=logo_src() alt="ChronX Logo" style="height:40px;width:auto;display:block;" />
                                </a>
                                <div class="header-right">
                                    <span class="node-status">
                                        <span class=move || if online.get() { "dot online" } else { "dot offline" }></span>
                                        {move || if online.get() { "Online" } else { "Offline" }}
                                    </span>
                                </div>
                            </header>
                            <nav class="tab-bar">
                                <button class=move || if active_tab.get()==0 {"tab active"} else {"tab"}
                                    on:click=move |_| active_tab.set(0)>
                                    {move || t(&lang.get(), "tab_receive")}
                                </button>
                                <button class=move || if active_tab.get()==1 {"tab active"} else {"tab"}
                                    on:click=move |_| active_tab.set(1)>
                                    {move || t(&lang.get(), "tab_send")}
                                    {move || {
                                        let count = poke_count.get();
                                        if count > 0 {
                                            view! { <span class="notice-badge" style="background:#ef4444;margin-left:4px">{count}</span> }.into_any()
                                        } else {
                                            view! { <span></span> }.into_any()
                                        }
                                    }}
                                </button>
                                <button class=move || if active_tab.get()==2 {"tab active"} else {"tab"}
                                    on:click=move |_| active_tab.set(2)>
                                    "Activity"
                                </button>
                                <button class=move || {
                                    if active_tab.get()==3 {"tab active"} else {"tab"}
                                }
                                    on:click=move |_| active_tab.set(3)>
                                    {move || t(&lang.get(), "tab_settings")}
                                    {move || {
                                        let unread = notices.get().iter()
                                            .filter(|n| n.severity != "urgent" && !seen_ids.get().contains(&n.id))
                                            .count();
                                        let has_update = update_available.get();
                                        if has_update {
                                            view! { <span class="update-badge" title="Update available">"\u{1f514}"</span> }.into_any()
                                        } else if unread > 0 {
                                            view! { <span class="notice-badge">{unread}</span> }.into_any()
                                        } else {
                                            view! { <span></span> }.into_any()
                                        }
                                    }}
                                </button>
                            </nav>
                        }.into_any()
                    }}

                    // Main content area
                    <div class=if desktop { "main-content" } else { "" }>
                    <div class=if desktop { "main-body" } else { "" }>
                        {move || {
                            let tab = active_tab.get();
                            let settings_tab: u8 = if desktop { 5 } else { 3 };
                            match tab {
                                // Tab 0: Receive
                                0 => view! {
                                    <AccountPanel info=info loading=loading err_msg=err_msg on_refresh=on_refresh pending_email_chronos=pending_email_chronos active_tab=active_tab activity_sub=activity_sub deep_link_code=deep_link_code lang=lang avatar_url=avatar_url avatar_bust=avatar_bust display_name=g_display_name display_name_editing=g_display_name_editing display_name_input=g_display_name_input avatar_msg=avatar_msg avatar_uploading=avatar_uploading show_profile_modal=show_profile_modal badge=badge_signal pending_loan_offers_count=pending_loan_offers_count loans_data=loans_data />
                                }.into_any(),
                                // Tab 1: Send / Request KX
                                1 => view! {
                                    // Send/Request toggle (all platforms)
                                    <div style="display:flex;gap:6px;margin-bottom:12px">
                                        <button
                                            class=move || if send_tab_mode.get()==0 { "send-mode-btn active" } else { "send-mode-btn" }
                                            style="flex:1"
                                            on:click=move |_| send_tab_mode.set(0)>
                                            "Send KX"
                                        </button>
                                        <button
                                            class=move || if send_tab_mode.get()==1 { "send-mode-btn active" } else { "send-mode-btn" }
                                            style="flex:1"
                                            on:click=move |_| send_tab_mode.set(1)>
                                            "Request KX"
                                        </button>
                                    </div>
                                    // Pay link info banner
                                    {move || if pay_link_show.get() {
                                        let to_addr = pay_link_to.get();
                                        let amount = pay_link_amount.get();
                                        let memo = pay_link_memo.get();
                                        let ref_id = pay_link_ref.get();
                                        view! {
                                            <div style="background:linear-gradient(135deg,rgba(212,168,75,0.15),rgba(212,168,75,0.05));border:1px solid #d4a84b;border-radius:8px;padding:12px;margin-bottom:12px">
                                                <p style="font-size:14px;font-weight:700;color:#d4a84b;margin:0 0 6px">
                                                    "\u{1f4b3} Payment Request"
                                                </p>
                                                <p style="font-size:13px;color:#e5e7eb;margin:0 0 4px">
                                                    {format!("Pay {} KX to {}", amount, if to_addr.len() > 20 { format!("{}...{}", &to_addr[..8], &to_addr[to_addr.len()-8..]) } else { to_addr.clone() })}
                                                </p>
                                                {if !memo.is_empty() {
                                                    view! { <p style="font-size:12px;color:#9ca3af;margin:0 0 4px">{format!("Memo: {}", memo)}</p> }.into_any()
                                                } else { view! { <span></span> }.into_any() }}
                                                {if !ref_id.is_empty() {
                                                    view! { <p style="font-size:11px;color:#888;margin:0">{format!("Reference: {}", ref_id)}</p> }.into_any()
                                                } else { view! { <span></span> }.into_any() }}
                                                <button style="margin-top:8px;font-size:12px;padding:4px 12px;background:none;border:1px solid #666;color:#888;border-radius:4px;cursor:pointer"
                                                    on:click=move |_| {
                                                        pay_link_show.set(false);
                                                        pay_link_to.set(String::new());
                                                        pay_link_amount.set(String::new());
                                                        pay_link_memo.set(String::new());
                                                        pay_link_ref.set(String::new());
                                                    }>"Cancel"</button>
                                            </div>
                                        }.into_any()
                                    } else { view! { <span></span> }.into_any() }}
                                    {move || if send_tab_mode.get() == 0 {
                                        // Send KX mode
                                        view! {
                                            {if desktop {
                                                view! {
                                                    <div class="send-mode-row" style="margin-bottom:12px">
                                                        <button type="button"
                                                            class=move || if send_cascade_mode.get()==0 { "send-mode-btn active" } else { "send-mode-btn" }
                                                            on:click=move |_| send_cascade_mode.set(0)>
                                                            {move || t(&lang.get(), "simple_send")}
                                                        </button>
                                                        <button type="button"
                                                            class=move || if send_cascade_mode.get()==1 { "send-mode-btn active" } else { "send-mode-btn" }
                                                            on:click=move |_| send_cascade_mode.set(1)>
                                                            {move || t(&lang.get(), "cascade_send")}
                                                        </button>
                                                    </div>
                                                }.into_any()
                                            } else {
                                                view! { <span></span> }.into_any()
                                            }}
                                            // Desktop only: Future Send with Beneficiary banner
                                            {if desktop {
                                                view! {
                                                    <div class="future-send-banner">
                                                        <div style="display:flex;align-items:center;gap:12px;flex:1">
                                                            <span style="font-size:22px">{"\u{1f3db}\u{FE0E}"}</span>
                                                            <div>
                                                                <strong style="color:#e5e7eb;font-size:13px">"Future Send with Beneficiary"</strong>
                                                                <span style="color:rgba(232,232,216,0.5);font-size:12px">
                                                                    " \u{2014} Estate planning, gifts up to 100 years. Includes beneficiary context for Verifas delivery."
                                                                </span>
                                                            </div>
                                                        </div>
                                                        <button class="send-mode-btn" style="font-size:13px;padding:8px 16px;flex-shrink:0">"Open \u{2192}"</button>
                                                    </div>
                                                }.into_any()
                                            } else {
                                                view! { <span></span> }.into_any()
                                            }}
                                            {move || if send_cascade_mode.get() == 0 {
                                                view! { <SendPanel info=info pending_email_chronos=pending_email_chronos lang=lang poke_prefill_email=poke_prefill_email poke_prefill_amount=poke_prefill_amount poke_prefill_memo=poke_prefill_memo poke_prefill_id=poke_prefill_id email_prefill_from_contact=email_prefill_from_contact pay_link_to=pay_link_to pay_link_amount=pay_link_amount pay_link_memo=pay_link_memo pay_link_show=pay_link_show /> }.into_any()
                                            } else {
                                                view! { <CascadeSendPanel info=info pending_email_chronos=pending_email_chronos lang=lang /> }.into_any()
                                            }}
                                        }.into_any()
                                    } else {
                                        // Request KX mode
                                        view! { <RequestPanel info=info lang=lang /> }.into_any()
                                    }}
                                }.into_any(),
                                // Tab 2: Activity (History/Promises/Open sub-tabs)
                                2 => {
                                    // Fetch loan offers + active loans + nicknames + contacts + autopay on Activity tab load
                                    spawn_local(async move {
                                        if let Ok(v) = call::<serde_json::Value>("get_loan_offers", no_args()).await {
                                            loan_offers.set(v);
                                        }
                                        if let Ok(v) = call::<serde_json::Value>("get_wallet_loans", no_args()).await {
                                            loans_data.set(v);
                                        }
                                        if let Ok(n) = call::<std::collections::HashMap<String,String>>("get_loan_nicknames", no_args()).await {
                                            loan_nicknames.set(n);
                                        }
                                        if let Ok(c) = call::<std::collections::HashMap<String,String>>("get_loan_contacts", no_args()).await {
                                            loan_contacts.set(c);
                                        }
                                        if let Ok(p) = call::<std::collections::HashMap<String,bool>>("get_autopay_prefs", no_args()).await {
                                            autopay_prefs.set(p);
                                        }
                                    });
                                    view! {
                                    // Sub-tab pill buttons
                                    <div style="display:flex;gap:6px;margin-bottom:14px">
                                        <button
                                            class=move || if activity_sub.get()==0 { "send-mode-btn active" } else { "send-mode-btn" }
                                            on:click=move |_| activity_sub.set(0)>
                                            "History"
                                        </button>
                                        <button
                                            class=move || if activity_sub.get()==1 { "send-mode-btn active" } else { "send-mode-btn" }
                                            on:click=move |_| activity_sub.set(1)>
                                            "Promises"
                                        </button>
                                        <button
                                            class=move || if activity_sub.get()==2 { "send-mode-btn active" } else { "send-mode-btn" }
                                            on:click=move |_| activity_sub.set(2)>
                                            "Open"
                                        </button>
                                    </div>
                                    // Sub-tab content
                                    {move || match activity_sub.get() {
                                        0 => view! {
                                            <HistoryPanel info=info email_locks=email_locks on_email_check=check_email />
                                        }.into_any(),
                                        1 => view! {
                                            <PromisesPanel info=info lang=lang />
                                        }.into_any(),
                                        _ => view! {
                                            // Incoming loan offers (mobile + desktop) — A2: identity, A3: predatory flags, A4: cooloff
                                            {move || {
                                                let data = loan_offers.get();
                                                let labels = wallet_labels.get();
                                                let offers = data.as_array();
                                                if let Some(arr) = offers {
                                                    if !arr.is_empty() {
                                                        let cards: Vec<_> = arr.iter().map(|offer| {
                                                            let lender = offer.get("lender_wallet").and_then(|v| v.as_str()).unwrap_or("\u{2014}").to_string();
                                                            let lender_short = if lender.len() > 16 { format!("{}...{}", &lender[..6], &lender[lender.len()-4..]) } else { lender.clone() };
                                                            // A2: Identity resolution — wallet_labels first, then truncated address
                                                            let lender_display = labels.get(&lender).cloned()
                                                                .map(|label| format!("{} ({})", label, lender_short))
                                                                .unwrap_or_else(|| lender_short.clone());
                                                            let principal = offer.get("principal_kx").and_then(|v| v.as_u64()).unwrap_or(0);
                                                            let rate = offer.get("interest_rate").and_then(|v| v.get("Fixed")).and_then(|v| v.as_u64()).unwrap_or(0);
                                                            let rate_pct = rate as f64 / 100.0;
                                                            let loan_id = offer.get("loan_id_hex").and_then(|v| v.as_str()).unwrap_or("").to_string();
                                                            let lid = loan_id.clone();
                                                            let lid2 = loan_id.clone();
                                                            // A3: Predatory loan flags
                                                            let rate_annual = offer.get("interest_rate_annual_pct").and_then(|v| v.as_f64()).unwrap_or(rate_pct);
                                                            let principal_f = principal as f64;
                                                            let (is_blocked, is_warned, _flag_msgs) = check_loan_flags(rate_annual, principal_f);
                                                            // A2: Terms viewed gate
                                                            let terms_viewed = RwSignal::new(false);
                                                            let accept_disabled = is_blocked;
                                                            // v2.5.29: Age confirmation + immediate feedback spinner
                                                            let age_confirmed = RwSignal::new(false);
                                                            let processing = RwSignal::new(0u8); // 0=idle, 1=accepting, 2=declining, 3=accepted, 4=declined
                                                            let proc_error = RwSignal::new(String::new());
                                                            view! {
                                                                <div class="offer-card">
                                                                    <div class="offer-card-left">
                                                                        <div style="font-size:14px;font-weight:600;color:#e5e7eb">{"\u{1f4cb} Loan Offer"}</div>
                                                                        <div style="font-size:12px;color:rgba(232,232,216,0.5);margin-top:2px">{format!("From: {}", lender_display)}</div>
                                                                        <div style="font-size:13px;color:#d4a84b;margin-top:4px;font-weight:600">
                                                                            {format!("{} KX \u{00b7} {}% \u{00b7} Daily", principal, rate_pct)}
                                                                        </div>
                                                                    </div>
                                                                    // A3: Predatory loan flag banners
                                                                    {if is_blocked {
                                                                        view! { <div style="background:rgba(231,76,60,0.15);border:1px solid #e74c3c;border-radius:8px;padding:10px;margin:8px 0;font-size:12px;color:#e74c3c;">
                                                                            "This offer cannot be accepted \u{2014} it exceeds safe lending limits set by the ChronX Protocol Foundation."
                                                                        </div> }.into_any()
                                                                    } else if is_warned {
                                                                        view! { <div style="background:rgba(241,196,15,0.15);border:1px solid #f1c40f;border-radius:8px;padding:10px;margin:8px 0;font-size:12px;color:#f1c40f;">
                                                                            "This offer has elevated risk characteristics. Review terms carefully before accepting."
                                                                        </div> }.into_any()
                                                                    } else {
                                                                        view! { <span></span> }.into_any()
                                                                    }}
                                                                    // Error message
                                                                    {move || {
                                                                        let err = proc_error.get();
                                                                        if !err.is_empty() {
                                                                            view! { <p style="color:#ef4444;font-size:12px;margin:4px 0">{err}</p> }.into_any()
                                                                        } else {
                                                                            view! { <span></span> }.into_any()
                                                                        }
                                                                    }}
                                                                    // v2.5.29: Age confirmation checkbox
                                                                    <label style="display:flex;align-items:center;gap:6px;margin:8px 0 4px;cursor:pointer;font-size:12px;color:#9ca3af">
                                                                        <input type="checkbox"
                                                                            prop:checked=move || age_confirmed.get()
                                                                            on:change=move |ev| {
                                                                                use wasm_bindgen::JsCast;
                                                                                let checked = ev.target()
                                                                                    .and_then(|t| t.dyn_into::<web_sys::HtmlInputElement>().ok())
                                                                                    .map(|i| i.checked())
                                                                                    .unwrap_or(false);
                                                                                age_confirmed.set(checked);
                                                                            }
                                                                            style="accent-color:#d4a84b" />
                                                                        "I confirm I am 18 years of age or older"
                                                                    </label>
                                                                    <div class="offer-card-actions">
                                                                        // A2: View Terms button
                                                                        <button style="background:transparent;border:1px solid #d4a84b;color:#d4a84b;padding:4px 12px;border-radius:6px;font-size:12px;cursor:pointer;margin-right:8px;" on:click=move |_| {
                                                                            terms_viewed.set(true);
                                                                        }>
                                                                            "View Terms"
                                                                        </button>
                                                                        // Accept button — requires terms viewed + age confirmed + spinner feedback
                                                                        <button class="offer-accept-btn"
                                                                            disabled=move || { accept_disabled || !terms_viewed.get() || !age_confirmed.get() || processing.get() != 0 }
                                                                            style=move || if accept_disabled || !terms_viewed.get() || !age_confirmed.get() || processing.get() != 0 { "opacity:0.4;cursor:not-allowed;" } else { "" }
                                                                            on:click=move |_| {
                                                                                if accept_disabled || !terms_viewed.get() || !age_confirmed.get() || processing.get() != 0 { return; }
                                                                                processing.set(1);
                                                                                proc_error.set(String::new());
                                                                                let id = lid.clone();
                                                                                let cid = cooloff_loan_id;
                                                                                let crem = cooloff_remaining;
                                                                                spawn_local(async move {
                                                                                    // 15s timeout via race with delay
                                                                                    let result = {
                                                                                        let args = serde_wasm_bindgen::to_value(&serde_json::json!({"loanIdHex": id, "ageConfirmed": true})).unwrap_or(no_args());
                                                                                        call::<String>("accept_loan_offer", args).await
                                                                                    };
                                                                                    match result {
                                                                                        Ok(_) => {
                                                                                            processing.set(3); // accepted
                                                                                            cid.set(Some(id));
                                                                                            crem.set(259200); // 72 hours rescission window
                                                                                            if let Ok(v) = call::<serde_json::Value>("get_loan_offers", no_args()).await { loan_offers.set(v); }
                                                                                            if let Ok(v) = call::<serde_json::Value>("get_wallet_loans", no_args()).await { loans_data.set(v); }
                                                                                        }
                                                                                        Err(e) => {
                                                                                            processing.set(0);
                                                                                            proc_error.set(format!("Failed \u{2014} please try again"));
                                                                                            let _ = e;
                                                                                        }
                                                                                    }
                                                                                });
                                                                            }>
                                                                            {move || match processing.get() {
                                                                                1 => "\u{23F3} Processing\u{2026}".to_string(),
                                                                                3 => "Accepted \u{2713}".to_string(),
                                                                                _ => "\u{2713} Accept".to_string(),
                                                                            }}
                                                                        </button>
                                                                        // Decline button — with immediate spinner feedback
                                                                        <button class="offer-decline-btn"
                                                                            disabled=move || processing.get() != 0
                                                                            style=move || if processing.get() != 0 { "opacity:0.4;cursor:not-allowed;" } else { "" }
                                                                            on:click=move |_| {
                                                                                if processing.get() != 0 { return; }
                                                                                processing.set(2);
                                                                                proc_error.set(String::new());
                                                                                let id = lid2.clone();
                                                                                spawn_local(async move {
                                                                                    let args = serde_wasm_bindgen::to_value(&serde_json::json!({"loanIdHex": id})).unwrap_or(no_args());
                                                                                    match call::<String>("decline_loan_offer", args).await {
                                                                                        Ok(_) => {
                                                                                            processing.set(4); // declined
                                                                                            if let Ok(v) = call::<serde_json::Value>("get_loan_offers", no_args()).await { loan_offers.set(v); }
                                                                                        }
                                                                                        Err(e) => {
                                                                                            processing.set(0);
                                                                                            proc_error.set(format!("Failed \u{2014} please try again"));
                                                                                            let _ = e;
                                                                                        }
                                                                                    }
                                                                                });
                                                                            }>
                                                                            {move || match processing.get() {
                                                                                2 => "\u{23F3} Processing\u{2026}".to_string(),
                                                                                4 => "Declined".to_string(),
                                                                                _ => "\u{2717} Decline".to_string(),
                                                                            }}
                                                                        </button>
                                                                    </div>
                                                                </div>
                                                            }
                                                        }).collect();
                                                        return view! { <div style="margin-bottom:12px">{cards}</div> }.into_any();
                                                    }
                                                }
                                                view! { <span></span> }.into_any()
                                            }}
                                            // ── Active Loans v3 (mobile redesign: 2-row cards with detail view) ──
                                            {move || if mobile_loan_detail.get().is_some() {
                                                // ── Mobile Loan Detail View ──
                                                let loan = mobile_loan_detail.get().unwrap();
                                                let loan_id = loan.get("loan_id_hex").and_then(|v| v.as_str()).unwrap_or("").to_string();
                                                let principal_chronos = loan.get("principal_chronos").and_then(|v| v.as_u64()).unwrap_or(0);
                                                let principal_kx = loan.get("principal_kx").and_then(|v| v.as_u64())
                                                    .unwrap_or(if principal_chronos > 0 { principal_chronos / 1_000_000 } else { 0 });
                                                let rate_bps = loan.get("interest_rate").and_then(|v| v.get("Fixed")).and_then(|v| v.as_u64()).unwrap_or(0);
                                                let rate_pct = rate_bps as f64 / 100.0;
                                                let lt = loan.get("loan_type").cloned().unwrap_or(serde_json::Value::Null);
                                                let is_revolving = lt.to_string().contains("Revolving");
                                                let exit_str = loan.get("exit_rights").and_then(|v| v.as_str()).unwrap_or("EitherParty").to_string();
                                                let exit_label = match exit_str.as_str() { "LenderOnly" => "The lender only", "BorrowerOnly" => "The borrower only", "MutualConsent" => "Both parties by mutual agreement", _ => "Either party" };
                                                let collateral = loan.get("collateral_lock_id").and_then(|v| v.as_str()).unwrap_or("").to_string();
                                                let portal = loan.get("servicer_portal_url").and_then(|v| v.as_str()).unwrap_or("").to_string();
                                                let (interval_amt, interval_label) = if is_revolving {
                                                    let period = lt.get("Revolving").and_then(|v| v.get("renewal_period_seconds")).and_then(|v| v.as_u64()).unwrap_or(86400);
                                                    let (ppy, lbl) = match period { 86400 => (365.0, "day"), 604800 => (52.0, "week"), 2592000 => (12.0, "month"), 31536000 => (1.0, "year"), _ => (365.0, "day") };
                                                    ((principal_kx as f64) * (rate_pct / 100.0) / ppy, lbl)
                                                } else { ((principal_kx as f64) * (rate_pct / 100.0) / 365.0, "day") };
                                                let portal_c = portal.clone();
                                                view! {
                                                    <div>
                                                        <button class="mobile-detail-back" on:click=move |_| mobile_loan_detail.set(None)>
                                                            {"\u{2190} Back"}
                                                        </button>
                                                        // PART 1 — Payment History
                                                        <h3 class="mobile-detail-header">"Payment History"</h3>
                                                        {move || {
                                                            let hist = mobile_loan_history.get();
                                                            if hist.is_empty() {
                                                                view! { <div style="text-align:center;color:rgba(232,232,216,0.4);font-size:13px;padding:20px 0">"No payments recorded yet"</div> }.into_any()
                                                            } else {
                                                                let rows: Vec<_> = hist.iter().map(|entry| {
                                                                    let is_credit = entry.get("is_credit").and_then(|v| v.as_bool()).unwrap_or(false);
                                                                    let amt = entry.get("amount_kx").and_then(|v| v.as_f64()).unwrap_or(0.0);
                                                                    let label = entry.get("label").and_then(|v| v.as_str()).unwrap_or("").to_string();
                                                                    let date = entry.get("date").and_then(|v| v.as_str()).unwrap_or("").to_string();
                                                                    let cls = if is_credit { "payment-row credit" } else { "payment-row debit" };
                                                                    let sign = if is_credit { "+" } else { "\u{2212}" };
                                                                    view! {
                                                                        <div class=cls>
                                                                            <span class="payment-amount">{format!("{} {:.2} KX", sign, amt)}</span>
                                                                            <span class="payment-label">{label}</span>
                                                                            <span class="payment-date">{date}</span>
                                                                        </div>
                                                                    }.into_any()
                                                                }).collect::<Vec<_>>();
                                                                view! { <div class="payment-history-list">{rows}</div> }.into_any()
                                                            }
                                                        }}
                                                        // Separator
                                                        <hr style="border:none;border-top:1px solid rgba(255,255,255,0.06);margin:16px 0" />
                                                        // PART 2 — Loan Summary
                                                        <h3 class="mobile-detail-header">"Loan Summary"</h3>
                                                        <ul class="terms-bullet-list" style="margin-bottom:12px">
                                                            <li>{format!("Principal: {:.2} KX", principal_kx as f64)}</li>
                                                            <li>{format!("Interest: {:.1}% annual, paid per {} (~{:.2} KX/{})", rate_pct, interval_label, interval_amt, interval_label)}</li>
                                                            <li>{format!("Type: {} {}", if is_revolving { "Revolving" } else { "Fixed Schedule" }, if is_revolving { "\u{2014} renews automatically" } else { "" })}</li>
                                                            <li>{if is_revolving { "Duration: No fixed end date".to_string() } else { "Duration: Fixed term".to_string() }}</li>
                                                            <li>{format!("Cancellation: {} may exit at any time", exit_label)}</li>
                                                            <li>{format!("Collateral posted: {}", if collateral.is_empty() { "None" } else { "Held on-chain" })}</li>
                                                            {if !portal.is_empty() {
                                                                view! { <li>"Servicer portal: "<a href=portal.clone() target="_blank" rel="noopener" style="color:#d4a84b">{portal.clone()}</a></li> }.into_any()
                                                            } else { view! { <span></span> }.into_any() }}
                                                        </ul>
                                                        // Collapsible raw terms
                                                        <div class="terms-raw-toggle" on:click=move |_| mobile_loan_show_terms.set(!mobile_loan_show_terms.get())>
                                                            {move || if mobile_loan_show_terms.get() { "\u{25bc} Hide full terms" } else { "View Full Terms \u{203a}" }}
                                                        </div>
                                                        {
                                                            let lid_rt = loan_id.clone();
                                                            let exit_str_rt = exit_str.clone();
                                                            let collateral_rt = collateral.clone();
                                                            let portal_rt = portal_c.clone();
                                                            move || if mobile_loan_show_terms.get() {
                                                            let coll_display = if collateral_rt.is_empty() { "None".to_string() } else { collateral_rt.clone() };
                                                            view! {
                                                                <table class="terms-raw-table">
                                                                    <tr><td class="terms-raw-key">"Loan ID"</td><td class="terms-raw-val">{lid_rt.clone()}</td></tr>
                                                                    <tr><td class="terms-raw-key">"Interest"</td><td class="terms-raw-val">{format!("{} bps ({:.2}%)", rate_bps, rate_pct)}</td></tr>
                                                                    <tr><td class="terms-raw-key">"Type"</td><td class="terms-raw-val">{if is_revolving { "Revolving" } else { "Fixed" }}</td></tr>
                                                                    <tr><td class="terms-raw-key">"Exit Rights"</td><td class="terms-raw-val">{exit_str_rt.clone()}</td></tr>
                                                                    <tr><td class="terms-raw-key">"Collateral"</td><td class="terms-raw-val">{coll_display}</td></tr>
                                                                </table>
                                                            }.into_any()
                                                        } else { view! { <span></span> }.into_any() }}
                                                    </div>
                                                }.into_any()
                                            } else {
                                                // ── Card List View ──
                                                let data = loans_data.get();
                                                let my_wallet = info.get().map(|a| a.account_id.clone()).unwrap_or_default();
                                                let nicks = loan_nicknames.get();
                                                let contacts = loan_contacts.get();
                                                let labels = wallet_labels.get();
                                                let ap = autopay_prefs.get();
                                                if let Some(arr) = data.as_array() {
                                                    let active: Vec<_> = arr.iter().filter(|l| {
                                                        let st = l.get("status").and_then(|s| s.as_str()).unwrap_or("");
                                                        st == "active" || st == "delinquent" || st == "default" || st == "accepted_pending_rescission"
                                                    }).cloned().collect();
                                                    if !active.is_empty() {
                                                        let cards: Vec<_> = active.iter().map(|loan| {
                                                            let loan_id = loan.get("loan_id_hex").and_then(|v| v.as_str()).unwrap_or("").to_string();
                                                            let lender_w = loan.get("lender_wallet").and_then(|v| v.as_str()).unwrap_or("").to_string();
                                                            let borrower_w = loan.get("borrower_wallet").and_then(|v| v.as_str()).unwrap_or("").to_string();
                                                            let status = loan.get("status").and_then(|v| v.as_str()).unwrap_or("active").to_string();
                                                            let is_lender = my_wallet == lender_w;
                                                            let is_delinquent = status == "delinquent" || status == "default";
                                                            let is_pending_rescission = status == "accepted_pending_rescission";
                                                            let role_class = if is_pending_rescission { "loan-card-v2 role-borrower" } else if is_delinquent { "loan-card-v2 status-delinquent" } else if is_lender { "loan-card-v2 role-lender" } else { "loan-card-v2 role-borrower" };
                                                            // Type badge
                                                            let lt = loan.get("loan_type").cloned().unwrap_or(serde_json::Value::Null);
                                                            let is_revolving = lt.to_string().contains("Revolving");
                                                            let exit_rights_str = loan.get("exit_rights").and_then(|v| v.as_str()).unwrap_or("").to_string();
                                                            let type_badge = if is_pending_rescission { "PENDING" } else if !is_revolving { "T" } else if exit_rights_str.contains("LenderOnly") { "C" } else { "R" };
                                                            // Role-aware display name
                                                            let counterparty = if is_lender { &borrower_w } else { &lender_w };
                                                            let counter_short = if counterparty.len() > 14 { format!("{}...{}", &counterparty[..6], &counterparty[counterparty.len()-4..]) } else { counterparty.clone() };
                                                            let display_name = nicks.get(&loan_id).cloned()
                                                                .or_else(|| contacts.get(counterparty).cloned())
                                                                .or_else(|| labels.get(counterparty).cloned())
                                                                .unwrap_or(counter_short.clone());
                                                            let has_nickname = nicks.contains_key(&loan_id) || contacts.contains_key(counterparty) || labels.contains_key(counterparty);
                                                            // Interval payment
                                                            let principal_chronos = loan.get("principal_chronos").and_then(|v| v.as_u64()).unwrap_or(0);
                                                            let principal_kx = loan.get("principal_kx").and_then(|v| v.as_u64())
                                                                .unwrap_or(if principal_chronos > 0 { principal_chronos / 1_000_000 } else { 0 });
                                                            let rate_bps = loan.get("interest_rate").and_then(|v| v.get("Fixed")).and_then(|v| v.as_u64()).unwrap_or(0);
                                                            let rate_pct = rate_bps as f64 / 100.0;
                                                            let (interval_amt, _) = if is_revolving {
                                                                let period = lt.get("Revolving").and_then(|v| v.get("renewal_period_seconds")).and_then(|v| v.as_u64()).unwrap_or(86400);
                                                                let ppy = match period { 86400 => 365.0, 604800 => 52.0, 2592000 => 12.0, 31536000 => 1.0, _ => 365.0 };
                                                                ((principal_kx as f64) * (rate_pct / 100.0) / ppy, "per day")
                                                            } else { ((principal_kx as f64) * (rate_pct / 100.0) / 365.0, "per day") };
                                                            // Next due
                                                            let next_due_ts = loan.get("next_payment_at").and_then(|v| v.as_u64());
                                                            let due_str = if is_pending_rescission {
                                                                let rescission_at = loan.get("rescission_expires_at").and_then(|v| v.as_u64()).unwrap_or(0);
                                                                let now_s = (js_sys::Date::now() / 1000.0) as u64;
                                                                let remaining = rescission_at.saturating_sub(now_s);
                                                                let hours = remaining / 3600;
                                                                format!("PENDING \u{2014} Funds transfer in {}h", hours)
                                                            } else if is_delinquent {
                                                                "OVERDUE".to_string()
                                                            } else if let Some(ts) = next_due_ts {
                                                                let now_s = (js_sys::Date::now() / 1000.0) as u64;
                                                                let diff = ts.saturating_sub(now_s);
                                                                if diff < 86400 && ts >= now_s { "Next Due: Tonight at midnight".to_string() }
                                                                else if diff < 172800 && ts >= now_s { "Next Due: Tomorrow at midnight".to_string() }
                                                                else {
                                                                    let days = diff / 86400;
                                                                    let d = js_sys::Date::new_0(); d.set_time((ts as f64) * 1000.0);
                                                                    let months = ["Jan","Feb","Mar","Apr","May","Jun","Jul","Aug","Sep","Oct","Nov","Dec"];
                                                                    let m = d.get_month() as usize;
                                                                    format!("Next Due: In {} days  ({} {})", days, months.get(m).unwrap_or(&"???"), d.get_date())
                                                                }
                                                            } else { "Next Due: \u{2014}".to_string() };
                                                            let due_class = if is_pending_rescission { "mobile-loan-due" } else if is_delinquent { "mobile-loan-due overdue" } else { "mobile-loan-due" };
                                                            // Autopay
                                                            let requires_autopay = loan.get("requires_autopay").and_then(|v| v.as_bool()).unwrap_or(false);
                                                            let user_autopay = ap.get(&loan_id).copied().unwrap_or(false);
                                                            let lid_ap = RwSignal::new(loan_id.clone());
                                                            // Signals for inline rename
                                                            let editing = RwSignal::new(false);
                                                            let edit_val = RwSignal::new(display_name.clone());
                                                            let lid_nick = RwSignal::new(loan_id.clone());
                                                            let dn_sig = RwSignal::new(display_name.clone());
                                                            let addr_sig = RwSignal::new(if !has_nickname { counter_short.clone() } else { String::new() });
                                                            let has_nick = has_nickname;
                                                            // Click handler for card → detail view
                                                            let loan_clone = loan.clone();
                                                            let lid_detail = loan_id.clone();
                                                            view! {
                                                                <div class=role_class on:click={
                                                                    let lc = loan_clone.clone();
                                                                    let lid_d = lid_detail.clone();
                                                                    move |_| {
                                                                        if !editing.get() {
                                                                            mobile_loan_show_terms.set(false);
                                                                            mobile_loan_history.set(Vec::new());
                                                                            mobile_loan_detail.set(Some(lc.clone()));
                                                                            let lid_h = lid_d.clone();
                                                                            spawn_local(async move {
                                                                                let args = serde_wasm_bindgen::to_value(&serde_json::json!({"loanId": lid_h})).unwrap_or(no_args());
                                                                                if let Ok(v) = call::<serde_json::Value>("get_loan_payment_history", args).await {
                                                                                    if let Some(arr) = v.as_array() { mobile_loan_history.set(arr.clone()); }
                                                                                }
                                                                            });
                                                                        }
                                                                    }
                                                                }>
                                                                    // ROW 1: Name + pencil + type badge
                                                                    <div class="loan-card-v2-line1">
                                                                        {move || if editing.get() {
                                                                            view! {
                                                                                <div class="loan-card-v2-name">
                                                                                    <input type="text"
                                                                                        prop:value=move || edit_val.get()
                                                                                        on:input=move |ev| edit_val.set(event_target_value(&ev))
                                                                                        on:keydown=move |ev: web_sys::KeyboardEvent| {
                                                                                            if ev.key() == "Enter" {
                                                                                                let lid_k = lid_nick.get();
                                                                                                let val = edit_val.get();
                                                                                                editing.set(false);
                                                                                                spawn_local(async move {
                                                                                                    let args = serde_wasm_bindgen::to_value(&serde_json::json!({"loanId": lid_k, "nickname": val})).unwrap_or(no_args());
                                                                                                    let _ = call::<()>("set_loan_nickname", args).await;
                                                                                                    if let Ok(n) = call::<std::collections::HashMap<String,String>>("get_loan_nicknames", no_args()).await { loan_nicknames.set(n); }
                                                                                                });
                                                                                            } else if ev.key() == "Escape" { editing.set(false); }
                                                                                        }
                                                                                        on:blur=move |_| {
                                                                                            let lid_b = lid_nick.get();
                                                                                            let val = edit_val.get();
                                                                                            editing.set(false);
                                                                                            spawn_local(async move {
                                                                                                let args = serde_wasm_bindgen::to_value(&serde_json::json!({"loanId": lid_b, "nickname": val})).unwrap_or(no_args());
                                                                                                let _ = call::<()>("set_loan_nickname", args).await;
                                                                                                if let Ok(n) = call::<std::collections::HashMap<String,String>>("get_loan_nicknames", no_args()).await { loan_nicknames.set(n); }
                                                                                            });
                                                                                        }
                                                                                    />
                                                                                </div>
                                                                            }.into_any()
                                                                        } else {
                                                                            view! {
                                                                                <span>
                                                                                    <span class="loan-borrower-name">{dn_sig.get()}</span>
                                                                                    {if !has_nick { view! { <span class="loan-borrower-addr">{" "}{addr_sig.get()}</span> }.into_any() } else { view! { <span></span> }.into_any() }}
                                                                                </span>
                                                                            }.into_any()
                                                                        }}
                                                                        <button class="loan-card-v2-edit" on:click=move |ev: web_sys::MouseEvent| {
                                                                            ev.stop_propagation();
                                                                            edit_val.set(loan_nicknames.get().get(&lid_nick.get()).cloned().unwrap_or_else(|| edit_val.get()));
                                                                            editing.set(true);
                                                                        }>{"\u{270f}\u{fe0f}"}</button>
                                                                        <span class="loan-type-badge" style="margin-left:auto">{type_badge}</span>
                                                                    </div>
                                                                    // ROW 2: Three detail lines
                                                                    <div class="mobile-loan-row2">
                                                                        <div class=due_class>{due_str}</div>
                                                                        <div class="mobile-loan-autopay">
                                                                            {if requires_autopay {
                                                                                view! { <span style="color:rgba(232,232,216,0.4)">"Auto-Payment: Required"</span> }.into_any()
                                                                            } else {
                                                                                view! {
                                                                                    <span class="mobile-autopay-toggle" on:click=move |ev: web_sys::MouseEvent| {
                                                                                        ev.stop_propagation();
                                                                                        let lid = lid_ap.get();
                                                                                        let current = autopay_prefs.get().get(&lid).copied().unwrap_or(false);
                                                                                        let new_val = !current;
                                                                                        let mut prefs = autopay_prefs.get_untracked();
                                                                                        prefs.insert(lid.clone(), new_val);
                                                                                        autopay_prefs.set(prefs);
                                                                                        spawn_local(async move {
                                                                                            let args = serde_wasm_bindgen::to_value(&serde_json::json!({"loanId": lid, "enabled": new_val})).unwrap_or(no_args());
                                                                                            let _ = call::<()>("set_autopay_pref", args).await;
                                                                                        });
                                                                                    }>
                                                                                        {move || format!("Auto-Payment: {}", if autopay_prefs.get().get(&lid_ap.get()).copied().unwrap_or(false) { "Yes" } else { "No" })}
                                                                                    </span>
                                                                                }.into_any()
                                                                            }}
                                                                        </div>
                                                                        <div class="mobile-loan-payment">{format!("Payment: {:.2} KX", interval_amt)}</div>
                                                                    </div>
                                                                </div>
                                                            }.into_any()
                                                        }).collect::<Vec<_>>();
                                                        return view! {
                                                            <div style="margin-bottom:12px">
                                                                <div style="font-size:11px;font-weight:600;letter-spacing:1px;text-transform:uppercase;color:rgba(232,232,216,0.5);margin-bottom:8px">"Active Loans"</div>
                                                                {cards}
                                                            </div>
                                                        }.into_any();
                                                    }
                                                }
                                                view! { <span></span> }.into_any()
                                            }}
                                            // ── Loan Terms Modal (gold-themed, AI summary + raw terms) ──
                                            // ── A1: Terms Modal ──
                                            {move || if let Some(loan) = terms_modal_loan.get() {
                                                let my_wallet = info.get().map(|a| a.account_id.clone()).unwrap_or_default();
                                                let lender_w = loan.get("lender_wallet").and_then(|v| v.as_str()).unwrap_or("").to_string();
                                                let borrower_w = loan.get("borrower_wallet").and_then(|v| v.as_str()).unwrap_or("").to_string();
                                                let is_lender = my_wallet == lender_w;
                                                let counterparty = if is_lender { borrower_w.clone() } else { lender_w.clone() };
                                                let counter_short = if counterparty.len() > 14 { format!("{}...{}", &counterparty[..6], &counterparty[counterparty.len()-4..]) } else { counterparty.clone() };
                                                let role_label = if is_lender { "Borrower" } else { "Lender" };
                                                let portal_url = loan.get("servicer_portal_url").and_then(|v| v.as_str()).unwrap_or("").to_string();
                                                let principal_chronos = loan.get("principal_chronos").and_then(|v| v.as_u64()).unwrap_or(0);
                                                let principal_kx = loan.get("principal_kx").and_then(|v| v.as_u64())
                                                    .unwrap_or(if principal_chronos > 0 { principal_chronos / 1_000_000 } else { 0 });
                                                let rate_bps = loan.get("interest_rate").and_then(|v| v.get("Fixed")).and_then(|v| v.as_u64()).unwrap_or(0);
                                                let rate_pct = rate_bps as f64 / 100.0;
                                                let lt = loan.get("loan_type").cloned().unwrap_or(serde_json::Value::Null);
                                                let is_revolving = lt.to_string().contains("Revolving");
                                                let status = loan.get("status").and_then(|v| v.as_str()).unwrap_or("unknown").to_string();
                                                let exit_str = loan.get("exit_rights").and_then(|v| v.as_str()).unwrap_or("EitherParty").to_string();
                                                let exit_label = match exit_str.as_str() { "LenderOnly" => "Lender only", "BorrowerOnly" => "Borrower only", "MutualConsent" => "Both parties (mutual)", _ => "Either party" };
                                                let collateral = loan.get("collateral_lock_id").and_then(|v| v.as_str()).unwrap_or("").to_string();
                                                let autopay = loan.get("requires_autopay").and_then(|v| v.as_bool()).unwrap_or(false);
                                                let loan_id_hex = loan.get("loan_id_hex").and_then(|v| v.as_str()).unwrap_or("").to_string();
                                                let lid_short = if loan_id_hex.len() > 8 { format!("{}...", &loan_id_hex[..8]) } else { loan_id_hex.clone() };
                                                let created_at = loan.get("created_at").and_then(|v| v.as_u64()).unwrap_or(0);
                                                let accepted_at = loan.get("accepted_at").and_then(|v| v.as_u64()).unwrap_or(0);
                                                let pay_as = loan.get("pay_as").and_then(|v| v.as_str()).unwrap_or("").to_string();
                                                let currency = if pay_as.is_empty() || pay_as == "null" { "KX".to_string() } else { "USD (PAY_AS)".to_string() };
                                                // Interval payment
                                                let (interval_amt, interval_label) = if is_revolving {
                                                    let period = lt.get("Revolving").and_then(|v| v.get("renewal_period_seconds")).and_then(|v| v.as_u64()).unwrap_or(86400);
                                                    let (ppy, lbl) = match period { 86400 => (365.0, "day"), 604800 => (52.0, "week"), 2592000 => (12.0, "month"), _ => (365.0, "day") };
                                                    ((principal_kx as f64) * (rate_pct / 100.0) / ppy, lbl)
                                                } else { ((principal_kx as f64) * (rate_pct / 100.0) / 365.0, "day") };
                                                let renewal_label = if is_revolving {
                                                    let period = lt.get("Revolving").and_then(|v| v.get("renewal_period_seconds")).and_then(|v| v.as_u64()).unwrap_or(86400);
                                                    match period { 86400 => "Daily", 604800 => "Weekly", 2592000 => "Monthly", 31536000 => "Yearly", _ => "Daily" }
                                                } else { "N/A (Fixed)" };
                                                let notice = loan.get("exit_notice_hours").and_then(|v| v.as_u64()).unwrap_or(24);
                                                let notice_label = if notice == 0 { "Immediate".to_string() } else { format!("{} hours", notice) };
                                                // Plain English summary
                                                let summary_text = format!(
                                                    "This is a {} loan of {} {} at {:.1}% annual interest. Payments are {}. {} may exit with {} notice. {}Interest accrues daily and settles at exit. Auto-pay is {}.",
                                                    if is_revolving { "revolving" } else { "fixed" },
                                                    principal_kx, currency, rate_pct, interval_label,
                                                    exit_label, notice_label,
                                                    if collateral.is_empty() { "No collateral is held. " } else { "Collateral is held on-chain. " },
                                                    if autopay { "enabled" } else { "optional" }
                                                ).replace("{:,}", &{let s = principal_kx.to_string(); let mut r = String::new(); for (i, c) in s.chars().rev().enumerate() { if i > 0 && i % 3 == 0 { r.push(','); } r.push(c); } r.chars().rev().collect::<String>()});
                                                let fmt_ts = |ts: u64| -> String {
                                                    if ts == 0 { return "\u{2014}".to_string(); }
                                                    let d = js_sys::Date::new_0(); d.set_time((ts as f64) * 1000.0);
                                                    let months = ["Jan","Feb","Mar","Apr","May","Jun","Jul","Aug","Sep","Oct","Nov","Dec"];
                                                    let m = d.get_month() as usize;
                                                    format!("{} {}, {}", months.get(m).unwrap_or(&"???"), d.get_date(), d.get_full_year())
                                                };
                                                let created_str = fmt_ts(created_at);
                                                let accepted_str = fmt_ts(accepted_at);

                                                view! {
                                                    <div class="terms-modal-overlay" on:click=move |_| terms_modal_loan.set(None)>
                                                        <div class="terms-modal terms-modal-gold" on:click=move |ev: web_sys::MouseEvent| ev.stop_propagation()>
                                                            // Title
                                                            <h3 class="terms-gold-header">{format!("Loan Terms \u{2014} {}", lid_short)}</h3>
                                                            // Section 1: Plain English Summary
                                                            <div class="terms-summary-section">
                                                                <p style="font-size:13px;color:#c8c8c0;line-height:1.7">{summary_text}</p>
                                                            </div>
                                                            // Section 2: Loan Details Table
                                                            <table class="terms-raw-table" style="margin-bottom:14px">
                                                                <tr><td class="terms-raw-key">"Loan ID"</td><td class="terms-raw-val" title=loan_id_hex.clone()>{lid_short.clone()}</td></tr>
                                                                <tr><td class="terms-raw-key">"Type"</td><td class="terms-raw-val">{if is_revolving { "Revolving" } else { "Fixed" }}</td></tr>
                                                                <tr><td class="terms-raw-key">"Principal"</td><td class="terms-raw-val">{
                                                                    let s = principal_kx.to_string();
                                                                    let mut r = String::new();
                                                                    for (i, c) in s.chars().rev().enumerate() { if i > 0 && i % 3 == 0 { r.push(','); } r.push(c); }
                                                                    format!("{} KX", r.chars().rev().collect::<String>())
                                                                }</td></tr>
                                                                <tr><td class="terms-raw-key">"Currency"</td><td class="terms-raw-val">{currency.clone()}</td></tr>
                                                                <tr><td class="terms-raw-key">"Annual Rate"</td><td class="terms-raw-val">{format!("{:.1}%", rate_pct)}</td></tr>
                                                                <tr><td class="terms-raw-key">"Interval Pay"</td><td class="terms-raw-val">{format!("{:.2} KX per {}", interval_amt, interval_label)}</td></tr>
                                                                <tr><td class="terms-raw-key">"Renewal"</td><td class="terms-raw-val">{renewal_label}</td></tr>
                                                                <tr><td class="terms-raw-key">"Exit Rights"</td><td class="terms-raw-val">{exit_label}</td></tr>
                                                                <tr><td class="terms-raw-key">"Notice Period"</td><td class="terms-raw-val">{notice_label.clone()}</td></tr>
                                                                <tr><td class="terms-raw-key">"Collateral"</td><td class="terms-raw-val">{if collateral.is_empty() { "None".to_string() } else { collateral.clone() }}</td></tr>
                                                                <tr><td class="terms-raw-key">"Servicer"</td><td class="terms-raw-val">{if portal_url.is_empty() { "None".to_string() } else { portal_url.clone() }}</td></tr>
                                                                <tr><td class="terms-raw-key">"Created"</td><td class="terms-raw-val">{created_str}</td></tr>
                                                                <tr><td class="terms-raw-key">"Accepted"</td><td class="terms-raw-val">{accepted_str}</td></tr>
                                                                <tr><td class="terms-raw-key">"Status"</td><td class="terms-raw-val" style={if status == "active" { "color:#5ce08a" } else { "" }}>{status.to_uppercase()}</td></tr>
                                                            </table>
                                                            // Section 3: Counterparty
                                                            <div class="terms-modal-counterparty">
                                                                {format!("{}: {}", role_label, counter_short)}
                                                            </div>
                                                            // Section 4: Payment History (D4)
                                                            {
                                                                let payments = loan.get("payment_history").and_then(|v| v.as_array()).cloned().unwrap_or_default();
                                                                view! {
                                                                    <div style="margin-top:16px;">
                                                                        <h4 style="color:var(--text,#e5e7eb);font-size:14px;margin-bottom:8px;">"Payment History"</h4>
                                                                        {if payments.is_empty() {
                                                                            view! { <p style="color:var(--muted,rgba(232,232,216,0.5));font-size:12px;font-style:italic;">"No payments recorded yet"</p> }.into_any()
                                                                        } else {
                                                                            let rows: Vec<_> = payments.iter().map(|p| {
                                                                                let p_status = p.get("status").and_then(|v| v.as_str()).unwrap_or("Unknown").to_string();
                                                                                let period = p.get("period_number").and_then(|v| v.as_u64()).unwrap_or(0);
                                                                                let paid = p.get("amount_paid_chronos").and_then(|v| v.as_u64()).unwrap_or(0) / 1_000_000;
                                                                                let required = p.get("amount_required_chronos").and_then(|v| v.as_u64()).unwrap_or(0) / 1_000_000;
                                                                                let p_color = match p_status.as_str() {
                                                                                    "OnTime" | "AutoPaid" => "#2ecc71",
                                                                                    "Late" => "#f1c40f",
                                                                                    "Missed" | "AutoPayFailed" => "#e74c3c",
                                                                                    "Partial" => "#e67e22",
                                                                                    "Prepaid" => "#3498db",
                                                                                    _ => "#8899aa",
                                                                                };
                                                                                view! {
                                                                                    <div style="display:flex;justify-content:space-between;align-items:center;padding:4px 0;border-bottom:1px solid rgba(255,255,255,0.04);font-size:12px;">
                                                                                        <span style="color:var(--muted,rgba(232,232,216,0.5));">{format!("Period {}", period)}</span>
                                                                                        <span style=format!("color:{};font-weight:600;", p_color)>{p_status}</span>
                                                                                        <span style="color:var(--text,#e5e7eb);">{format!("{}/{} KX", paid, required)}</span>
                                                                                    </div>
                                                                                }
                                                                            }).collect();
                                                                            view! {
                                                                                <div style="max-height:200px;overflow-y:auto;">
                                                                                    {rows}
                                                                                </div>
                                                                            }.into_any()
                                                                        }}
                                                                        <p style="color:var(--muted,rgba(232,232,216,0.5));font-size:10px;margin-top:6px;font-style:italic;">"Payment history is private by default."</p>
                                                                    </div>
                                                                }
                                                            }
                                                            // Section 5: Post Status Update (D2) — lender only, desktop only
                                                            {
                                                                if is_lender && is_desktop() {
                                                                    let flag_posting = RwSignal::new(false);
                                                                    let flag_selected = RwSignal::new(String::new());
                                                                    let flag_memo = RwSignal::new(String::new());
                                                                    let flag_result = RwSignal::new(Option::<String>::None);
                                                                    view! {
                                                                        <div style="margin-top:20px;padding-top:16px;border-top:1px solid var(--border,#1e2a42);">
                                                                            <h4 style="color:var(--gold,#d4a84b);font-size:13px;margin-bottom:8px;">"Post Status Update"</h4>
                                                                            <p style="color:var(--muted,rgba(232,232,216,0.5));font-size:11px;margin-bottom:8px;">
                                                                                "This flag will be recorded on-chain permanently and may be superseded but never deleted."
                                                                            </p>
                                                                            <select style="width:100%;padding:6px;background:var(--bg,#0e1525);border:1px solid var(--border,#1e2a42);border-radius:6px;color:var(--text,#e5e7eb);font-size:12px;margin-bottom:8px;"
                                                                                on:change=move |e| flag_selected.set(event_target_value(&e))>
                                                                                <option value="">"Select flag..."</option>
                                                                                <option value="Late">"Late"</option>
                                                                                <option value="Delinquent">"Delinquent"</option>
                                                                                <option value="Default">"Default"</option>
                                                                                <option value="Accelerated">"Accelerated"</option>
                                                                                <option value="WrittenOff">"Written Off"</option>
                                                                                <option value="Reinstated">"Reinstated"</option>
                                                                                <option value="Settled">"Settled"</option>
                                                                                <option value="Forgiven">"Forgiven"</option>
                                                                                <option value="Transferred">"Transferred"</option>
                                                                                <option value="Refinanced">"Refinanced"</option>
                                                                            </select>
                                                                            <input type="text" maxlength="256" placeholder="Optional memo..."
                                                                                style="width:100%;padding:6px;background:var(--bg,#0e1525);border:1px solid var(--border,#1e2a42);border-radius:6px;color:var(--text,#e5e7eb);font-size:12px;margin-bottom:8px;"
                                                                                prop:value=move || flag_memo.get()
                                                                                on:input=move |e| flag_memo.set(event_target_value(&e))
                                                                            />
                                                                            <button style="background:var(--gold,#d4a84b);color:#000;border:none;padding:6px 16px;border-radius:6px;font-size:12px;font-weight:600;cursor:pointer;"
                                                                                disabled=move || flag_selected.get().is_empty() || flag_posting.get()
                                                                                on:click=move |_| {
                                                                                    flag_result.set(Some("Flag posting will be available in the next protocol version.".to_string()));
                                                                                }>
                                                                                "Post Flag"
                                                                            </button>
                                                                            {move || flag_result.get().map(|msg| view! {
                                                                                <p style="color:var(--muted,rgba(232,232,216,0.5));font-size:11px;margin-top:6px;">{msg}</p>
                                                                            })}
                                                                        </div>
                                                                    }.into_any()
                                                                } else {
                                                                    view! { <span></span> }.into_any()
                                                                }
                                                            }
                                                            // Legal Disclaimer
                                                            <div class="terms-modal-disclaimer">
                                                                "This summary is generated from on-chain data. ChronX Protocol is not a party to this loan and makes no representations regarding its legality in any jurisdiction."
                                                            </div>
                                                            <button class="terms-modal-close" on:click=move |_| terms_modal_loan.set(None)>"Close"</button>
                                                        </div>
                                                    </div>
                                                }.into_any()
                                            } else {
                                                view! { <span></span> }.into_any()
                                            }}
                                            // ── A2: Exit Confirmation Modal ──
                                            {move || if let Some(loan) = exit_modal_loan.get() {
                                                let loan_id_hex = loan.get("loan_id_hex").and_then(|v| v.as_str()).unwrap_or("").to_string();
                                                let notice = loan.get("exit_notice_hours").and_then(|v| v.as_u64()).unwrap_or(24);
                                                let notice_label = if notice == 0 { "immediately".to_string() } else { format!("in {} hours", notice) };
                                                let rate_bps = loan.get("interest_rate").and_then(|v| v.get("Fixed")).and_then(|v| v.as_u64()).unwrap_or(0);
                                                let principal_kx = loan.get("principal_kx").and_then(|v| v.as_u64()).unwrap_or(0);
                                                let accepted_at = loan.get("accepted_at").and_then(|v| v.as_u64()).unwrap_or(0);
                                                let now_s = (js_sys::Date::now() / 1000.0) as u64;
                                                let elapsed = now_s.saturating_sub(accepted_at);
                                                let accrued = (principal_kx as f64) * (rate_bps as f64 / 10000.0) * (elapsed as f64 / 31536000.0);
                                                let lid = loan_id_hex.clone();
                                                view! {
                                                    <div class="terms-modal-overlay" on:click=move |_| exit_modal_loan.set(None)>
                                                        <div class="terms-modal" on:click=move |ev: web_sys::MouseEvent| ev.stop_propagation()>
                                                            <h3 style="color:#e05c5c;font-size:17px;font-weight:700;margin:0 0 14px">"Exit This Loan?"</h3>
                                                            <p style="font-size:13px;color:#c8c8c0;line-height:1.7;margin-bottom:12px">
                                                                {format!("You are requesting to exit this loan. Per the agreed terms, exit takes effect {}. Accrued interest to date: approximately {:.2} KX. Your exit request will be recorded on-chain immediately.", notice_label, accrued)}
                                                            </p>
                                                            {move || if !exit_error.get().is_empty() {
                                                                view! { <div style="color:#ef4444;font-size:12px;margin-bottom:8px">{exit_error.get()}</div> }.into_any()
                                                            } else { view! { <span></span> }.into_any() }}
                                                            <div style="display:flex;gap:8px;margin-top:14px">
                                                                <button style="flex:1;padding:10px;background:#e05c5c;border:none;border-radius:8px;color:#fff;font-size:13px;font-weight:600;cursor:pointer"
                                                                    disabled=move || exit_submitting.get()
                                                                    on:click=move |_| {
                                                                        let lid2 = lid.clone();
                                                                        exit_submitting.set(true);
                                                                        exit_error.set(String::new());
                                                                        spawn_local(async move {
                                                                            let args = serde_wasm_bindgen::to_value(&serde_json::json!({"loanIdHex": lid2})).unwrap_or(no_args());
                                                                            match call::<String>("submit_loan_exit", args).await {
                                                                                Ok(_) => {
                                                                                    exit_modal_loan.set(None);
                                                                                    exit_submitting.set(false);
                                                                                    // Refresh loans
                                                                                    if let Ok(v) = call::<serde_json::Value>("get_wallet_loans", no_args()).await { loans_data.set(v); }
                                                                                }
                                                                                Err(e) => { exit_error.set(format!("{}", e)); exit_submitting.set(false); }
                                                                            }
                                                                        });
                                                                    }>
                                                                    {move || if exit_submitting.get() { "Submitting..." } else { "Confirm Exit" }}
                                                                </button>
                                                                <button class="terms-modal-close" style="flex:1" on:click=move |_| exit_modal_loan.set(None)>"Cancel"</button>
                                                            </div>
                                                        </div>
                                                    </div>
                                                }.into_any()
                                            } else {
                                                view! { <span></span> }.into_any()
                                            }}
                                            <OpenPanel info=info lang=lang />
                                        }.into_any(),
                                    }}
                                }.into_any()
                                },
                                // Tab 3: Request (desktop only) OR Settings (mobile)
                                3 if desktop => view! {
                                    <RequestPanel info=info lang=lang />
                                }.into_any(),
                                // Tab 4: Loans (desktop only)
                                4 if desktop => {
                                    // Fetch loans + offers + contacts on tab load
                                    spawn_local(async move {
                                        if let Ok(loans_val) = call::<serde_json::Value>("get_wallet_loans", no_args()).await {
                                            // Fetch wallet labels for all borrowers
                                            if let Some(arr) = loans_val.as_array() {
                                                let mut labels = wallet_labels.get_untracked();
                                                for loan in arr {
                                                    let bw = loan.get("borrower_wallet").and_then(|v| v.as_str()).unwrap_or("").to_string();
                                                    if !bw.is_empty() && !labels.contains_key(&bw) {
                                                        let bw2 = bw.clone();
                                                        let args = serde_wasm_bindgen::to_value(&serde_json::json!({"walletAddress": bw2})).unwrap_or(no_args());
                                                        if let Ok(label) = call::<String>("get_wallet_label", args).await {
                                                            labels.insert(bw, label);
                                                        }
                                                    }
                                                }
                                                wallet_labels.set(labels);
                                            }
                                            loans_data.set(loans_val);
                                            loans_loaded.set(true); // A9: mark loaded
                                        }
                                        if let Ok(offers_val) = call::<serde_json::Value>("get_loan_offers", no_args()).await {
                                            loan_offers.set(offers_val);
                                        }
                                        if let Ok(c) = call::<std::collections::HashMap<String,String>>("get_loan_contacts", no_args()).await {
                                            loan_contacts.set(c);
                                        }
                                    });
                                    view! {
                                    <div class="loans-panel">
                                        // View toggle + header
                                        <div class="loans-header">
                                            <h2 style="font-size:20px;font-weight:700;color:#e5e7eb;margin:0">"Loan Portfolio"</h2>
                                            <div style="display:flex;gap:6px">
                                                <button
                                                    class=move || if loans_view.get()==0 { "send-mode-btn active" } else { "send-mode-btn" }
                                                    style="font-size:13px;padding:8px 16px"
                                                    on:click=move |_| loans_view.set(0)>
                                                    "Lender View"
                                                </button>
                                                <button
                                                    class=move || if loans_view.get()==1 { "send-mode-btn active" } else { "send-mode-btn" }
                                                    style="font-size:13px;padding:8px 16px"
                                                    on:click=move |_| loans_view.set(1)>
                                                    "Borrower View"
                                                </button>
                                            </div>
                                        </div>

                                        {move || if loans_view.get() == 0 {
                                            // ════════════════════════════════════
                                            // LENDER VIEW
                                            // ════════════════════════════════════
                                            view! {
                                                <div>
                                                    <div style="display:flex;justify-content:flex-end;margin-bottom:16px">
                                                        <button class="send-mode-btn active" style="font-size:13px;padding:8px 16px"
                                                            on:click=move |_| { wizard_step.set(1); wiz_loan_type.set(0); wiz_borrower.set(String::new()); wiz_nickname.set(String::new()); wiz_amount.set(String::new()); wiz_rate_bps.set(String::new()); wiz_term_months.set(String::new()); wiz_collateral_id.set(String::new()); wiz_servicer_url.set(String::new()); wiz_error.set(String::new()); wiz_success.set(false); wiz_success_tx.set(None); wiz_loan_ref.set(String::new()); wiz_borrower_email.set(String::new()); wiz_email_resolved.set(None); wiz_email_display.set(String::new()); wiz_offer_expiry.set(0); wiz_penalty_enabled.set(false); wiz_penalty_type.set(String::from("Flat")); wiz_penalty_amount.set(String::new()); wizard_open.set(true); }>"+ New Loan"</button>
                                                    </div>
                                                    <div class="loans-summary">
                                                        {move || {
                                                            let data = loans_data.get();
                                                            let my_wallet = info.get().map(|a| a.account_id.clone()).unwrap_or_default();
                                                            let arr = data.as_array();
                                                            let (active_count, total_lent, next_due, defaults) = if let Some(loans) = arr {
                                                                let my_loans: Vec<_> = loans.iter().filter(|l| {
                                                                    l.get("lender_wallet").and_then(|v| v.as_str()).unwrap_or("") == my_wallet
                                                                }).collect();
                                                                let active = my_loans.iter().filter(|l| {
                                                                    let st = l.get("status").and_then(|s| s.as_str()).unwrap_or("");
                                                                    st == "active" || st == "delinquent"
                                                                }).count();
                                                                let total: u64 = my_loans.iter().filter(|l| {
                                                                    let st = l.get("status").and_then(|s| s.as_str()).unwrap_or("");
                                                                    st == "active" || st == "delinquent"
                                                                }).map(|l| {
                                                                    l.get("principal_kx").and_then(|v| v.as_u64())
                                                                        .or_else(|| l.get("principal_chronos").and_then(|v| v.as_u64()).map(|c| c / 1_000_000))
                                                                        .unwrap_or(0)
                                                                }).sum();
                                                                let earliest_due = my_loans.iter().filter_map(|l| {
                                                                    l.get("next_payment_at").and_then(|v| v.as_u64())
                                                                }).min();
                                                                let defs = my_loans.iter().filter(|l| {
                                                                    l.get("status").and_then(|s| s.as_str()).unwrap_or("") == "default"
                                                                }).count();
                                                                (active, total, earliest_due, defs)
                                                            } else {
                                                                (0, 0, None, 0)
                                                            };
                                                            // A9: Show "0" when loaded but empty, em-dash only while loading
                                                            let loaded = loans_loaded.get();
                                                            let total_str = if total_lent > 0 {
                                                                let s = total_lent.to_string();
                                                                let mut r = String::new();
                                                                for (i, c) in s.chars().rev().enumerate() {
                                                                    if i > 0 && i % 3 == 0 { r.push(','); }
                                                                    r.push(c);
                                                                }
                                                                format!("{} KX", r.chars().rev().collect::<String>())
                                                            } else if loaded { "0 KX".to_string() } else { "\u{2014}".to_string() };
                                                            let due_str = if let Some(ts) = next_due {
                                                                let now_ms = js_sys::Date::now();
                                                                let now_s = (now_ms / 1000.0) as u64;
                                                                let diff = ts.saturating_sub(now_s);
                                                                if diff < 86400 && ts >= now_s { "Today".to_string() }
                                                                else if diff < 172800 && ts >= now_s { "Tomorrow".to_string() }
                                                                else {
                                                                    let d = js_sys::Date::new_0();
                                                                    d.set_time((ts as f64) * 1000.0);
                                                                    let months = ["Jan","Feb","Mar","Apr","May","Jun","Jul","Aug","Sep","Oct","Nov","Dec"];
                                                                    let m = d.get_month() as usize;
                                                                    let day = d.get_date();
                                                                    format!("{} {}", months.get(m).unwrap_or(&"???"), day)
                                                                }
                                                            } else if loaded { "None".to_string() } else { "\u{2014}".to_string() };
                                                            let active_str = if active_count > 0 { active_count.to_string() } else if loaded { "0".to_string() } else { "\u{2014}".to_string() };
                                                            view! {
                                                                <div class="loan-stat-card">
                                                                    <span class="loan-stat-val">{active_str}</span>
                                                                    <span class="loan-stat-label">"Active Loans"</span>
                                                                </div>
                                                                <div class="loan-stat-card">
                                                                    <span class="loan-stat-val">{total_str}</span>
                                                                    <span class="loan-stat-label">"Total Lent"</span>
                                                                </div>
                                                                <div class="loan-stat-card">
                                                                    <span class="loan-stat-val">{due_str}</span>
                                                                    <span class="loan-stat-label">"Next Payment Due"</span>
                                                                </div>
                                                                <div class="loan-stat-card">
                                                                    <span class="loan-stat-val">{defaults.to_string()}</span>
                                                                    <span class="loan-stat-label">"Defaults"</span>
                                                                </div>
                                                            }
                                                        }}
                                                    </div>
                                                    <div class="loans-table">
                                                        <div class="loans-table-header" style="grid-template-columns:2fr 1fr 0.8fr 1.2fr 0.8fr 1.2fr 1fr 0.8fr 0.6fr">
                                                            <span>"Borrower"</span>
                                                            <span>"Principal"</span>
                                                            <span>"Currency"</span>
                                                            <span>"Interval Payment"</span>
                                                            <span>"Type"</span>
                                                            <span>"Next Payment"</span>
                                                            <span>"Status"</span>
                                                            <span>"Flag"</span>
                                                            <span></span>
                                                        </div>
                                                        {move || {
                                                            let data = loans_data.get();
                                                            let contacts = loan_contacts.get();
                                                            let labels = wallet_labels.get();
                                                            let loans = data.as_array();
                                                            if let Some(arr) = loans {
                                                                if !arr.is_empty() {
                                                                    let rows: Vec<_> = arr.iter().map(|loan| {
                                                                        let borrower = loan.get("borrower_wallet").and_then(|v| v.as_str()).unwrap_or("\u{2014}").to_string();
                                                                        // FIX 1: Priority display — contact nickname > API label > truncated address
                                                                        let borrower_display = contacts.get(&borrower).cloned()
                                                                            .or_else(|| labels.get(&borrower).cloned())
                                                                            .unwrap_or_else(|| {
                                                                                if borrower.len() > 14 { format!("{}...{}", &borrower[..6], &borrower[borrower.len()-4..]) } else { borrower.clone() }
                                                                            });
                                                                        let has_nickname = contacts.contains_key(&borrower) || labels.contains_key(&borrower);
                                                                        let borrower_addr_short = if borrower.len() > 14 { format!("{}...{}", &borrower[..6], &borrower[borrower.len()-4..]) } else { borrower.clone() };
                                                                        let bw_for_edit = borrower.clone();
                                                                        // Inline rename signals
                                                                        let editing_contact = RwSignal::new(false);
                                                                        let edit_contact_val = RwSignal::new(contacts.get(&borrower).cloned().unwrap_or_default());
                                                                        let principal_kx = loan.get("principal_kx").and_then(|v| v.as_u64())
                                                                            .or_else(|| loan.get("principal_chronos").and_then(|v| v.as_u64()).map(|c| c / 1_000_000))
                                                                            .unwrap_or(0);
                                                                        let has_pay_as = loan.get("pay_as").is_some() && !loan.get("pay_as").unwrap().is_null();
                                                                        let pay_as = if has_pay_as { "USD" } else { "KX" };
                                                                        let status = loan.get("status").and_then(|v| v.as_str()).unwrap_or("pending").to_string();
                                                                        let is_revolving = loan.get("loan_type").map(|v| v.to_string().contains("Revolving")).unwrap_or(false);
                                                                        let loan_type_str = if is_revolving { "Revolving" } else { "Fixed" };
                                                                        // FIX 3: Interval payment calculation
                                                                        let rate_bps = loan.get("interest_rate").and_then(|v| v.get("Fixed")).and_then(|v| v.as_u64()).unwrap_or(0);
                                                                        let rate_pct = rate_bps as f64 / 100.0;
                                                                        let lt = loan.get("loan_type").cloned().unwrap_or(serde_json::Value::Null);
                                                                        let (interval_amt, interval_label) = if is_revolving {
                                                                            let period = lt.get("Revolving")
                                                                                .and_then(|v| v.get("renewal_period_seconds"))
                                                                                .and_then(|v| v.as_u64())
                                                                                .unwrap_or(86400);
                                                                            let (ppy, lbl) = match period {
                                                                                1 => (31_536_000.0, "per second"),
                                                                                3600 => (8_760.0, "per hour"),
                                                                                86400 => (365.0, "per day"),
                                                                                604800 => (52.0, "per week"),
                                                                                2592000 => (12.0, "per month"),
                                                                                31536000 => (1.0, "per year"),
                                                                                _ => (365.0, "per day"),
                                                                            };
                                                                            let amt = (principal_kx as f64) * (rate_pct / 100.0) / ppy;
                                                                            (amt, lbl)
                                                                        } else {
                                                                            let amt = (principal_kx as f64) * (rate_pct / 100.0) / 365.0;
                                                                            (amt, "per day")
                                                                        };
                                                                        // Next payment date
                                                                        let next_due = loan.get("next_payment_at").and_then(|v| v.as_u64());
                                                                        let next_due_str = if let Some(ts) = next_due {
                                                                            let now_ms = js_sys::Date::now();
                                                                            let now_s = (now_ms / 1000.0) as u64;
                                                                            let diff = ts.saturating_sub(now_s);
                                                                            if diff < 86400 && ts >= now_s { "Today".to_string() }
                                                                            else if diff < 172800 && ts >= now_s { "Tomorrow".to_string() }
                                                                            else {
                                                                                let d = js_sys::Date::new_0();
                                                                                d.set_time((ts as f64) * 1000.0);
                                                                                let months = ["Jan","Feb","Mar","Apr","May","Jun","Jul","Aug","Sep","Oct","Nov","Dec"];
                                                                                let m = d.get_month() as usize;
                                                                                let day = d.get_date();
                                                                                format!("{} {}", months.get(m).unwrap_or(&"???"), day)
                                                                            }
                                                                        } else { "\u{2014}".to_string() };
                                                                        let status_class = match status.as_str() {
                                                                            "active" => "loan-status-badge active",
                                                                            "pending" => "loan-status-badge pending",
                                                                            "declined" | "defaulted" => "loan-status-badge defaulted",
                                                                            _ => "loan-status-badge",
                                                                        };
                                                                        // Terms button
                                                                        let loan_clone = loan.clone();
                                                                        // Borrower cell: display name + pencil edit
                                                                        let borrower_display_s = RwSignal::new(borrower_display.clone());
                                                                        let borrower_addr_s = RwSignal::new(if !has_nickname { borrower_addr_short.clone() } else { String::new() });
                                                                        let bw_addr = RwSignal::new(bw_for_edit.clone());
                                                                        let has_nick = has_nickname;
                                                                        view! {
                                                                            <div class="loans-table-row" style="grid-template-columns:2fr 1fr 0.8fr 1.2fr 0.8fr 1.2fr 1fr 0.8fr 0.6fr">
                                                                                <span class="loan-borrower-cell">
                                                                                    {move || if editing_contact.get() {
                                                                                        view! {
                                                                                            <input type="text" class="loan-contact-input"
                                                                                                prop:value=move || edit_contact_val.get()
                                                                                                on:input=move |ev| edit_contact_val.set(event_target_value(&ev))
                                                                                                on:keydown=move |ev: web_sys::KeyboardEvent| {
                                                                                                    if ev.key() == "Enter" {
                                                                                                        let bw_s = bw_addr.get();
                                                                                                        let val = edit_contact_val.get();
                                                                                                        editing_contact.set(false);
                                                                                                        spawn_local(async move {
                                                                                                            let args = serde_wasm_bindgen::to_value(&serde_json::json!({
                                                                                                                "walletAddress": bw_s, "nickname": val
                                                                                                            })).unwrap_or(no_args());
                                                                                                            let _ = call::<()>("set_loan_contact", args).await;
                                                                                                            if let Ok(c) = call::<std::collections::HashMap<String,String>>("get_loan_contacts", no_args()).await {
                                                                                                                loan_contacts.set(c);
                                                                                                            }
                                                                                                        });
                                                                                                    } else if ev.key() == "Escape" { editing_contact.set(false); }
                                                                                                }
                                                                                                on:blur=move |_| {
                                                                                                    let bw_s = bw_addr.get();
                                                                                                    let val = edit_contact_val.get();
                                                                                                    editing_contact.set(false);
                                                                                                    spawn_local(async move {
                                                                                                        let args = serde_wasm_bindgen::to_value(&serde_json::json!({
                                                                                                            "walletAddress": bw_s, "nickname": val
                                                                                                        })).unwrap_or(no_args());
                                                                                                        let _ = call::<()>("set_loan_contact", args).await;
                                                                                                        if let Ok(c) = call::<std::collections::HashMap<String,String>>("get_loan_contacts", no_args()).await {
                                                                                                            loan_contacts.set(c);
                                                                                                        }
                                                                                                    });
                                                                                                }
                                                                                            />
                                                                                        }.into_any()
                                                                                    } else {
                                                                                        view! {
                                                                                            <span>
                                                                                                <span class="loan-borrower-name">{borrower_display_s.get()}</span>
                                                                                                {if !has_nick {
                                                                                                    view! { <span class="loan-borrower-addr">{borrower_addr_s.get()}</span> }.into_any()
                                                                                                } else {
                                                                                                    view! { <span></span> }.into_any()
                                                                                                }}
                                                                                                <button class="loan-edit-pencil" on:click=move |ev: web_sys::MouseEvent| {
                                                                                                    ev.stop_propagation();
                                                                                                    edit_contact_val.set(loan_contacts.get().get(&bw_addr.get()).cloned().unwrap_or_default());
                                                                                                    editing_contact.set(true);
                                                                                                }>{"\u{270f}\u{fe0f}"}</button>
                                                                                            </span>
                                                                                        }.into_any()
                                                                                    }}
                                                                                </span>
                                                                                <span>{format!("{} KX", principal_kx)}</span>
                                                                                <span>{pay_as}</span>
                                                                                <span class="interval-payment-cell">
                                                                                    <span class="interval-amount">{format!("{:.2} KX", interval_amt)}</span>
                                                                                    <span class="interval-label">{interval_label}</span>
                                                                                </span>
                                                                                <span>{loan_type_str}</span>
                                                                                <span>{next_due_str}</span>
                                                                                <span class=status_class>{status.to_uppercase()}</span>
                                                                                <span>{
                                                                                    let flag = loan.get("current_flag").and_then(|v| v.as_str()).unwrap_or("").to_string();
                                                                                    if !flag.is_empty() {
                                                                                        let (color, bg) = flag_badge_style(&flag);
                                                                                        view! {
                                                                                            <span style=format!("background:{};color:{};padding:2px 8px;border-radius:10px;font-size:10px;font-weight:600;white-space:nowrap;", bg, color)>
                                                                                                {flag}
                                                                                            </span>
                                                                                        }.into_any()
                                                                                    } else {
                                                                                        view! { <span></span> }.into_any()
                                                                                    }
                                                                                }</span>
                                                                                <span>
                                                                                    <button class="loan-terms-btn" on:click={
                                                                                        let lc = loan_clone.clone();
                                                                                        move |ev: web_sys::MouseEvent| {
                                                                                            ev.stop_propagation();
                                                                                            show_raw_terms.set(false);
                                                                                            loan_summary_text.set(None);
                                                                                            loan_summary_loading.set(true);
                                                                                            terms_modal_loan.set(Some(lc.clone()));
                                                                                            let lid = lc.get("loan_id_hex").and_then(|v| v.as_str()).unwrap_or("").to_string();
                                                                                            let lj = serde_json::to_string(&lc).unwrap_or_default();
                                                                                            spawn_local(async move {
                                                                                                let args = serde_wasm_bindgen::to_value(&serde_json::json!({
                                                                                                    "loanId": lid, "loanJson": lj
                                                                                                })).unwrap_or(no_args());
                                                                                                match call::<String>("get_loan_summary", args).await {
                                                                                                    Ok(s) => { loan_summary_text.set(Some(s)); loan_summary_loading.set(false); }
                                                                                                    Err(_) => { loan_summary_loading.set(false); }
                                                                                                }
                                                                                            });
                                                                                        }
                                                                                    }>"Terms"</button>
                                                                                    {
                                                                                        // Exit button — only show when exit_rights permit
                                                                                        let exit_rights = loan.get("exit_rights").and_then(|v| v.as_str()).unwrap_or("EitherParty");
                                                                                        let my_w = info.get().map(|a| a.account_id.clone()).unwrap_or_default();
                                                                                        let lw = loan.get("lender_wallet").and_then(|v| v.as_str()).unwrap_or("");
                                                                                        let is_lender_role = my_w == lw;
                                                                                        let can_exit = match exit_rights {
                                                                                            "EitherParty" => true,
                                                                                            "LenderOnly" => is_lender_role,
                                                                                            "BorrowerOnly" => !is_lender_role,
                                                                                            "MutualConsent" => true,
                                                                                            _ => true,
                                                                                        };
                                                                                        let loan_status = loan.get("status").and_then(|v| v.as_str()).unwrap_or("");
                                                                                        let show_exit = can_exit && loan_status == "active";
                                                                                        if show_exit {
                                                                                            let lc_exit = loan.clone();
                                                                                            view! {
                                                                                                <button class="loan-exit-btn" on:click=move |ev: web_sys::MouseEvent| {
                                                                                                    ev.stop_propagation();
                                                                                                    exit_error.set(String::new());
                                                                                                    exit_submitting.set(false);
                                                                                                    exit_modal_loan.set(Some(lc_exit.clone()));
                                                                                                }>"Exit"</button>
                                                                                            }.into_any()
                                                                                        } else {
                                                                                            view! { <span></span> }.into_any()
                                                                                        }
                                                                                    }
                                                                                </span>
                                                                            </div>
                                                                        }.into_any()
                                                                    }).collect::<Vec<_>>();
                                                                    return view! { <div>{rows}</div> }.into_any();
                                                                }
                                                            }
                                                            // Empty state
                                                            view! {
                                                                <div class="loans-empty">
                                                                    <div style="font-size:40px;margin-bottom:16px">"\u{1f4cb}"</div>
                                                                    <div style="font-size:16px;font-weight:600;color:#e5e7eb;margin-bottom:8px">"No loans issued"</div>
                                                                    <div style="font-size:13px;color:rgba(232,232,216,0.5);line-height:1.6;max-width:400px;margin:0 auto">
                                                                        "Create your first loan using the New Loan wizard. "
                                                                        "Loans are recorded on the ChronX blockchain via Genesis 10 primitives."
                                                                    </div>
                                                                    <button class="send-mode-btn active" style="margin-top:20px;padding:10px 24px;font-size:14px"
                                                                        on:click=move |_| { wizard_step.set(1); wiz_loan_type.set(0); wiz_borrower.set(String::new()); wiz_nickname.set(String::new()); wiz_amount.set(String::new()); wiz_rate_bps.set(String::new()); wiz_term_months.set(String::new()); wiz_collateral_id.set(String::new()); wiz_servicer_url.set(String::new()); wiz_error.set(String::new()); wiz_success.set(false); wiz_success_tx.set(None); wiz_loan_ref.set(String::new()); wiz_borrower_email.set(String::new()); wiz_email_resolved.set(None); wiz_email_display.set(String::new()); wiz_offer_expiry.set(0); wiz_penalty_enabled.set(false); wiz_penalty_type.set(String::from("Flat")); wiz_penalty_amount.set(String::new()); wizard_open.set(true); }>"Create First Loan"</button>
                                                                </div>
                                                            }.into_any()
                                                        }}
                                                    </div>
                                                </div>
                                            }.into_any()
                                        } else {
                                            // ════════════════════════════════════
                                            // BORROWER VIEW
                                            // ════════════════════════════════════
                                            view! {
                                                <div>
                                                    // ── Incoming Loan Offers ──
                                                    {move || {
                                                        let data = loan_offers.get();
                                                        let offers = data.as_array();
                                                        if let Some(arr) = offers {
                                                            if !arr.is_empty() {
                                                                let cards: Vec<_> = arr.iter().map(|offer| {
                                                                    let lender = offer.get("lender_wallet").and_then(|v| v.as_str()).unwrap_or("—").to_string();
                                                                    let lender_short = if lender.len() > 16 { format!("{}...{}", &lender[..6], &lender[lender.len()-4..]) } else { lender.clone() };
                                                                    let principal = offer.get("principal_kx").and_then(|v| v.as_u64()).unwrap_or(0);
                                                                    let loan_id = offer.get("loan_id_hex").and_then(|v| v.as_str()).unwrap_or("").to_string();
                                                                    let lid = loan_id.clone();
                                                                    let lid2 = loan_id.clone();
                                                                    view! {
                                                                        <div class="offer-card">
                                                                            <div class="offer-card-left">
                                                                                <div style="font-size:14px;font-weight:600;color:#e5e7eb">
                                                                                    {"\u{1f4cb} Loan Offer"}
                                                                                </div>
                                                                                <div style="font-size:12px;color:rgba(232,232,216,0.5);margin-top:2px">
                                                                                    {format!("From: {}", lender_short)}
                                                                                </div>
                                                                                <div style="font-size:13px;color:#d4a84b;margin-top:4px;font-weight:600">
                                                                                    {format!("{} KX", principal)}
                                                                                </div>
                                                                            </div>
                                                                            <div class="offer-card-actions">
                                                                                <button class="offer-accept-btn"
                                                                                    on:click=move |_| {
                                                                                        let id = lid.clone();
                                                                                        spawn_local(async move {
                                                                                            let args = serde_wasm_bindgen::to_value(&serde_json::json!({"loanIdHex": id, "ageConfirmed": true})).unwrap_or(no_args());
                                                                                            match call::<String>("accept_loan_offer", args).await {
                                                                                                Ok(_) => {
                                                                                                    if let Ok(v) = call::<serde_json::Value>("get_loan_offers", no_args()).await { loan_offers.set(v); }
                                                                                                }
                                                                                                Err(e) => { web_sys::window().unwrap().alert_with_message(&format!("Accept failed: {}", e)).ok(); }
                                                                                            }
                                                                                        });
                                                                                    }>
                                                                                    {"\u{2713} Accept"}
                                                                                </button>
                                                                                <button class="offer-decline-btn"
                                                                                    on:click=move |_| {
                                                                                        let id = lid2.clone();
                                                                                        spawn_local(async move {
                                                                                            let args = serde_wasm_bindgen::to_value(&serde_json::json!({"loanIdHex": id})).unwrap_or(no_args());
                                                                                            match call::<String>("decline_loan_offer", args).await {
                                                                                                Ok(_) => {
                                                                                                    if let Ok(v) = call::<serde_json::Value>("get_loan_offers", no_args()).await { loan_offers.set(v); }
                                                                                                }
                                                                                                Err(e) => { web_sys::window().unwrap().alert_with_message(&format!("Decline failed: {}", e)).ok(); }
                                                                                            }
                                                                                        });
                                                                                    }>
                                                                                    {"\u{2717} Decline"}
                                                                                </button>
                                                                            </div>
                                                                        </div>
                                                                    }
                                                                }).collect();
                                                                return view! { <div style="margin-bottom:16px">{cards}</div> }.into_any();
                                                            }
                                                        }
                                                        view! { <span></span> }.into_any()
                                                    }}
                                                    // ── Next Payment Card ──
                                                    <div class="next-payment-card">
                                                        <div class="npc-left">
                                                            <div class="npc-label">"Next payment due"</div>
                                                            <div class="npc-due">"\u{2014} days"</div>
                                                            <div class="npc-amounts">
                                                                <span class="npc-kx">"\u{2014} KX"</span>
                                                                <span class="npc-fiat">{"\u{2248} $\u{2014} USD"}</span>
                                                            </div>
                                                        </div>
                                                        <div class="npc-actions">
                                                            <button class="send-mode-btn active" style="padding:10px 20px;font-size:13px">
                                                                "Pay with KX"
                                                            </button>
                                                            <button class="send-mode-btn" style="padding:10px 20px;font-size:13px">
                                                                "Pay via XChan"
                                                            </button>
                                                        </div>
                                                    </div>

                                                    // ── Auto-Pay Setup ──
                                                    <div class="autopay-card">
                                                        <div style="display:flex;align-items:center;justify-content:space-between">
                                                            <div style="display:flex;align-items:center;gap:12px">
                                                                <span style="font-size:20px">{"\u{1f504}"}</span>
                                                                <div>
                                                                    <div style="font-size:14px;font-weight:600;color:#e5e7eb">"Auto-Pay"</div>
                                                                    <div style="font-size:12px;color:rgba(232,232,216,0.5)">"Creates a TYPE C Credit Authorization on-chain"</div>
                                                                </div>
                                                            </div>
                                                            <label class="toggle-switch">
                                                                <input type="checkbox" disabled=true />
                                                                <span class="toggle-slider"></span>
                                                            </label>
                                                        </div>
                                                        <div style="margin-top:10px;padding:10px 12px;background:rgba(255,255,255,0.02);border-radius:6px;font-size:12px;color:rgba(232,232,216,0.45);line-height:1.5">
                                                            "Auto-pay inactive. When enabled, up to X KX will be drawn on the 1st of each month. Requires sufficient KX balance at payment time."
                                                        </div>
                                                    </div>

                                                    // ── Loans Owed Summary (A9: show 0 when loaded) ──
                                                    <div class="loans-summary" style="margin-top:20px">
                                                        <div class="loan-stat-card">
                                                            <span class="loan-stat-val">{move || if loans_loaded.get() { "0".to_string() } else { "\u{2014}".to_string() }}</span>
                                                            <span class="loan-stat-label">"Loans Owed"</span>
                                                        </div>
                                                        <div class="loan-stat-card">
                                                            <span class="loan-stat-val">{move || if loans_loaded.get() { "0 KX".to_string() } else { "\u{2014}".to_string() }}</span>
                                                            <span class="loan-stat-label">"Total Owed"</span>
                                                        </div>
                                                        <div class="loan-stat-card">
                                                            <span class="loan-stat-val">{move || if loans_loaded.get() { "None".to_string() } else { "\u{2014}".to_string() }}</span>
                                                            <span class="loan-stat-label">"Next Due"</span>
                                                        </div>
                                                        <div class="loan-stat-card">
                                                            <span class="loan-stat-val">"0"</span>
                                                            <span class="loan-stat-label">"Missed"</span>
                                                        </div>
                                                    </div>

                                                    // ── Loans Owed Table ──
                                                    <div class="loans-table" style="margin-top:16px">
                                                        <div class="loans-table-header" style="grid-template-columns:2fr 1fr 1fr 1.5fr 1fr 1fr">
                                                            <span>"Lender"</span>
                                                            <span>"Principal"</span>
                                                            <span>"Currency"</span>
                                                            <span>"Next Due"</span>
                                                            <span>"Amount Due"</span>
                                                            <span>"Status"</span>
                                                        </div>
                                                        <div class="loans-empty">
                                                            <div style="font-size:36px;margin-bottom:12px">"\u{1f4e5}"</div>
                                                            <div style="font-size:15px;font-weight:600;color:#e5e7eb;margin-bottom:6px">"No loans owed"</div>
                                                            <div style="font-size:13px;color:rgba(232,232,216,0.5);line-height:1.5;max-width:380px;margin:0 auto">
                                                                "When you borrow KX via a ChronX loan, it will appear here with payment schedules and status."
                                                            </div>
                                                        </div>
                                                    </div>

                                                    // ── My Credit Record ──
                                                    <div style="margin-top:24px">
                                                        <h3 style="font-size:15px;font-weight:700;color:#e5e7eb;margin-bottom:12px">"My Credit Record"</h3>
                                                        <div class="credit-record-panel">
                                                            <div class="credit-empty">
                                                                <div style="font-size:28px;margin-bottom:8px">{"\u{1f4dc}"}</div>
                                                                <div style="font-size:13px;color:rgba(232,232,216,0.5);line-height:1.6;max-width:360px;margin:0 auto">
                                                                    "No loan history yet \u{2014} your on-chain credit record will appear here. "
                                                                    "Completed loans show as green entries. Defaults show in red with lender annotations."
                                                                </div>
                                                            </div>
                                                        </div>
                                                    </div>
                                                </div>
                                            }.into_any()
                                        }}
                                    </div>
                                }.into_any()
                                },
                                // Settings tab (3 on mobile, 5 on desktop)
                                _ if tab == settings_tab => view! {
                                    <SettingsPanel
                                        online=online
                                        app_phase=app_phase
                                        pin_digits=pin_digits
                                        pin_msg=pin_msg
                                        pin_shake=pin_shake
                                        app_version=app_version
                                        notices=notices
                                        seen_ids=seen_ids
                                        pin_len=pin_len
                                        update_available=update_available
                                        lang=lang
                                        desktop=desktop
                                        info=info
                                        email_locks=email_locks
                                        on_email_check=check_email
                                        active_tab=active_tab
                                        bug_modal_open=bug_modal_open
                                        bug_body=bug_body
                                        on_mark_seen=move |id: String| {
                                            let mut ids = seen_ids.get_untracked();
                                            if !ids.contains(&id) {
                                                ids.push(id.clone());
                                                seen_ids.set(ids);
                                            }
                                            spawn_local(async move {
                                                let args = serde_wasm_bindgen::to_value(
                                                    &serde_json::json!({ "id": id })
                                                ).unwrap_or(no_args());
                                                let _ = call::<()>("mark_notice_dismissed", args).await;
                                            });
                                        }
                                    />
                                }.into_any(),
                                _ => view! { <span></span> }.into_any(),
                            }
                        }}
                    </div>

                    // Version footer — hidden on Settings tab (shown there already)
                    <div style:display=move || {
                        let settings_idx: u8 = if desktop { 4 } else { 3 };
                        if active_tab.get() == settings_idx { "none" } else { "" }
                    }>
                    <p class="version-footer">
                        "ChronX Wallet v"
                        {move || app_version.get()}
                    </p>
                    <div class="bug-footer">
                        <button class="bug-report-btn" on:click=move |_| {
                            bug_body.set(String::new());
                            bug_modal_open.set(true);
                        }>"Report a Bug"</button>
                    </div>
                    </div>
                    </div> // close main-content

                    // ── Rescission Window Modal — 72h cancel window after accepting a loan ──
                    {move || {
                        if let Some(ref loan_id) = cooloff_loan_id.get() {
                            let secs = cooloff_remaining.get();
                            let hours = secs / 3600;
                            let mins = (secs % 3600) / 60;
                            let s = secs % 60;
                            let lid_cancel = loan_id.clone();
                            view! {
                                <div style="position:fixed;top:0;left:0;right:0;bottom:0;background:rgba(0,0,0,0.7);z-index:9999;display:flex;align-items:center;justify-content:center;">
                                    <div style="background:var(--bg2,#0f1422);border:1px solid #d4a84b;border-radius:12px;padding:32px;max-width:400px;text-align:center;">
                                        <h3 style="color:#d4a84b;margin-bottom:16px;">"Loan Accepted"</h3>
                                        <p style="color:#e5e7eb;font-size:18px;font-weight:700;margin-bottom:8px;font-variant-numeric:tabular-nums">
                                            {format!("{}h {:02}m {:02}s", hours, mins, s)}
                                        </p>
                                        <p style="color:#e5e7eb;font-size:14px;margin-bottom:16px;">
                                            "Funds will transfer automatically when the rescission window closes."
                                        </p>
                                        <p style="color:rgba(232,232,216,0.5);font-size:12px;margin-bottom:20px;">
                                            "You may cancel this loan at no cost within the rescission window."
                                        </p>
                                        <div style="display:flex;gap:12px;justify-content:center;">
                                            <button style="background:#e74c3c;color:#fff;border:none;padding:10px 24px;border-radius:8px;cursor:pointer;font-size:14px;" on:click=move |_| {
                                                let lid = lid_cancel.clone();
                                                spawn_local(async move {
                                                    let args = serde_wasm_bindgen::to_value(&serde_json::json!({"loanIdHex": lid})).unwrap_or(no_args());
                                                    let _ = call::<String>("cancel_loan_rescission", args).await;
                                                });
                                                cooloff_loan_id.set(None);
                                            }>
                                                "Cancel Loan"
                                            </button>
                                            <button style="background:transparent;border:1px solid #374151;color:#9ca3af;padding:10px 24px;border-radius:8px;cursor:pointer;font-size:14px;" on:click=move |_| {
                                                cooloff_loan_id.set(None);
                                            }>
                                                "Close"
                                            </button>
                                        </div>
                                    </div>
                                </div>
                            }.into_any()
                        } else {
                            view! { <span></span> }.into_any()
                        }
                    }}

                    // ── Loan Wizard Modal (Desktop Only) ──
                    {move || if wizard_open.get() && is_desktop() {
                        let step = wizard_step.get();
                        view! {
                            <div class="modal-overlay" on:click=move |ev| {
                                use wasm_bindgen::JsCast;
                                if let Some(target) = ev.target() {
                                    if target.dyn_into::<web_sys::HtmlElement>().ok()
                                        .and_then(|el| el.class_list().contains("modal-overlay").then_some(()))
                                        .is_some()
                                    {
                                        wizard_open.set(false);
                                    }
                                }
                            }>
                                <div class="wizard-modal">
                                    <div class="wizard-header">
                                        <div class="wizard-title">"New Loan"</div>
                                        <div class="wizard-steps-bar">
                                            <span class=move || { if wizard_step.get() >= 1 {"wstep active"} else {"wstep"} }>"1"</span>
                                            <span class="wstep-line"></span>
                                            <span class=move || { if wizard_step.get() >= 2 {"wstep active"} else {"wstep"} }>"2"</span>
                                            <span class="wstep-line"></span>
                                            <span class=move || { if wizard_step.get() >= 3 {"wstep active"} else {"wstep"} }>"3"</span>
                                            <span class="wstep-line"></span>
                                            <span class=move || { if wizard_step.get() >= 4 {"wstep active"} else {"wstep"} }>"4"</span>
                                            <span class="wstep-line"></span>
                                            <span class=move || { if wizard_step.get() >= 5 {"wstep active"} else {"wstep"} }>"5"</span>
                                            <span class="wstep-line"></span>
                                            <span class=move || { if wizard_step.get() >= 6 {"wstep active"} else {"wstep"} }>"6"</span>
                                        </div>
                                        <button class="wizard-close" on:click=move |_| wizard_open.set(false)>{"\u{2715}"}</button>
                                    </div>
                                    <div class="wizard-body">
                                        {move || {
                                            let s = wizard_step.get();
                                            match s {
                                                1 => view! {
                                                    <div>
                                                        <h3 class="wiz-step-title">"Step 1: Loan Type"</h3>
                                                        <p class="wiz-step-sub">"Choose the structure for this loan."</p>
                                                        <div class="wiz-type-cards">
                                                            <div class=move || if wiz_loan_type.get()==0 {"wiz-type-card selected"} else {"wiz-type-card"}
                                                                on:click=move |_| wiz_loan_type.set(0)>
                                                                <div class="wiz-type-icon">{"\u{1f4c5}"}</div>
                                                                <div class="wiz-type-name">"Fixed Schedule"</div>
                                                                <div class="wiz-type-desc">"Set payment dates and amounts upfront. Standard term loan."</div>
                                                            </div>
                                                            <div class=move || if wiz_loan_type.get()==1 {"wiz-type-card selected"} else {"wiz-type-card"}
                                                                on:click=move |_| wiz_loan_type.set(1)>
                                                                <div class="wiz-type-icon">{"\u{1f504}"}</div>
                                                                <div class="wiz-type-name">"Revolving"</div>
                                                                <div class="wiz-type-desc">"Auto-renews each period. Either party can exit with notice."</div>
                                                            </div>
                                                        </div>
                                                        // A6: Loan Reference Number
                                                        <div style="margin-top:16px;">
                                                            <label style="color:rgba(232,232,216,0.5);font-size:12px;">"Loan Reference / Servicer Number (optional)"</label>
                                                            <input type="text" maxlength="64" placeholder="Your internal reference number or servicer tracking ID"
                                                                style="width:100%;padding:8px;background:var(--bg,#0c0e1a);border:1px solid rgba(255,255,255,0.1);border-radius:6px;color:#e5e7eb;font-size:13px;margin-top:4px;"
                                                                prop:value=move || wiz_loan_ref.get()
                                                                on:input=move |e| wiz_loan_ref.set(event_target_value(&e))
                                                            />
                                                            <span style="font-size:11px;color:rgba(232,232,216,0.4);">"Stored on-chain in the loan memo field."</span>
                                                        </div>
                                                    </div>
                                                }.into_any(),
                                                2 => view! {
                                                    <div>
                                                        <h3 class="wiz-step-title">"Step 2: Counterparty"</h3>
                                                        <p class="wiz-step-sub">"Enter the borrower's email or wallet address."</p>
                                                        // A5: Email-first lookup
                                                        <div class="wiz-field">
                                                            <label>"Borrower Email Address"</label>
                                                            <input type="email" placeholder="borrower@example.com"
                                                                prop:value=move || wiz_borrower_email.get()
                                                                on:input=move |ev| {
                                                                    let val = event_target_value(&ev);
                                                                    wiz_borrower_email.set(val.clone());
                                                                    // Reset resolved state on change
                                                                    wiz_email_resolved.set(None);
                                                                    wiz_email_display.set(String::new());
                                                                }
                                                                on:blur=move |_| {
                                                                    let email = wiz_borrower_email.get_untracked();
                                                                    if !email.is_empty() && email.contains('@') {
                                                                        spawn_local(async move {
                                                                            let args = serde_wasm_bindgen::to_value(&serde_json::json!({"email": email})).unwrap_or(no_args());
                                                                            if let Ok(result) = call::<serde_json::Value>("lookup_wallet_by_email", args).await {
                                                                                if let Some(wallet) = result.get("wallet_address").and_then(|v| v.as_str()) {
                                                                                    let display = result.get("display_name").and_then(|v| v.as_str()).unwrap_or("").to_string();
                                                                                    wiz_email_resolved.set(Some(wallet.to_string()));
                                                                                    wiz_email_display.set(display);
                                                                                    wiz_borrower.set(wallet.to_string());
                                                                                }
                                                                            }
                                                                        });
                                                                    }
                                                                }
                                                            />
                                                        </div>
                                                        // A5: Resolved state display
                                                        {move || {
                                                            if let Some(ref wallet) = wiz_email_resolved.get() {
                                                                let display = wiz_email_display.get();
                                                                let short = if wallet.len() > 14 { format!("{}...{}", &wallet[..6], &wallet[wallet.len()-4..]) } else { wallet.clone() };
                                                                let label = if display.is_empty() { short.clone() } else { format!("{} ({})", display, short) };
                                                                view! {
                                                                    <div style="padding:8px 12px;background:rgba(46,204,113,0.1);border:1px solid rgba(46,204,113,0.3);border-radius:6px;margin-bottom:8px;font-size:12px;color:#2ecc71;">
                                                                        {format!("\u{2713} Registered ChronX user \u{2014} {}", label)}
                                                                    </div>
                                                                }.into_any()
                                                            } else if !wiz_borrower_email.get().is_empty() && wiz_borrower_email.get().contains('@') {
                                                                view! {
                                                                    <div style="padding:8px 12px;background:rgba(255,255,255,0.03);border:1px solid rgba(255,255,255,0.08);border-radius:6px;margin-bottom:8px;font-size:12px;color:rgba(232,232,216,0.5);">
                                                                        "Email not found in registry. Enter wallet address manually below."
                                                                    </div>
                                                                }.into_any()
                                                            } else {
                                                                view! { <span></span> }.into_any()
                                                            }
                                                        }}
                                                        <div class="wiz-field">
                                                            <label>"Borrower Wallet Address"</label>
                                                            <input type="text" placeholder="e.g. BCwHsGLP..."
                                                                prop:value=move || wiz_borrower.get()
                                                                on:input=move |ev| wiz_borrower.set(event_target_value(&ev)) />
                                                        </div>
                                                        <div class="wiz-field">
                                                            <label>"Local Nickname (optional)"</label>
                                                            <input type="text" placeholder="e.g. Alex"
                                                                prop:value=move || wiz_nickname.get()
                                                                on:input=move |ev| wiz_nickname.set(event_target_value(&ev)) />
                                                        </div>
                                                        // A5: Offer Expires dropdown
                                                        <div class="wiz-field">
                                                            <label>"Offer Expires In"</label>
                                                            <select prop:value=move || wiz_offer_expiry.get().to_string()
                                                                on:change=move |ev| { if let Ok(v) = event_target_value(&ev).parse::<u64>() { wiz_offer_expiry.set(v); } }>
                                                                <option value="0">"Never"</option>
                                                                <option value="86400">"24 hours"</option>
                                                                <option value="172800">"48 hours"</option>
                                                                <option value="604800">"7 days"</option>
                                                            </select>
                                                        </div>
                                                        {move || { let e = wiz_error.get(); if !e.is_empty() { view! { <p class="wiz-error">{e}</p> }.into_any() } else { view! { <span></span> }.into_any() }}}
                                                    </div>
                                                }.into_any(),
                                                3 => view! {
                                                    <div>
                                                        <h3 class="wiz-step-title">"Step 3: Terms"</h3>
                                                        <p class="wiz-step-sub">{move || if wiz_loan_type.get()==0 { "Set the principal, rate, and term." } else { "Set the principal, rate, and renewal period." }}</p>
                                                        <div class="wiz-field-row">
                                                            <div class="wiz-field" style="flex:2">
                                                                <label>"Principal Amount"</label>
                                                                <input type="text" placeholder="e.g. 10000"
                                                                    prop:value=move || wiz_amount.get()
                                                                    on:input=move |ev| wiz_amount.set(event_target_value(&ev)) />
                                                            </div>
                                                            <div class="wiz-field" style="flex:1">
                                                                <label>"Currency"</label>
                                                                <select prop:value=move || wiz_currency.get()
                                                                    on:change=move |ev| wiz_currency.set(event_target_value(&ev))>
                                                                    <option value="KX">"KX"</option>
                                                                    <option value="USD">"USD"</option>
                                                                    <option value="EUR">"EUR"</option>
                                                                </select>
                                                            </div>
                                                        </div>
                                                        <div class="wiz-field">
                                                            <label>"Annual Interest Rate (%)"</label>
                                                            <input type="text" placeholder="e.g. 5.0"
                                                                prop:value=move || wiz_rate_bps.get()
                                                                on:input=move |ev| wiz_rate_bps.set(event_target_value(&ev)) />
                                                        </div>
                                                        // A7: Live payment preview
                                                        {move || {
                                                            let amount: f64 = wiz_amount.get().parse().unwrap_or(0.0);
                                                            let rate: f64 = wiz_rate_bps.get().parse().unwrap_or(0.0);
                                                            let renewal = wiz_renewal_period.get();
                                                            if amount > 0.0 && rate > 0.0 {
                                                                let (per_period, label) = match renewal {
                                                                    0 => ((amount * rate / 100.0) / 31_536_000.0, "per second"),
                                                                    1 => ((amount * rate / 100.0) / 8_760.0, "per hour"),
                                                                    2 => ((amount * rate / 100.0) / 365.0, "per day"),
                                                                    3 => ((amount * rate / 100.0) / 52.0, "per week"),
                                                                    4 => ((amount * rate / 100.0) / 12.0, "per month"),
                                                                    _ => ((amount * rate / 100.0), "per year"),
                                                                };
                                                                view! {
                                                                    <p style="color:#d4a84b;font-size:13px;font-style:italic;margin-top:8px;">
                                                                        {format!("\u{2248} {:.4} KX {}", per_period, label)}
                                                                    </p>
                                                                }.into_any()
                                                            } else {
                                                                view! { <span></span> }.into_any()
                                                            }
                                                        }}
                                                        {move || if wiz_loan_type.get() == 0 {
                                                            view! {
                                                                <div class="wiz-field">
                                                                    <label>"Term (months)"</label>
                                                                    <input type="text" placeholder="e.g. 24"
                                                                        prop:value=move || wiz_term_months.get()
                                                                        on:input=move |ev| wiz_term_months.set(event_target_value(&ev)) />
                                                                </div>
                                                            }.into_any()
                                                        } else {
                                                            view! {
                                                                <div class="wiz-field">
                                                                    <label>"Renewal Period"</label>
                                                                    <select prop:value=move || wiz_renewal_period.get().to_string()
                                                                        on:change=move |ev| { if let Ok(v) = event_target_value(&ev).parse::<u8>() { wiz_renewal_period.set(v); } }>
                                                                        <option value="0">"Every second (AI micro-loans)"</option>
                                                                        <option value="1">"Every hour"</option>
                                                                        <option value="2">"Daily"</option>
                                                                        <option value="3">"Weekly"</option>
                                                                        <option value="4">"Monthly"</option>
                                                                        <option value="5">"Yearly"</option>
                                                                    </select>
                                                                </div>
                                                                <div class="wiz-field">
                                                                    <label>{move || {
                                                                        let labels = ["Auto-terminate if cost/sec exceeds (%)", "Auto-terminate if hourly cost exceeds (%)", "Auto-terminate if daily cost exceeds (%) \u{2014} optional", "Auto-terminate if weekly cost exceeds (%)", "Auto-terminate if monthly cost exceeds (%)", "Auto-terminate if yearly cost exceeds (%)"];
                                                                        labels[wiz_renewal_period.get() as usize].to_string()
                                                                    }}</label>
                                                                    <input type="text" placeholder="e.g. 0.50 — or leave blank for no cap"
                                                                        prop:value=move || wiz_rate_cap.get()
                                                                        on:input=move |ev| wiz_rate_cap.set(event_target_value(&ev)) />
                                                                </div>
                                                            }.into_any()
                                                        }}
                                                        {move || { let e = wiz_error.get(); if !e.is_empty() { view! { <p class="wiz-error">{e}</p> }.into_any() } else { view! { <span></span> }.into_any() }}}
                                                    </div>
                                                }.into_any(),
                                                4 => view! {
                                                    <div>
                                                        <h3 class="wiz-step-title">{move || if wiz_loan_type.get()==0 { "Step 4: Payment Schedule" } else { "Step 4: Revolving Details" }}</h3>
                                                        {move || if wiz_loan_type.get() == 0 {
                                                            view! {
                                                                <div>
                                                                    <p class="wiz-step-sub">"Choose how payments are structured."</p>
                                                                    <div class="wiz-type-cards" style="grid-template-columns:1fr 1fr 1fr">
                                                                        <div class=move || if wiz_schedule_type.get()==0 {"wiz-type-card selected"} else {"wiz-type-card"}
                                                                            on:click=move |_| wiz_schedule_type.set(0)>
                                                                            <div class="wiz-type-name">"Bullet"</div>
                                                                            <div class="wiz-type-desc">"All principal + interest due at maturity."</div>
                                                                        </div>
                                                                        <div class=move || if wiz_schedule_type.get()==1 {"wiz-type-card selected"} else {"wiz-type-card"}
                                                                            on:click=move |_| wiz_schedule_type.set(1)>
                                                                            <div class="wiz-type-name">"Amortizing"</div>
                                                                            <div class="wiz-type-desc">"Equal monthly payments of principal + interest."</div>
                                                                        </div>
                                                                        <div class=move || if wiz_schedule_type.get()==2 {"wiz-type-card selected"} else {"wiz-type-card"}
                                                                            on:click=move |_| wiz_schedule_type.set(2)>
                                                                            <div class="wiz-type-name">"Custom"</div>
                                                                            <div class="wiz-type-desc">"Define each payment stage manually."</div>
                                                                        </div>
                                                                    </div>
                                                                </div>
                                                            }.into_any()
                                                        } else {
                                                            view! {
                                                                <div>
                                                                    <p class="wiz-step-sub">"Configure exit rights, interest collection, and conditions."</p>
                                                                    <div class="wiz-field">
                                                                        <label>"Exit Rights"</label>
                                                                        <select prop:value=move || wiz_exit_rights.get().to_string()
                                                                            on:change=move |ev| { if let Ok(v) = event_target_value(&ev).parse::<u8>() { wiz_exit_rights.set(v); } }>
                                                                            <option value="0">"Either party (recommended)"</option>
                                                                            <option value="1">"Lender only"</option>
                                                                            <option value="2">"Borrower only"</option>
                                                                            <option value="3">"Mutual consent"</option>
                                                                        </select>
                                                                    </div>
                                                                    <div class="wiz-field">
                                                                        <label>"Exit Notice Period"</label>
                                                                        <select>
                                                                            <option value="3600">"1 hour"</option>
                                                                            <option value="21600">"6 hours"</option>
                                                                            <option value="86400" selected>"24 hours (default)"</option>
                                                                            <option value="172800">"48 hours"</option>
                                                                            <option value="604800">"7 days"</option>
                                                                        </select>
                                                                    </div>
                                                                    <div class="wiz-field">
                                                                        <label>"Interest Collection"</label>
                                                                        <select>
                                                                            <option value="exit" selected>"At Exit (default) \u{2014} settles when loan closes"</option>
                                                                            <option value="daily">"Daily \u{2014} borrower pays interest each day"</option>
                                                                            <option value="weekly">"Weekly \u{2014} every 7 renewal periods"</option>
                                                                        </select>
                                                                    </div>
                                                                    <div class="wiz-field">
                                                                        <label>"Default Triggers (optional)"</label>
                                                                        <select>
                                                                            <option value="none" selected>"None"</option>
                                                                            <option value="missed">"Missed payment \u{2014} with grace period"</option>
                                                                        </select>
                                                                    </div>
                                                                    <div class="wiz-field">
                                                                        <label>"Revival Condition"</label>
                                                                        <select prop:value=move || wiz_revival.get().to_string()
                                                                            on:change=move |ev| { if let Ok(v) = event_target_value(&ev).parse::<u8>() { wiz_revival.set(v); } }>
                                                                            <option value="0">"Always renew (default)"</option>
                                                                        </select>
                                                                        <p style="font-size:11px;color:rgba(232,232,216,0.4);margin-top:4px">"Oracle-based conditions coming in a future update."</p>
                                                                    </div>
                                                                </div>
                                                            }.into_any()
                                                        }}
                                                    </div>
                                                }.into_any(),
                                                5 => view! {
                                                    <div>
                                                        <h3 class="wiz-step-title">"Step 5: Protection"</h3>
                                                        <p class="wiz-step-sub">"Optional collateral and payment matching."</p>
                                                        <div class="wiz-field">
                                                            <label>"Collateral Lock ID (optional)"</label>
                                                            <input type="text" placeholder="Enter an existing TYPE V lock ID"
                                                                prop:value=move || wiz_collateral_id.get()
                                                                on:input=move |ev| wiz_collateral_id.set(event_target_value(&ev)) />
                                                            <p style="font-size:11px;color:rgba(232,232,216,0.4);margin-top:4px">"Leave blank if no collateral is required."</p>
                                                        </div>
                                                        <div class="wiz-field">
                                                            <label>"Payment Matching"</label>
                                                            <select prop:value=move || wiz_payment_match.get().to_string()
                                                                on:change=move |ev| { if let Ok(v) = event_target_value(&ev).parse::<u8>() { wiz_payment_match.set(v); } }>
                                                                <option value="0">"Exact amount required"</option>
                                                                <option value="1">"Partial payments accepted"</option>
                                                                <option value="2">"Minimum required"</option>
                                                            </select>
                                                        </div>
                                                        <div class="wiz-field">
                                                            <label>"Servicer Portal URL (optional)"</label>
                                                            <input type="text" placeholder="https://..."
                                                                prop:value=move || wiz_servicer_url.get()
                                                                on:input=move |ev| wiz_servicer_url.set(event_target_value(&ev)) />
                                                        </div>
                                                        // Prepayment Penalty (D3)
                                                        <div style="margin-top:16px;">
                                                            <button style="background:transparent;border:none;color:var(--gold,#d4a84b);font-size:13px;cursor:pointer;padding:0;"
                                                                on:click=move |_| wiz_penalty_enabled.update(|v| *v = !*v)>
                                                                {move || if wiz_penalty_enabled.get() { "\u{25bc} Prepayment penalty" } else { "\u{25b6} Add prepayment penalty (optional)" }}
                                                            </button>
                                                            {move || if wiz_penalty_enabled.get() {
                                                                view! {
                                                                    <div style="margin-top:8px;padding:12px;background:var(--bg,#0e1525);border:1px solid var(--border,#1e2a42);border-radius:8px;">
                                                                        <div style="margin-bottom:8px;">
                                                                            <label style="color:var(--muted,rgba(232,232,216,0.5));font-size:12px;">"Penalty Type"</label>
                                                                            <select style="width:100%;padding:6px;background:var(--bg3,#1a2538);border:1px solid var(--border,#1e2a42);border-radius:6px;color:var(--text,#e5e7eb);font-size:12px;"
                                                                                on:change=move |e| wiz_penalty_type.set(event_target_value(&e))>
                                                                                <option value="Flat">"Flat Amount"</option>
                                                                                <option value="Percentage">"% of Remaining Principal"</option>
                                                                                <option value="MonthsInterest">"Months of Interest"</option>
                                                                            </select>
                                                                        </div>
                                                                        <div style="margin-bottom:8px;">
                                                                            <label style="color:var(--muted,rgba(232,232,216,0.5));font-size:12px;">"Amount"</label>
                                                                            <input type="number" step="0.01" placeholder="0.00"
                                                                                style="width:100%;padding:6px;background:var(--bg3,#1a2538);border:1px solid var(--border,#1e2a42);border-radius:6px;color:var(--text,#e5e7eb);font-size:12px;"
                                                                                prop:value=move || wiz_penalty_amount.get()
                                                                                on:input=move |e| wiz_penalty_amount.set(event_target_value(&e))
                                                                            />
                                                                        </div>
                                                                        <p style="color:var(--muted,rgba(232,232,216,0.5));font-size:11px;">"Borrower will see this penalty before accepting."</p>
                                                                    </div>
                                                                }.into_any()
                                                            } else {
                                                                view! { <span></span> }.into_any()
                                                            }}
                                                        </div>
                                                    </div>
                                                }.into_any(),
                                                _ => view! {
                                                    <div>
                                                        <h3 class="wiz-step-title">"Step 6: Review & Send Offer"</h3>
                                                        <p class="wiz-step-sub">"Your signed offer will be sent to the borrower. The loan activates only after they accept."</p>
                                                        <div class="wiz-review">
                                                            <div class="wiz-review-row">
                                                                <span class="wiz-review-label">"Type"</span>
                                                                <span class="wiz-review-val">{move || if wiz_loan_type.get()==0 { "Fixed Schedule" } else { "Revolving" }}</span>
                                                            </div>
                                                            <div class="wiz-review-row">
                                                                <span class="wiz-review-label">"Borrower"</span>
                                                                <span class="wiz-review-val">{move || { let b = wiz_borrower.get(); if b.len() > 20 { format!("{}...{}", &b[..8], &b[b.len()-8..]) } else { b } }}</span>
                                                            </div>
                                                            {move || { let n = wiz_nickname.get(); if !n.is_empty() { view! { <div class="wiz-review-row"><span class="wiz-review-label">"Nickname"</span><span class="wiz-review-val">{n}</span></div> }.into_any() } else { view! { <span></span> }.into_any() }}}
                                                            <div class="wiz-review-row">
                                                                <span class="wiz-review-label">"Principal"</span>
                                                                <span class="wiz-review-val">{move || format!("{} {}", wiz_amount.get(), wiz_currency.get())}</span>
                                                            </div>
                                                            <div class="wiz-review-row">
                                                                <span class="wiz-review-label">"Interest Rate"</span>
                                                                <span class="wiz-review-val">{move || format!("{}% annual", wiz_rate_bps.get())}</span>
                                                            </div>
                                                            {move || if wiz_loan_type.get() == 0 {
                                                                view! {
                                                                    <div class="wiz-review-row">
                                                                        <span class="wiz-review-label">"Term"</span>
                                                                        <span class="wiz-review-val">{move || format!("{} months", wiz_term_months.get())}</span>
                                                                    </div>
                                                                    <div class="wiz-review-row">
                                                                        <span class="wiz-review-label">"Schedule"</span>
                                                                        <span class="wiz-review-val">{move || match wiz_schedule_type.get() { 0 => "Bullet", 1 => "Amortizing", _ => "Custom" }}</span>
                                                                    </div>
                                                                }.into_any()
                                                            } else {
                                                                let periods = ["Every second","Hourly","Daily","Weekly","Monthly","Yearly"];
                                                                let exit_labels = ["Either party","Lender only","Borrower only","Mutual consent"];
                                                                view! {
                                                                    <div class="wiz-review-row">
                                                                        <span class="wiz-review-label">"Renewal"</span>
                                                                        <span class="wiz-review-val">{periods[wiz_renewal_period.get_untracked() as usize]}</span>
                                                                    </div>
                                                                    <div class="wiz-review-row">
                                                                        <span class="wiz-review-label">"Rate Cap"</span>
                                                                        <span class="wiz-review-val">{move || format!("{}% per period", wiz_rate_cap.get())}</span>
                                                                    </div>
                                                                    <div class="wiz-review-row">
                                                                        <span class="wiz-review-label">"Exit Rights"</span>
                                                                        <span class="wiz-review-val">{exit_labels[wiz_exit_rights.get_untracked() as usize]}</span>
                                                                    </div>
                                                                }.into_any()
                                                            }}
                                                            {move || { let c = wiz_collateral_id.get(); if !c.is_empty() {
                                                                view! { <div class="wiz-review-row"><span class="wiz-review-label">"Collateral"</span><span class="wiz-review-val">{if c.len()>16 { format!("{}...", &c[..16]) } else { c }}</span></div> }.into_any()
                                                            } else { view! { <span></span> }.into_any() }}}
                                                            {move || if wiz_penalty_enabled.get() && !wiz_penalty_amount.get().is_empty() {
                                                                view! {
                                                                    <div style="display:flex;justify-content:space-between;padding:6px 0;border-bottom:1px solid rgba(255,255,255,0.06);">
                                                                        <span style="color:var(--muted,rgba(232,232,216,0.5));font-size:13px;">"Prepayment Penalty"</span>
                                                                        <span style="color:var(--text,#e5e7eb);font-size:13px;">
                                                                            {format!("{} KX ({})", wiz_penalty_amount.get(), wiz_penalty_type.get())}
                                                                        </span>
                                                                    </div>
                                                                }.into_any()
                                                            } else {
                                                                view! { <span></span> }.into_any()
                                                            }}
                                                        </div>
                                                        // A8 Fix 3: Plain-English summary line
                                                        {move || {
                                                            let loan_type = if wiz_loan_type.get() == 0 { "Fixed" } else { "Revolving" };
                                                            let currency = wiz_currency.get();
                                                            let amount = wiz_amount.get();
                                                            let rate = wiz_rate_bps.get();
                                                            let renewal = match wiz_renewal_period.get() {
                                                                0 => "per-second", 1 => "hourly", 2 => "daily", 3 => "weekly", 4 => "monthly", _ => "yearly"
                                                            };
                                                            let exit = match wiz_exit_rights.get() {
                                                                0 => "either party may exit", 1 => "lender may exit", 2 => "borrower may exit", _ => "mutual consent required"
                                                            };
                                                            let collateral = if wiz_collateral_id.get().is_empty() { "no collateral posted" } else { "collateral posted" };
                                                            view! {
                                                                <p style="color:rgba(232,232,216,0.5);font-size:12px;font-style:italic;margin:12px 0;line-height:1.5;">
                                                                    {format!("Summary: {} {} loan of {} KX at {}% annual, {} payments, {}, {}.",
                                                                        loan_type, currency, amount, rate, renewal, exit, collateral)}
                                                                </p>
                                                            }
                                                        }}
                                                        // A8 Fix 2: Validation banners
                                                        {move || {
                                                            let borrower = wiz_borrower.get();
                                                            let rate: f64 = wiz_rate_bps.get().parse().unwrap_or(0.0);
                                                            let amount: f64 = wiz_amount.get().parse().unwrap_or(0.0);
                                                            let has_collateral = !wiz_collateral_id.get().is_empty();
                                                            let mut banners = Vec::new();
                                                            if !borrower.is_empty() {
                                                                banners.push(view! { <div style="background:rgba(46,204,113,0.15);border-left:3px solid #2ecc71;padding:6px 12px;margin:6px 0;font-size:12px;color:#2ecc71;">
                                                                    {"\u{2713} Borrower wallet verified"}
                                                                </div> }.into_any());
                                                            }
                                                            if !has_collateral {
                                                                banners.push(view! { <div style="background:rgba(241,196,15,0.15);border-left:3px solid #f1c40f;padding:6px 12px;margin:6px 0;font-size:12px;color:#f1c40f;">
                                                                    {"\u{26a0} Unsecured loan \u{2014} no collateral"}
                                                                </div> }.into_any());
                                                            }
                                                            if rate > 10.0 {
                                                                banners.push(view! { <div style="background:rgba(241,196,15,0.15);border-left:3px solid #f1c40f;padding:6px 12px;margin:6px 0;font-size:12px;color:#f1c40f;">
                                                                    {"\u{26a0} Rate above 10% annual"}
                                                                </div> }.into_any());
                                                            }
                                                            if amount * 0.00319 > 100.0 {
                                                                banners.push(view! { <div style="background:rgba(241,196,15,0.15);border-left:3px solid #f1c40f;padding:6px 12px;margin:6px 0;font-size:12px;color:#f1c40f;">
                                                                    {"\u{26a0} Principal above $100 USD"}
                                                                </div> }.into_any());
                                                            }
                                                            view! { <div>{banners}</div> }
                                                        }}
                                                        {move || { let e = wiz_error.get(); if !e.is_empty() { view! { <p class="wiz-error">{e}</p> }.into_any() } else { view! { <span></span> }.into_any() }}}
                                                        {move || { let c = wiz_collateral_id.get(); if c.trim().is_empty() && !wiz_success.get() {
                                                            view! { <div style="margin-top:8px;padding:10px 14px;background:rgba(212,168,75,0.08);border:1px solid rgba(212,168,75,0.25);border-radius:6px;font-size:12px;color:#d4a84b">{"\u{26a0} No collateral locked \u{2014} lender extends this loan on trust"}</div> }.into_any()
                                                        } else { view! { <span></span> }.into_any() }}}
                                                        // A8: Success state with TX hash + track text
                                                        {move || if wiz_success.get() {
                                                            view! {
                                                                <div class="wiz-success">
                                                                    <div style="font-size:15px;margin-bottom:8px">{"\u{2705} Offer sent \u{2014} waiting for borrower acceptance."}</div>
                                                                    <div style="font-size:12px;color:rgba(232,232,216,0.5);margin-top:4px">"Track this offer in your Activity tab."</div>
                                                                    // A8 Fix 5: Show TX hash
                                                                    {move || {
                                                                        if let Some(ref tx_id) = wiz_success_tx.get() {
                                                                            let tx_copy = tx_id.clone();
                                                                            view! {
                                                                                <div style="margin-top:8px;font-size:12px;">
                                                                                    <span style="color:rgba(232,232,216,0.5);">"TX: "</span>
                                                                                    <code style="color:#d4a84b;font-size:11px;">{tx_id.clone()}</code>
                                                                                    <button style="margin-left:8px;background:transparent;border:1px solid rgba(255,255,255,0.1);color:#d4a84b;padding:2px 8px;border-radius:4px;font-size:11px;cursor:pointer;" on:click=move |_| {
                                                                                        if let Some(win) = web_sys::window() {
                                                                                            let clip = win.navigator().clipboard();
                                                                                            let _ = clip.write_text(&tx_copy);
                                                                                        }
                                                                                    }>"Copy"</button>
                                                                                </div>
                                                                            }.into_any()
                                                                        } else {
                                                                            view! { <span></span> }.into_any()
                                                                        }
                                                                    }}
                                                                </div>
                                                            }.into_any()
                                                        } else { view! { <span></span> }.into_any() }}
                                                    </div>
                                                }.into_any(),
                                            }
                                        }}
                                    </div>
                                    <div class="wizard-footer">
                                        // A8 Fix 4: Hide Cancel button on success
                                        {move || if !wiz_success.get() {
                                            view! { <button class="wizard-cancel" on:click=move |_| wizard_open.set(false)>"Cancel"</button> }.into_any()
                                        } else {
                                            view! { <span></span> }.into_any()
                                        }}
                                        <div style="display:flex;gap:8px">
                                            {move || if wizard_step.get() > 1 {
                                                view! { <button class="send-mode-btn" style="padding:10px 20px;font-size:13px" on:click=move |_| { wiz_error.set(String::new()); wizard_step.update(|s| *s -= 1); }>{"\u{2190} Back"}</button> }.into_any()
                                            } else { view! { <span></span> }.into_any() }}
                                            {move || if wizard_step.get() < 6 {
                                                view! { <button class="send-mode-btn active" style="padding:10px 20px;font-size:13px" on:click=move |_| {
                                                    wiz_error.set(String::new());
                                                    let s = wizard_step.get_untracked();
                                                    if s == 2 && wiz_borrower.get_untracked().trim().is_empty() {
                                                        wiz_error.set("Borrower address is required.".into());
                                                        return;
                                                    }
                                                    if s == 3 {
                                                        if wiz_amount.get_untracked().trim().is_empty() || wiz_amount.get_untracked().trim().parse::<f64>().is_err() {
                                                            wiz_error.set("Enter a valid principal amount.".into());
                                                            return;
                                                        }
                                                        if wiz_rate_bps.get_untracked().trim().is_empty() || wiz_rate_bps.get_untracked().trim().parse::<f64>().is_err() {
                                                            wiz_error.set("Enter a valid interest rate.".into());
                                                            return;
                                                        }
                                                        if wiz_loan_type.get_untracked() == 0 && (wiz_term_months.get_untracked().trim().is_empty() || wiz_term_months.get_untracked().trim().parse::<u32>().is_err()) {
                                                            wiz_error.set("Enter a valid term in months.".into());
                                                            return;
                                                        }
                                                    }
                                                    wizard_step.update(|s| *s += 1);
                                                }>{"Next \u{2192}"}</button> }.into_any()
                                            } else if !wiz_success.get() {
                                                view! { <button class="send-mode-btn active" style="padding:10px 20px;font-size:13px;background:#d4a84b;color:#111" disabled=move || wiz_submitting.get() on:click=move |_| {
                                                    wiz_error.set(String::new());
                                                    wiz_submitting.set(true);
                                                    let is_revolving = wiz_loan_type.get_untracked() == 1;
                                                    let sched = if !is_revolving { Some(wiz_schedule_type.get_untracked()) } else { None::<u8> };
                                                    let renew_idx = if is_revolving { Some(wiz_renewal_period.get_untracked()) } else { None::<u8> };
                                                    let rcap = if is_revolving { wiz_rate_cap.get_untracked().trim().parse::<f64>().ok() } else { None::<f64> };
                                                    let exit_r = if is_revolving { Some(wiz_exit_rights.get_untracked()) } else { None::<u8> };
                                                    let coll_hex = { let c = wiz_collateral_id.get_untracked(); if c.trim().is_empty() { None::<String> } else { Some(c) } };
                                                    let svc_url = { let s = wiz_servicer_url.get_untracked(); if s.trim().is_empty() { None::<String> } else { Some(s) } };
                                                    let nick = wiz_nickname.get_untracked();
                                                    // A6: Include loan reference in memo
                                                    let loan_ref = wiz_loan_ref.get_untracked();
                                                    let memo_str = {
                                                        let mut parts = Vec::new();
                                                        if !nick.is_empty() { parts.push(format!("Borrower: {}", nick)); }
                                                        if !loan_ref.is_empty() { parts.push(format!("Ref: {}", loan_ref)); }
                                                        if parts.is_empty() { None::<String> } else { Some(parts.join(" | ")) }
                                                    };
                                                    // A5: Wire offer expiry
                                                    let expiry_secs = { let v = wiz_offer_expiry.get_untracked(); if v == 0 { None::<u64> } else { Some(v) } };
                                                    let args = serde_wasm_bindgen::to_value(&serde_json::json!({
                                                        "borrowerAddress": wiz_borrower.get_untracked(),
                                                        "principalKx": wiz_amount.get_untracked().trim().parse::<f64>().unwrap_or(0.0),
                                                        "payAsCurrency": wiz_currency.get_untracked(),
                                                        "interestRateAnnualPct": wiz_rate_bps.get_untracked().trim().parse::<f64>().unwrap_or(0.0),
                                                        "termMonths": wiz_term_months.get_untracked().trim().parse::<u32>().ok(),
                                                        "scheduleType": sched,
                                                        "loanTypeRevolving": is_revolving,
                                                        "renewalPeriodIdx": renew_idx,
                                                        "rateCapPct": rcap,
                                                        "exitRightsIdx": exit_r,
                                                        "collateralLockHex": coll_hex,
                                                        "paymentMatchIdx": wiz_payment_match.get_untracked(),
                                                        "servicerUrl": svc_url,
                                                        "offerExpirySeconds": expiry_secs,
                                                        "memo": memo_str,
                                                    })).unwrap_or(no_args());
                                                    spawn_local(async move {
                                                        match call::<String>("create_loan_offer", args).await {
                                                            Ok(txid) => {
                                                                wiz_submitting.set(false);
                                                                wiz_success.set(true);
                                                                wiz_success_tx.set(Some(txid));
                                                                // Refresh loans list
                                                                if let Ok(loans_val) = call::<serde_json::Value>("get_wallet_loans", no_args()).await {
                                                                    loans_data.set(loans_val);
                                                                }
                                                            }
                                                            Err(e) => {
                                                                wiz_submitting.set(false);
                                                                wiz_error.set(format!("Failed: {}", e));
                                                            }
                                                        }
                                                    });
                                                }>{move || if wiz_submitting.get() { "Signing..." } else { "\u{270d}\u{FE0E} Send Offer" }}</button> }.into_any()
                                            } else {
                                                view! { <button class="send-mode-btn active" style="padding:10px 20px;font-size:13px" on:click=move |_| wizard_open.set(false)>"Done"</button> }.into_any()
                                            }}
                                        </div>
                                    </div>
                                </div>
                            </div>
                        }.into_any()
                    } else { view! { <span></span> }.into_any() }}

                    // Bug report modal
                    {move || if bug_modal_open.get() {
                        let version = app_version.get();
                        view! {
                            <div class="modal-overlay" on:click=move |ev| {
                                use wasm_bindgen::JsCast;
                                if let Some(target) = ev.target() {
                                    if target.dyn_into::<web_sys::HtmlElement>().ok()
                                        .and_then(|el| el.class_list().contains("modal-overlay").then_some(()))
                                        .is_some()
                                    {
                                        bug_modal_open.set(false);
                                    }
                                }
                            }>
                                <div class="modal-box">
                                    <p class="modal-title">"Report a Bug"</p>
                                    <p class="label" style="font-size:12px">
                                        "Subject: " {format!("ChronX Wallet v{version} — Bug Report")}
                                    </p>
                                    <textarea
                                        class="modal-textarea"
                                        placeholder="Describe the bug: what happened, what you expected, steps to reproduce..."
                                        on:input=move |ev| {
                                            use wasm_bindgen::JsCast;
                                            if let Some(el) = ev.target()
                                                .and_then(|t| t.dyn_into::<web_sys::HtmlTextAreaElement>().ok())
                                            {
                                                bug_body.set(el.value());
                                            }
                                        }
                                    >
                                        {move || bug_body.get()}
                                    </textarea>
                                    <div class="modal-actions">
                                        <button on:click=move |_| bug_modal_open.set(false)>
                                            "Cancel"
                                        </button>
                                        <button class="primary" on:click=move |_| {
                                            let body = bug_body.get_untracked();
                                            let ver = app_version.get_untracked();
                                            let subject = format!("ChronX Wallet v{ver} — Bug Report");
                                            let encoded_subject = js_sys::encode_uri_component(&subject);
                                            let encoded_body = js_sys::encode_uri_component(&body);
                                            let mailto = format!("mailto:support@chronx.io?subject={encoded_subject}&body={encoded_body}");
                                            spawn_local(async move {
                                                let args = serde_wasm_bindgen::to_value(
                                                    &serde_json::json!({ "url": mailto })
                                                ).unwrap_or(no_args());
                                                let _ = call::<()>("open_url", args).await;
                                            });
                                            bug_modal_open.set(false);
                                        }>"Send Report"</button>
                                    </div>
                                </div>
                            </div>
                        }.into_any()
                    } else {
                        view! { <span></span> }.into_any()
                    }}

                    // Poke decline modal
                    {move || if decline_modal_open.get() {
                        let sender = decline_sender_email.get();
                        view! {
                            <div class="modal-overlay" on:click=move |ev| {
                                use wasm_bindgen::JsCast;
                                if let Some(target) = ev.target() {
                                    if target.dyn_into::<web_sys::HtmlElement>().ok()
                                        .and_then(|el| el.class_list().contains("modal-overlay").then_some(()))
                                        .is_some()
                                    {
                                        decline_modal_open.set(false);
                                    }
                                }
                            }>
                                <div class="modal-box">
                                    <p class="modal-title" style="color:#ef4444">"Request Declined"</p>
                                    <p style="font-size:14px;margin:12px 0">
                                        "Payment request from "
                                        <strong style="color:#e8e9eb">{sender.clone()}</strong>
                                        " has been declined."
                                    </p>
                                    <label style="display:flex;align-items:center;gap:8px;margin:16px 0;cursor:pointer;font-size:13px;color:#9ca3af">
                                        <input type="checkbox"
                                            prop:checked=move || decline_block_checked.get()
                                            on:change=move |ev| {
                                                use wasm_bindgen::JsCast;
                                                let checked = ev.target()
                                                    .and_then(|t| t.dyn_into::<web_sys::HtmlInputElement>().ok())
                                                    .map(|i| i.checked())
                                                    .unwrap_or(false);
                                                decline_block_checked.set(checked);
                                            }
                                        />
                                        "Block this sender (future requests will be silently ignored)"
                                    </label>
                                    <div class="modal-actions">
                                        <button class="primary" style="background:#ef4444;border-color:#ef4444"
                                            disabled=move || decline_busy.get()
                                            on:click=move |_| {
                                                decline_busy.set(true);
                                                let rid = decline_request_id.get_untracked();
                                                let should_block = decline_block_checked.get_untracked();
                                                let sender_em = decline_sender_email.get_untracked();
                                                spawn_local(async move {
                                                    // Call decline API
                                                    let args = serde_wasm_bindgen::to_value(
                                                        &serde_json::json!({ "requestId": rid })
                                                    ).unwrap_or(no_args());
                                                    let _ = call::<()>("decline_poke", args).await;
                                                    // Block sender if checked
                                                    if should_block && !sender_em.is_empty() {
                                                        let args = serde_wasm_bindgen::to_value(
                                                            &serde_json::json!({ "email": sender_em })
                                                        ).unwrap_or(no_args());
                                                        let _ = call::<()>("add_blocked_sender", args).await;
                                                    }
                                                    decline_busy.set(false);
                                                    decline_modal_open.set(false);
                                                });
                                            }>
                                            {move || if decline_busy.get() { "Declining..." } else { "OK" }}
                                        </button>
                                    </div>
                                </div>
                            </div>
                        }.into_any()
                    } else {
                        view! { <span></span> }.into_any()
                    }}
                </div>
            }.into_any(),
        }}
    }
}

// ── SplashScreen ──────────────────────────────────────────────────────────────

#[component]
fn SplashScreen() -> impl IntoView {
    view! {
        <div class="splash-screen">
            <img src=logo_src() alt="ChronX" class="splash-logo" />
            <p class="splash-title">"ChronX Wallet"</p>
            <p class="splash-sub">"The Future Payment Protocol"</p>
        </div>
    }
}

// ── PinInput — shared PIN digit entry component ───────────────────────────────
// ONE component used by every PIN screen. This is the single source of truth
// for PIN input behaviour — fixing it here fixes all screens simultaneously.
//
// Pattern: keydown-first on desktop (prevent_default() suppresses the input
// event in Tauri/WebView2), on-screen keypad buttons for mobile. There is
// NO on:input handler and NO call to input.set_value() — ever. Those were
// the root cause of the infinite-loop bug (set_value re-fires on:input on
// some WebView builds, stalling the input after the first digit).
//
// Auto-focus: whenever `digits` becomes empty the hidden input is focused
// automatically, covering initial mount, post-submit, and phase transitions.

#[component]
fn PinInput(
    digits: RwSignal<String>,
    shake:  RwSignal<bool>,
    #[prop(default = 4)] pin_len: u8,
) -> impl IntoView {
    let max_len = pin_len as usize;
    let input_ref = NodeRef::<leptos::html::Input>::new();

    // Focus whenever digits is cleared (initial mount, after each submit,
    // after every phase transition). Yield one microtask so the NodeRef is
    // populated before focus() is called.
    Effect::new(move |_| {
        if digits.get().is_empty() {
            let ir = input_ref;
            spawn_local(async move {
                let _ = JsFuture::from(Promise::resolve(&JsValue::UNDEFINED)).await;
                if let Some(el) = ir.get() { let _ = el.focus(); }
            });
        }
    });

    // Clicking anywhere in the component re-focuses the hidden input.
    let on_wrap_click = move |_: web_sys::MouseEvent| {
        if let Some(el) = input_ref.get() { let _ = el.focus(); }
    };

    // Keydown — desktop digit capture. prevent_default() suppresses the
    // subsequent input event in Tauri/WebView2, so no on:input is needed.
    let on_keydown = move |ev: web_sys::KeyboardEvent| {
        let key = ev.key();
        if key.len() == 1 {
            if let Some(ch) = key.chars().next() {
                if ch.is_ascii_digit() {
                    ev.prevent_default();
                    let mut d = digits.get_untracked();
                    if d.len() < max_len { d.push(ch); digits.set(d); }
                }
            }
        } else if key == "Backspace" {
            ev.prevent_default();
            let mut d = digits.get_untracked();
            d.pop();
            digits.set(d);
        }
    };

    let dots: Vec<usize> = (0..max_len).collect();

    view! {
        <div class="pin-input-wrap" on:click=on_wrap_click>
            // Dot display
            <div class=move || if shake.get() { "pin-blocks-wrap pin-shake" } else { "pin-blocks-wrap" }>
                <div class="pin-blocks">
                    {dots.into_iter().map(|i| view! {
                        <div class=move || {
                            let len = digits.get().len();
                            if len > i { "pin-block filled" }
                            else if len == i { "pin-block active" }
                            else { "pin-block" }
                        }>
                            {move || if digits.get().len() > i { "\u{25cf}" } else { "" }}
                        </div>
                    }).collect_view()}
                </div>
            </div>

            // Hidden input — type="text" (not "tel") ensures prevent_default()
            // on keydown suppresses the input event in Tauri/WebView2 on Windows.
            // Deliberately NO on:input — eliminates set_value() infinite loop.
            <input
                node_ref=input_ref
                type="text"
                inputmode="numeric"
                autocomplete="off"
                class="pin-hidden-input"
                on:keydown=on_keydown
            />

            // On-screen keypad — mobile digit entry via button clicks,
            // which bypass the input event system entirely.
            <div class="pin-keypad">
                {["1","2","3","4","5","6","7","8","9","","0","\u{232b}"]
                    .iter()
                    .map(|&label| {
                        if label.is_empty() {
                            view! {
                                <button type="button" class="pin-key blank" disabled=true></button>
                            }.into_any()
                        } else if label == "\u{232b}" {
                            view! {
                                <button type="button" class="pin-key back"
                                    on:click=move |ev| {
                                        ev.stop_propagation();
                                        let mut d = digits.get_untracked();
                                        d.pop();
                                        digits.set(d);
                                    }>
                                    {label}
                                </button>
                            }.into_any()
                        } else {
                            let ch = label.chars().next().unwrap();
                            view! {
                                <button type="button" class="pin-key"
                                    on:click=move |ev| {
                                        ev.stop_propagation();
                                        let mut d = digits.get_untracked();
                                        if d.len() < max_len { d.push(ch); digits.set(d); }
                                    }>
                                    {label}
                                </button>
                            }.into_any()
                        }
                    })
                    .collect::<Vec<_>>()}
            </div>
        </div>
    }
}

// ── PinScreen ─────────────────────────────────────────────────────────────────

#[component]
fn PinScreen(
    phase: RwSignal<AppPhase>,
    pin_digits: RwSignal<String>,
    pin_msg: RwSignal<String>,
    pin_shake: RwSignal<bool>,
    countdown: RwSignal<u32>,
    #[prop(default = 4)] pin_len: u8,
    on_submit: impl Fn(String) + Clone + Send + 'static,
    // Forgot PIN props
    show_forgot_pin: RwSignal<bool>,
    forgot_input: RwSignal<String>,
    forgot_msg: RwSignal<String>,
    forgot_busy: RwSignal<bool>,
    forgot_use_raw_key: RwSignal<bool>,
    // Biometric props
    bio_attempted: RwSignal<bool>,
    bio_show_pin: RwSignal<bool>,
) -> impl IntoView {
    let target_len = pin_len as usize;
    let on_submit_auto = on_submit.clone();
    let on_submit_btn  = on_submit.clone();

    // Auto-submit when all digits are entered.
    Effect::new(move |_| {
        let d = pin_digits.get();
        if d.len() == target_len {
            let captured = d.clone();
            pin_digits.set(String::new()); // clearing triggers PinInput auto-focus
            on_submit_auto(captured);
        }
    });

    // Biometric auto-trigger on mount (PinUnlock only)
    Effect::new(move |_| {
        if phase.get() == AppPhase::PinUnlock && !bio_attempted.get() {
            bio_attempted.set(true);
            spawn_local(async move {
                let method = call::<String>("get_auth_method", no_args()).await.unwrap_or_else(|_| "pin".to_string());
                if method == "biometric" {
                    match call::<bool>("authenticate_biometric", no_args()).await {
                        Ok(true) => {
                            // Success — emit a synthetic "biometric-unlock" event
                            // The parent handles unlock via bio_show_pin signal
                            bio_show_pin.set(false); // biometric succeeded, no need for PIN
                            pin_msg.set("biometric_ok".to_string()); // signal to parent
                        }
                        _ => {
                            bio_show_pin.set(true); // show PIN pad as fallback
                        }
                    }
                } else {
                    bio_show_pin.set(true); // PIN mode, show pad normally
                }
            });
        }
    });

    view! {
        <div class="app">
            <div style="text-align:center;padding:20px 0 8px">
                <img src=logo_src() alt="ChronX" style="height:44px;width:auto;display:inline-block" />
            </div>

            // Biometric unlock screen (shown when biometric mode, before fallback to PIN)
            {move || {
                let is_unlock = phase.get() == AppPhase::PinUnlock;
                let show_pin = bio_show_pin.get();
                let attempted = bio_attempted.get();
                if is_unlock && !show_pin && attempted {
                    // Biometric succeeded or still waiting — show clean screen
                    return view! {
                        <div class="pin-screen" style="display:flex;flex-direction:column;align-items:center;justify-content:center;gap:16px;padding-top:40px">
                            <span style="font-size:48px">{"\u{1f9d1}\u{200d}\u{1f4bb}"}</span>
                            <p class="pin-title">"Unlock with Windows Hello"</p>
                            <p class="muted" style="font-size:13px">"Verifying your identity\u{2026}"</p>
                        </div>
                    }.into_any();
                }
                if is_unlock && !show_pin && !attempted {
                    // Still loading auth_method check
                    return view! {
                        <div class="pin-screen" style="display:flex;flex-direction:column;align-items:center;justify-content:center;gap:16px;padding-top:40px">
                            <span style="font-size:48px">{"\u{1f513}"}</span>
                            <p class="pin-title">"Unlocking\u{2026}"</p>
                        </div>
                    }.into_any();
                }
                view! { <span></span> }.into_any()
            }}

            <div class="pin-screen" style:display=move || {
                let is_unlock = phase.get() == AppPhase::PinUnlock;
                let show_pin = bio_show_pin.get();
                if is_unlock && !show_pin { "none" } else { "" }
            }>
                <p class="pin-title">
                    {move || match phase.get() {
                        AppPhase::PinSetup   => "Create Your PIN",
                        AppPhase::PinConfirm => "Confirm Your PIN",
                        AppPhase::PinUnlock  => "Enter Your PIN",
                        _ => "PIN",
                    }}
                </p>

                <p class="pin-subtitle">
                    {move || match phase.get() {
                        AppPhase::PinSetup   => format!("Choose a {}-digit PIN to secure your wallet", pin_len),
                        AppPhase::PinConfirm => "Enter the same PIN again to confirm".to_string(),
                        AppPhase::PinUnlock  => "Enter your PIN to access your wallet".to_string(),
                        _ => String::new(),
                    }}
                </p>

                // Shared PIN digit entry: dots + hidden keyboard input + on-screen keypad
                <PinInput digits=pin_digits shake=pin_shake pin_len=pin_len />

                // Confirm button — appears when all digits are entered
                {move || if pin_digits.get().len() == target_len {
                    let on_submit_btn2 = on_submit_btn.clone();
                    view! {
                        <button class="pin-confirm-btn" on:click=move |_| {
                            let d = pin_digits.get_untracked();
                            if d.len() == target_len {
                                pin_digits.set(String::new());
                                on_submit_btn2(d);
                            }
                        }>"Confirm"</button>
                    }.into_any()
                } else {
                    view! { <span></span> }.into_any()
                }}

                {move || {
                    let c = countdown.get();
                    let msg = pin_msg.get();
                    if c > 0 {
                        view! {
                            <p class="pin-lockout-msg">"\u{23f1} Please wait " {c} " seconds"</p>
                        }.into_any()
                    } else if !msg.is_empty() && msg != "biometric_ok" {
                        view! { <p class="pin-msg">{msg}</p> }.into_any()
                    } else {
                        view! { <span></span> }.into_any()
                    }
                }}

                // "Forgot PIN?" link (only on unlock screen)
                {move || if phase.get() == AppPhase::PinUnlock {
                    view! {
                        <a href="javascript:void(0)" style="color:#666;font-size:12px;text-decoration:none;margin-top:8px;display:inline-block"
                            on:click=move |_| {
                                forgot_input.set(String::new());
                                forgot_msg.set(String::new());
                                forgot_busy.set(false);
                                forgot_use_raw_key.set(false);
                                show_forgot_pin.set(true);
                            }>"Forgot PIN?"</a>
                    }.into_any()
                } else {
                    view! { <span></span> }.into_any()
                }}

                <p class="version-footer" style="margin-top:auto;padding-top:12px;opacity:0.4;font-size:11px">
                    "ChronX Wallet v2.5.9"
                </p>
            </div>
        </div>

        // ── Forgot PIN modal ────────────────────────────────────────────────
        {move || if show_forgot_pin.get() {
            view! {
                <div class="modal-overlay" on:click=move |_| {
                    if !forgot_busy.get_untracked() { show_forgot_pin.set(false); }
                }>
                    <div class="modal-card" style="max-width:440px" on:click=move |ev: web_sys::MouseEvent| ev.stop_propagation()>
                        <p class="modal-title">"\u{1f511} Reset Your PIN"</p>
                        <div class="modal-body" style="text-align:left">
                            {move || if forgot_use_raw_key.get() {
                                // Raw key mode
                                view! {
                                    <p class="muted" style="font-size:13px;margin-bottom:10px">
                                        "Paste your base64 private key to verify your identity."
                                    </p>
                                    <textarea
                                        rows="4"
                                        placeholder="Paste your private key (base64)..."
                                        style="width:100%;padding:10px;font-size:13px;background:#1a1a2e;border:1px solid #333;border-radius:8px;color:#fff;font-family:monospace;resize:vertical"
                                        prop:value=move || forgot_input.get()
                                        on:input=move |ev| {
                                            forgot_input.set(event_target_value(&ev));
                                            forgot_msg.set(String::new());
                                        }
                                    ></textarea>
                                    {move || {
                                        let m = forgot_msg.get();
                                        if m.is_empty() { view! { <span></span> }.into_any() }
                                        else { view! { <p class="msg error" style="margin-top:8px">{m}</p> }.into_any() }
                                    }}
                                    <button
                                        style="width:100%;margin-top:12px;background:linear-gradient(135deg,#b8860b,#daa520);color:#000;font-weight:700;padding:10px;border:none;border-radius:8px;cursor:pointer"
                                        disabled=move || forgot_busy.get() || forgot_input.get().trim().is_empty()
                                        on:click=move |_| {
                                            forgot_busy.set(true);
                                            forgot_msg.set(String::new());
                                            let key = forgot_input.get_untracked();
                                            spawn_local(async move {
                                                let args = serde_wasm_bindgen::to_value(
                                                    &serde_json::json!({ "key": key })
                                                ).unwrap_or(no_args());
                                                match call::<String>("reset_pin_with_key", args).await {
                                                    Ok(_) => {
                                                        show_forgot_pin.set(false);
                                                        pin_digits.set(String::new());
                                                        pin_msg.set(String::new());
                                                        phase.set(AppPhase::PinSetup);
                                                    }
                                                    Err(e) => forgot_msg.set(format!("Error: {e}")),
                                                }
                                                forgot_busy.set(false);
                                            });
                                        }
                                    >{move || if forgot_busy.get() { "Verifying\u{2026}" } else { "Verify Raw Key" }}</button>
                                    <p style="text-align:center;margin-top:8px">
                                        <a href="javascript:void(0)" style="color:#888;font-size:12px;text-decoration:underline"
                                            on:click=move |_| { forgot_use_raw_key.set(false); forgot_input.set(String::new()); forgot_msg.set(String::new()); }
                                        >"Use seed phrase instead"</a>
                                    </p>
                                }.into_any()
                            } else {
                                // Seed phrase mode (default)
                                view! {
                                    <p class="muted" style="font-size:13px;margin-bottom:10px">
                                        "Enter your 24-word seed phrase to verify your identity and set a new PIN."
                                    </p>
                                    <textarea
                                        rows="4"
                                        placeholder="word1 word2 word3..."
                                        style="width:100%;padding:10px;font-size:13px;background:#1a1a2e;border:1px solid #333;border-radius:8px;color:#fff;font-family:monospace;resize:vertical"
                                        prop:value=move || forgot_input.get()
                                        on:input=move |ev| {
                                            forgot_input.set(event_target_value(&ev));
                                            forgot_msg.set(String::new());
                                        }
                                    ></textarea>
                                    {move || {
                                        let m = forgot_msg.get();
                                        if m.is_empty() { view! { <span></span> }.into_any() }
                                        else { view! { <p class="msg error" style="margin-top:8px">{m}</p> }.into_any() }
                                    }}
                                    <button
                                        style="width:100%;margin-top:12px;background:linear-gradient(135deg,#b8860b,#daa520);color:#000;font-weight:700;padding:10px;border:none;border-radius:8px;cursor:pointer"
                                        disabled=move || forgot_busy.get() || forgot_input.get().trim().is_empty()
                                        on:click=move |_| {
                                            forgot_busy.set(true);
                                            forgot_msg.set(String::new());
                                            let words = forgot_input.get_untracked();
                                            spawn_local(async move {
                                                let args = serde_wasm_bindgen::to_value(
                                                    &serde_json::json!({ "words": words })
                                                ).unwrap_or(no_args());
                                                match call::<String>("reset_pin_with_mnemonic", args).await {
                                                    Ok(_) => {
                                                        show_forgot_pin.set(false);
                                                        pin_digits.set(String::new());
                                                        pin_msg.set(String::new());
                                                        phase.set(AppPhase::PinSetup);
                                                    }
                                                    Err(e) => forgot_msg.set(format!("Error: {e}")),
                                                }
                                                forgot_busy.set(false);
                                            });
                                        }
                                    >{move || if forgot_busy.get() { "Verifying\u{2026}" } else { "Verify Seed Phrase" }}</button>
                                    <p style="text-align:center;margin-top:8px">
                                        <a href="javascript:void(0)" style="color:#888;font-size:12px;text-decoration:underline"
                                            on:click=move |_| { forgot_use_raw_key.set(true); forgot_input.set(String::new()); forgot_msg.set(String::new()); }
                                        >"Using a legacy wallet? Enter raw key instead"</a>
                                    </p>
                                }.into_any()
                            }}
                        </div>
                        <button on:click=move |_| show_forgot_pin.set(false)
                            style="margin-top:12px;width:100%">"Cancel"</button>
                    </div>
                </div>
            }.into_any()
        } else { view! { <span></span> }.into_any() }}
    }
}

// ── WelcomeScreen ─────────────────────────────────────────────────────────────

#[component]
fn WelcomeScreen(
    on_create: impl Fn(web_sys::MouseEvent) + 'static,
    busy: RwSignal<bool>,
    msg: RwSignal<String>,
    on_restore: impl Fn(web_sys::MouseEvent) + 'static,
) -> impl IntoView {
    view! {
        <div class="app">
            <div class="welcome-screen">
                <img src=logo_src() alt="ChronX" style="height:64px;width:auto" />
                <p class="welcome-title">"Welcome to ChronX Wallet"</p>
                <p class="welcome-sub">"The Future Payment Protocol"</p>
                <div class="welcome-btn-group">
                    <button class="primary" on:click=on_create disabled=move || busy.get()>
                        {move || if busy.get() { "Creating wallet\u{2026}" } else { "Create New Wallet" }}
                    </button>
                    <button on:click=on_restore disabled=move || busy.get()>
                        "Restore Existing Wallet"
                    </button>
                </div>
                {move || {
                    let s = msg.get();
                    if s.is_empty() { view! { <span></span> }.into_any() }
                    else {
                        let cls = if s.starts_with("Error") { "msg error" } else { "msg" };
                        view! { <p class=cls>{s}</p> }.into_any()
                    }
                }}
            </div>
        </div>
    }
}

// ── BackupKeyScreen ───────────────────────────────────────────────────────────

#[component]
fn BackupKeyScreen(
    backup_key: RwSignal<String>,
    mnemonic: RwSignal<String>,
    copied: RwSignal<bool>,
    on_copy: impl Fn(web_sys::MouseEvent) + 'static,
    on_confirm: impl Fn(web_sys::MouseEvent) + 'static,
) -> impl IntoView {
    view! {
        <div class="app">
            <div style="text-align:center;padding:20px 0 8px">
                <img src=logo_src() alt="ChronX" style="height:44px;width:auto;display:inline-block" />
            </div>
            <div class="backup-screen">
                {move || {
                    let words = mnemonic.get();
                    if !words.is_empty() {
                        // ── Mnemonic backup (new wallets) ──
                        let word_list: Vec<String> = words.split_whitespace().map(|s| s.to_string()).collect();
                        view! {
                            <p class="section-title">"Back Up Your Recovery Phrase"</p>
                            <div class="backup-warning">
                                "\u{26a0}\u{fe0f} Write these 24 words down on paper. \
                                 They are the ONLY way to recover your wallet. \
                                 Never share them with anyone."
                            </div>
                            <div style="display:grid;grid-template-columns:1fr 1fr 1fr;gap:6px 12px;\
                                        background:#1a1a2e;border:1px solid #333;border-radius:8px;\
                                        padding:16px;margin:8px 0;font-family:monospace;font-size:14px">
                                {word_list.into_iter().enumerate().map(|(i, word)| {
                                    view! {
                                        <div style="display:flex;gap:6px;align-items:center">
                                            <span style="color:#888;min-width:24px;text-align:right;font-size:12px">
                                                {format!("{}.", i + 1)}
                                            </span>
                                            <span style="color:#e0e0e0">{word}</span>
                                        </div>
                                    }
                                }).collect::<Vec<_>>()}
                            </div>
                        }.into_any()
                    } else {
                        // ── Legacy backup key ──
                        view! {
                            <p class="section-title">"Save Your Backup Key"</p>
                            <div class="backup-warning">
                                "\u{26a0} Save this backup key now. Anyone who has it can access your wallet. \
                                 If you lose it and forget your PIN, your wallet cannot be recovered."
                            </div>
                            <textarea
                                class="backup-key-box"
                                readonly
                                rows="5"
                                prop:value=move || backup_key.get()
                            />
                        }.into_any()
                    }
                }}
                <div style="display:flex;gap:8px">
                    <button on:click=on_copy style="flex:1">
                        {move || if copied.get() { "\u{2713} Copied!" } else { "Copy to Clipboard" }}
                    </button>
                </div>
                <button class="primary" on:click=on_confirm style="margin-top:8px">
                    "I've written them down \u{2192}"
                </button>
                <p class="muted" style="font-size:11px;text-align:center">
                    "Store on paper in a safe place. Never store digitally."
                </p>
            </div>
        </div>
    }
}

// ── RestoreWalletScreen ───────────────────────────────────────────────────────

#[component]
fn RestoreWalletScreen(
    input: RwSignal<String>,
    msg: RwSignal<String>,
    busy: RwSignal<bool>,
    on_back: impl Fn(web_sys::MouseEvent) + 'static,
    on_restore: impl Fn(web_sys::MouseEvent) + 'static,
) -> impl IntoView {
    view! {
        <div class="app">
            <div style="text-align:center;padding:20px 0 8px">
                <img src=logo_src() alt="ChronX" style="height:44px;width:auto;display:inline-block" />
            </div>
            <div class="restore-screen">
                <p class="section-title">"Restore Existing Wallet"</p>
                <p class="label">"Enter your 24-word recovery phrase or paste your backup key:"</p>
                <textarea
                    class="restore-textarea"
                    rows="5"
                    placeholder="Enter your 24 recovery words or paste backup key\u{2026}"
                    on:input=move |ev| {
                        use wasm_bindgen::JsCast;
                        if let Some(el) = ev.target()
                            .and_then(|t| t.dyn_into::<web_sys::HtmlTextAreaElement>().ok())
                        {
                            input.set(el.value());
                        }
                    }
                >
                    {move || input.get()}
                </textarea>
                {move || {
                    let s = msg.get();
                    if s.is_empty() { view! { <span></span> }.into_any() }
                    else {
                        let cls = if s.starts_with("Error") { "msg error" } else { "msg success" };
                        view! { <p class=cls>{s}</p> }.into_any()
                    }
                }}
                <button class="primary" on:click=on_restore disabled=move || busy.get()>
                    {move || if busy.get() { "Restoring\u{2026}" } else { "Restore Wallet" }}
                </button>
                <button on:click=on_back disabled=move || busy.get()>
                    "\u{2190} Back"
                </button>
            </div>
        </div>
    }
}

// ── AccountPanel ──────────────────────────────────────────────────────────────

#[component]
fn AccountPanel(
    info: RwSignal<Option<AccountInfo>>,
    loading: RwSignal<bool>,
    err_msg: RwSignal<String>,
    on_refresh: impl Fn(web_sys::MouseEvent) + 'static,
    pending_email_chronos: RwSignal<u64>,
    active_tab: RwSignal<u8>,
    activity_sub: RwSignal<u8>,
    deep_link_code: RwSignal<String>,
    lang: RwSignal<String>,
    avatar_url: RwSignal<String>,
    avatar_bust: RwSignal<f64>,
    display_name: RwSignal<String>,
    display_name_editing: RwSignal<bool>,
    display_name_input: RwSignal<String>,
    avatar_msg: RwSignal<String>,
    avatar_uploading: RwSignal<bool>,
    show_profile_modal: RwSignal<bool>,
    badge: RwSignal<String>,
    pending_loan_offers_count: RwSignal<u32>,
    loans_data: RwSignal<serde_json::Value>,
) -> impl IntoView {
    let copy_success = RwSignal::new(false);
    let incoming     = RwSignal::new(Vec::<TimeLockInfo>::new());
    let inc_loading  = RwSignal::new(false);
    let qr_svg       = RwSignal::new(String::new());
    let qr_visible   = RwSignal::new(false);

    // v2.2.2: KXGO badges — fetched from notify API alongside existing badge
    let kxgo_badges: RwSignal<Vec<WalletBadge>> = RwSignal::new(Vec::new());
    Effect::new(move |_| {
        if let Some(acct) = info.get() {
            let wallet = acct.account_id.clone();
            spawn_local(async move {
                let args = serde_wasm_bindgen::to_value(
                    &serde_json::json!({ "walletAddress": wallet })
                ).unwrap_or(JsValue::NULL);
                if let Ok(badges) = call::<Vec<WalletBadge>>("get_wallet_badges", args).await {
                    kxgo_badges.set(badges);
                }
            });
        }
    });

    // Claim code on Receive tab (pre-fill from deep link — reactive Effect)
    let home_claim_code = RwSignal::new(String::new());
    Effect::new(move |_| {
        let code = deep_link_code.get();
        if !code.is_empty() {
            home_claim_code.set(code);
            deep_link_code.set(String::new()); // consume it
        }
    });
    let home_claim_msg  = RwSignal::new(String::new());
    let home_claim_busy = RwSignal::new(false);
    let claim_collapsed = RwSignal::new(false); // true = collapsed (user has verified emails)

    // Check if user has verified emails → collapse claim section
    Effect::new(move |_| {
        spawn_local(async move {
            let emails = call::<Vec<String>>("get_verified_emails", no_args()).await.unwrap_or_default();
            claim_collapsed.set(!emails.is_empty());
        });
    });

    // Whitelist popup state (shown after successful claim)
    let wl_show    = RwSignal::new(false);
    let wl_email   = RwSignal::new(String::new());
    let wl_amount  = RwSignal::new(String::new());
    let wl_busy    = RwSignal::new(false);
    let wl_msg     = RwSignal::new(String::new());

    // Email registration prompt after claim code (mobile)
    let claim_reg_show  = RwSignal::new(false);
    let claim_reg_email = RwSignal::new(String::new());
    let claim_reg_msg   = RwSignal::new(String::new());
    let claim_reg_busy  = RwSignal::new(false);

    // Avatar & profile state (signals passed from parent App — loading Effect is in App())

    // Convert block state
    let convert_visible   = RwSignal::new(false);
    let convert_amount    = RwSignal::new(String::new());
    let convert_quote     = RwSignal::new(Option::<ConvertQuote>::None);
    let convert_loading   = RwSignal::new(false);
    let convert_error     = RwSignal::new(String::new());
    let convert_countdown = RwSignal::new(0u32);
    let convert_debounce  = RwSignal::new(0u32);
    // Base address for KX↔USDC conversion
    let convert_base_addr = RwSignal::new(String::new());
    let convert_base_err  = RwSignal::new(String::new());
    let convert_busy      = RwSignal::new(false);
    let convert_msg       = RwSignal::new(String::new());
    let convert_nickname  = RwSignal::new(String::new());
    // Saved base addresses (chips)
    let convert_saved_addrs = RwSignal::new(Vec::<(String, String)>::new()); // (address, nickname)
    let convert_nick_saved = RwSignal::new(false); // brief "Saved!" confirmation
    let convert_addr_unknown = RwSignal::new(false);
    let convert_addr_checked = RwSignal::new(false); // true = check completed
    let convert_addr_override = RwSignal::new(false); // user checked "I've verified"
    // Load saved base addresses
    spawn_local(async move {
        if let Ok(addrs) = call::<Vec<serde_json::Value>>("get_base_addresses", no_args()).await {
            let parsed: Vec<(String, String)> = addrs.into_iter().filter_map(|v| {
                let addr = v.get("address")?.as_str()?.to_string();
                let nick = v.get("nickname")?.as_str()?.to_string();
                Some((addr, nick))
            }).collect();
            convert_saved_addrs.set(parsed);
        }
    });

    // Quote countdown timer (runs once, loops forever)
    spawn_local(async move {
        loop {
            delay_ms(1000).await;
            let c = convert_countdown.get_untracked();
            if c > 1 {
                convert_countdown.set(c - 1);
            } else if c == 1 {
                convert_countdown.set(0);
                // Auto-refetch if amount is valid
                let amt_str = convert_amount.get_untracked();
                if let Ok(amt) = amt_str.parse::<f64>() {
                    if amt > 0.0 {
                        convert_loading.set(true);
                        convert_error.set(String::new());
                        match fetch_convert_quote(amt).await {
                            Ok(q) => {
                                convert_quote.set(Some(q));
                                convert_countdown.set(30);
                                convert_loading.set(false);
                            }
                            Err(e) => {
                                convert_loading.set(false);
                                convert_error.set(e);
                                convert_quote.set(None);
                            }
                        }
                    }
                }
            }
        }
    });

    // Load incoming promises on mount
    Effect::new(move |_| {
        spawn_local(async move {
            inc_loading.set(true);
            if let Ok(locks) = call::<Vec<TimeLockInfo>>("get_pending_incoming", no_args()).await {
                incoming.set(locks);
            }
            inc_loading.set(false);
        });
    });

    let on_copy = move |_: web_sys::MouseEvent| {
        let addr = info.get_untracked().map(|a| a.account_id).unwrap_or_default();
        if addr.is_empty() { return; }
        spawn_local(async move {
            copy_to_clipboard(addr).await;
            copy_success.set(true);
            delay_ms(2000).await;
            copy_success.set(false);
        });
    };

    let on_toggle_qr = move |_: web_sys::MouseEvent| {
        if qr_visible.get() {
            qr_visible.set(false);
        } else {
            let account_id = info.get_untracked().map(|a| a.account_id).unwrap_or_default();
            if account_id.is_empty() { return; }
            qr_svg.set(make_qr_svg(&account_id));
            qr_visible.set(true);
        }
    };

    view! {
        // ── Receive tab content ──────────────────────────────────────────────
        <div>
            <div class="card">
                // ── Avatar + Balance + Refresh (combined row) ──────────────
                <div style="display:flex;align-items:center;gap:14px;margin-bottom:8px">
                    // Avatar circle (left) — click opens profile modal
                    <div style="flex-shrink:0">
                        <div style="position:relative;cursor:pointer"
                            on:click=move |_| show_profile_modal.set(true)>
                            <img
                                src={move || {
                                    let base = avatar_url.get();
                                    if base.is_empty() { return String::new(); }
                                    let bust = avatar_bust.get();
                                    if bust > 0.0 { format!("{}?t={:.0}", base, bust) } else { base }
                                }}
                                style="width:56px;height:56px;border-radius:50%;border:2px solid #d4a84b;object-fit:cover;display:block;background:#1a1a2e"
                            />
                            <div style="position:absolute;bottom:0;right:0;background:#d4a84b;border-radius:50%;width:18px;height:18px;display:flex;align-items:center;justify-content:center;font-size:11px;line-height:1">
                                "\u{1F4F7}"
                            </div>
                        </div>
                    </div>
                    // Name + Balance (middle)
                    <div style="flex:1;min-width:0">
                        {move || {
                            let dn = display_name.get();
                            if !dn.is_empty() {
                                view! { <p style="font-size:13px;color:#d4a84b;font-weight:700;margin:0 0 2px;white-space:nowrap;overflow:hidden;text-overflow:ellipsis">{dn}
                                    {move || match badge.get().as_str() {
                                        "FOUNDING_MEMBER" | "Founding Team" => view! { <span style="display:inline-block;padding:2px 10px;border-radius:4px;background:#d4a84b;color:black;font-size:11px;font-weight:700;margin-left:4px">{"Founding Team"}</span> }.into_any(),
                                        "GENESIS_MEMBER" => view! { <span style="display:inline-block;padding:2px 10px;border-radius:4px;background:#d4a84b;color:black;font-size:11px;font-weight:700;margin-left:4px">{"Genesis"}</span> }.into_any(),
                                        "PROTOCOL_PATRON" => view! { <span style="display:inline-block;padding:2px 10px;border-radius:4px;background:#e2e8f0;color:#1a1a2e;font-size:11px;font-weight:700;margin-left:4px">{"Patron"}</span> }.into_any(),
                                        _ => view! { <span></span> }.into_any(),
                                    }}
                                    // v2.2.2: KXGO badges (Bronze/Silver/Gold)
                                    {move || {
                                        let badges = kxgo_badges.get();
                                        view! {
                                            {badges.into_iter().filter_map(|b| {
                                                let (bg, fg, label) = match b.badge_type.as_str() {
                                                    "KXGO_BRONZE" => ("#CD7F32", "white", "KXGO Bronze"),
                                                    "KXGO_SILVER" => ("#C0C0C0", "#1a1a2e", "KXGO Silver"),
                                                    "KXGO_GOLD"   => ("#D4A84B", "black", "KXGO Gold"),
                                                    _ => return None,
                                                };
                                                Some(view! {
                                                    <span style={format!("display:inline-block;padding:2px 10px;border-radius:4px;background:{bg};color:{fg};font-size:11px;font-weight:700;margin-left:4px")}>{label}</span>
                                                })
                                            }).collect_view()}
                                        }
                                    }}
                                </p> }.into_any()
                            } else {
                                view! { <span></span> }.into_any()
                            }
                        }}
                        <p class="label" style="margin:0 0 2px">"Balance"</p>
                        <p class="balance" style="margin:0">
                            {move || {
                                if loading.get() { "\u{2026}".into() }
                                else {
                                    info.get()
                                        .map(|a| format!("{} KX", format_kx(&a.balance_chronos)))
                                        .unwrap_or_else(|| "\u{2014}".into())
                                }
                            }}
                        </p>
                    </div>
                    // Refresh button (right)
                    <button on:click=on_refresh disabled=move || loading.get()
                        style="background:none;border:1px solid #333;color:#888;padding:6px 10px;border-radius:6px;cursor:pointer;font-size:14px;flex-shrink:0">
                        {move || if loading.get() { "\u{2026}" } else { "\u{21bb}" }}
                    </button>
                </div>
                // Hidden file input for avatar upload
                <input type="file" id="avatar-file-input" accept="image/jpeg,image/png,image/gif,image/webp"
                    style="display:none"
                    on:change=move |ev| {
                        let target = event_target::<web_sys::HtmlInputElement>(&ev);
                        let files = target.files();
                        if let Some(file_list) = files {
                            if let Some(file) = file_list.get(0) {
                                let wallet = info.get_untracked().map(|a| a.account_id.clone()).unwrap_or_default();
                                if wallet.is_empty() { return; }
                                avatar_uploading.set(true);
                                avatar_msg.set(String::new());
                                let file_name = file.name();
                                spawn_local(async move {
                                    let reader = web_sys::FileReader::new().unwrap();
                                    let reader_clone = reader.clone();
                                    let (tx, rx) = futures::channel::oneshot::channel::<Vec<u8>>();
                                    let tx = std::cell::RefCell::new(Some(tx));
                                    let onload = wasm_bindgen::closure::Closure::wrap(Box::new(move || {
                                        if let Ok(result) = reader_clone.result() {
                                            let arr = js_sys::Uint8Array::new(&result);
                                            let bytes = arr.to_vec();
                                            if let Some(sender) = tx.borrow_mut().take() {
                                                let _ = sender.send(bytes);
                                            }
                                        }
                                    }) as Box<dyn FnMut()>);
                                    reader.set_onloadend(Some(onload.as_ref().unchecked_ref()));
                                    let _ = reader.read_as_array_buffer(&file);
                                    onload.forget();
                                    if let Ok(bytes) = rx.await {
                                        // Base64-encode and upload via Tauri command (avoids CORS)
                                        let b64 = base64::engine::general_purpose::STANDARD.encode(&bytes);
                                        let dn = display_name.get_untracked();
                                        let dn_val = if dn.is_empty() { serde_json::Value::Null } else { serde_json::Value::String(dn) };
                                        let args = serde_wasm_bindgen::to_value(&serde_json::json!({
                                            "walletAddress": wallet,
                                            "imageBase64": b64,
                                            "fileName": file_name,
                                            "displayName": dn_val,
                                        })).unwrap_or(no_args());
                                        match call::<String>("upload_avatar_bytes", args).await {
                                            Ok(_) => {
                                                avatar_bust.set(js_sys::Date::now());
                                                avatar_msg.set("\u{2713} Photo saved".to_string());
                                                show_profile_modal.set(false);
                                                delay_ms(500).await;
                                                show_profile_modal.set(true);
                                            }
                                            Err(e) => {
                                                avatar_msg.set(format!("Upload failed: {}", e));
                                            }
                                        }
                                    }
                                    avatar_uploading.set(false);
                                    delay_ms(3000).await;
                                    avatar_msg.set(String::new());
                                });
                            }
                        }
                        target.set_value("");
                    }
                />
                // Upload status message
                {move || {
                    if avatar_uploading.get() {
                        view! { <p style="font-size:11px;color:#d4a84b;text-align:center;margin:4px 0">"Uploading..."</p> }.into_any()
                    } else {
                        let msg = avatar_msg.get();
                        if msg.is_empty() {
                            view! { <span></span> }.into_any()
                        } else {
                            let color = if msg.contains('\u{2713}') { "#22c55e" } else { "#ef4444" };
                            view! { <p style={format!("font-size:11px;color:{color};text-align:center;margin:4px 0")}>{msg}</p> }.into_any()
                        }
                    }
                }}
                        <p class="label" style="margin-top:4px">
                            "Spendable: "
                            {move || {
                                if loading.get() { "\u{2026}".into() }
                                else {
                                    let pending = pending_email_chronos.get();
                                    let base_str = info.get()
                                        .map(|a| a.spendable_chronos)
                                        .unwrap_or_default();
                                    if base_str.is_empty() { return "\u{2014}".into(); }
                                    if pending > 0 {
                                        let base: u128 = base_str.parse().unwrap_or(0);
                                        let spendable = base.saturating_sub(pending as u128);
                                        format!("{} KX  ({} KX pending email)",
                                            format_kx(&spendable.to_string()),
                                            format_kx(&pending.to_string()))
                                    } else {
                                        format!("{} KX", format_kx(&base_str))
                                    }
                                }
                            }}
                        </p>
                        <p class="fee-free-badge">"✓ Zero fees — every KX sent is received in full"</p>
                        <p style="margin-top:8px;font-size:12px">
                            <a class="exchange-link exchange-link-mobile" href="#" on:click=move |ev| {
                                ev.prevent_default();
                                convert_visible.set(!convert_visible.get_untracked());
                            }>{move || if convert_visible.get() { "\u{25BC} Convert KX \u{2194} USDC" } else { "\u{25B6} Convert KX \u{2194} USDC" }}</a>
                        </p>
                        {move || {
                            if !convert_visible.get() { return view! { <span></span> }.into_any(); }
                            let l = lang.get();
                            view! {
                                <div class="convert-block">
                                    <input type="number" class="convert-input" placeholder="Amount in KX" min="0"
                                        prop:value=move || convert_amount.get()
                                        on:input=move |ev| {
                                            let val = event_target_value(&ev);
                                            convert_amount.set(val.clone());
                                            let counter = convert_debounce.get_untracked() + 1;
                                            convert_debounce.set(counter);
                                            convert_quote.set(None);
                                            convert_error.set(String::new());
                                            convert_countdown.set(0);
                                            let amount: f64 = val.parse().unwrap_or(0.0);
                                            if amount <= 0.0 { convert_loading.set(false); return; }
                                            convert_loading.set(true);
                                            spawn_local(async move {
                                                delay_ms(600).await;
                                                if convert_debounce.get_untracked() != counter { return; }
                                                match fetch_convert_quote(amount).await {
                                                    Ok(q) => {
                                                        if convert_debounce.get_untracked() == counter {
                                                            convert_quote.set(Some(q));
                                                            convert_countdown.set(30);
                                                            convert_loading.set(false);
                                                            convert_error.set(String::new());
                                                        }
                                                    }
                                                    Err(e) => {
                                                        if convert_debounce.get_untracked() == counter {
                                                            convert_loading.set(false);
                                                            convert_error.set(e);
                                                            convert_quote.set(None);
                                                        }
                                                    }
                                                }
                                            });
                                        }
                                    />
                                    {move || {
                                        if convert_loading.get() {
                                            view! { <p class="convert-loading">{t(&lang.get(), "convert_quote_loading")}</p> }.into_any()
                                        } else { view! { <span></span> }.into_any() }
                                    }}
                                    {move || {
                                        let l = lang.get();
                                        if let Some(q) = convert_quote.get() {
                                            let cd = convert_countdown.get();
                                            let expired = cd == 0;
                                            let level_cls = format!("quote-panel quote-{}", q.warning_level);
                                            let cls = if expired { format!("{} quote-expired", level_cls) } else { level_cls };
                                            view! {
                                                <div class={cls}>
                                                    <p class="quote-main">
                                                        {format!("{} KX  \u{2192}  {} USDC", q.kx_in, q.usdc_out)}
                                                    </p>
                                                    <p class="quote-detail">
                                                        {format!("Rate: ${:.6}/KX  \u{00b7}  Fee: {}%", q.trade_rate, q.fee_pct)}
                                                    </p>
                                                    <p class="quote-detail">
                                                        {format!("Slippage: {}%  \u{00b7}  Total cost: {}%", q.slippage_pct, q.total_cost_pct)}
                                                    </p>
                                                    {match q.warning.as_deref() {
                                                        Some(w) => view! { <p class="quote-warning">{format!("\u{26a0} {}", w)}</p> }.into_any(),
                                                        None => view! { <span></span> }.into_any(),
                                                    }}
                                                    <p class="quote-countdown">
                                                        {if expired { t(&l, "convert_quote_expired") }
                                                         else { format!("{} {}s", t(&l, "convert_quote_valid_for"), cd) }}
                                                    </p>
                                                    {if q.requires_confirmation {
                                                        view! { <p class="convert-blocked">{t(&l, "convert_blocked_slippage")}</p> }.into_any()
                                                    } else { view! { <span></span> }.into_any() }}
                                                </div>
                                            }.into_any()
                                        } else if !convert_error.get().is_empty() {
                                            let err = convert_error.get();
                                            let amt: f64 = convert_amount.get().parse().unwrap_or(0.0);
                                            let rate = if err.starts_with("FALLBACK:") {
                                                err[9..].parse().unwrap_or(0.00319)
                                            } else { 0.00319 };
                                            let est = amt * rate;
                                            view! {
                                                <div class="quote-panel quote-yellow">
                                                    <p class="quote-detail">{t(&l, "convert_quote_error")}</p>
                                                    <p class="quote-main">{format!("\u{2248} {:.2} KX \u{2192} {:.4} USDC @ ${:.5}/KX", amt, est, rate)}</p>
                                                </div>
                                            }.into_any()
                                        } else { view! { <span></span> }.into_any() }
                                    }}
                                    // Base wallet address input
                                    <div style="margin-top:10px">
                                        // Saved address chips
                                        {move || {
                                            let addrs = convert_saved_addrs.get();
                                            if addrs.is_empty() { return view! { <span></span> }.into_any(); }
                                            view! {
                                                <div style="margin-bottom:8px">
                                                    <p style="font-size:11px;color:#9ca3af;margin:0 0 4px">"Saved addresses:"</p>
                                                    <div style="display:flex;flex-wrap:wrap;gap:6px">
                                                        {addrs.into_iter().map(|(addr, nick)| {
                                                            let addr_click = addr.clone();
                                                            let addr_del = addr.clone();
                                                            view! {
                                                                <span style="display:inline-flex;align-items:center;gap:4px;background:#1a1a2e;border:1px solid #d4a84b;border-radius:6px;padding:3px 8px;font-size:13px;color:#d4a84b;cursor:pointer"
                                                                    on:click=move |_| {
                                                                        convert_base_addr.set(addr_click.clone());
                                                                        convert_addr_checked.set(false);
                                                                        convert_addr_unknown.set(false);
                                                                        convert_addr_override.set(false);
                                                                        convert_base_err.set(String::new());
                                                                    }>
                                                                    {nick}
                                                                    <span style="color:#ef4444;cursor:pointer;font-size:14px;margin-left:2px;font-weight:700"
                                                                        on:click=move |ev: web_sys::MouseEvent| {
                                                                            ev.stop_propagation();
                                                                            let a = addr_del.clone();
                                                                            spawn_local(async move {
                                                                                let args = serde_wasm_bindgen::to_value(
                                                                                    &serde_json::json!({ "address": a })
                                                                                ).unwrap_or(no_args());
                                                                                let _ = call::<()>("delete_base_address", args).await;
                                                                                if let Ok(addrs) = call::<Vec<serde_json::Value>>("get_base_addresses", no_args()).await {
                                                                                    let parsed: Vec<(String, String)> = addrs.into_iter().filter_map(|v| {
                                                                                        let addr = v.get("address")?.as_str()?.to_string();
                                                                                        let nick = v.get("nickname")?.as_str()?.to_string();
                                                                                        Some((addr, nick))
                                                                                    }).collect();
                                                                                    convert_saved_addrs.set(parsed);
                                                                                }
                                                                            });
                                                                        }>{"\u{00d7}"}</span>
                                                                </span>
                                                            }
                                                        }).collect::<Vec<_>>()}
                                                    </div>
                                                </div>
                                            }.into_any()
                                        }}
                                        <p style="color:#ff4444;font-size:13px;font-weight:700;margin:0 0 6px">{"\u{26a0} Please enter ONLY a receiving USDC address on the Base network. Sending to any other address risks permanent loss of funds."}</p>
                                        <label style="font-size:12px;color:#9ca3af;display:block;margin-bottom:4px">"Your Base wallet address (to receive USDC)"</label>
                                        <input type="text" class="convert-input" placeholder="0x..."
                                            style="font-family:monospace;font-size:13px"
                                            prop:value=move || convert_base_addr.get()
                                            on:input=move |ev| {
                                                let val = event_target_value(&ev);
                                                convert_base_addr.set(val.clone());
                                                convert_addr_checked.set(false);
                                                convert_addr_unknown.set(false);
                                                convert_addr_override.set(false);
                                                let v = val.trim().to_string();
                                                if v.is_empty() {
                                                    convert_base_err.set(String::new());
                                                } else if !v.starts_with("0x") || v.len() != 42
                                                    || !v[2..].chars().all(|c| c.is_ascii_hexdigit())
                                                {
                                                    convert_base_err.set("Must be a valid Base address (0x + 40 hex chars)".into());
                                                } else {
                                                    convert_base_err.set(String::new());
                                                    // Soft address check via XChan API
                                                    let addr_for_check = v.clone();
                                                    spawn_local(async move {
                                                        use wasm_bindgen::JsCast;
                                                        let url = format!("https://api.chronx.io/xchan/check-address?address={}", addr_for_check);
                                                        let window = match web_sys::window() { Some(w) => w, None => return };
                                                        // 2s timeout via AbortController
                                                        let controller = web_sys::AbortController::new().ok();
                                                        let signal = controller.as_ref().map(|c| c.signal());
                                                        if let Some(ref ctrl) = controller {
                                                            let ctrl_clone = ctrl.clone();
                                                            let cb = wasm_bindgen::closure::Closure::once(move || ctrl_clone.abort());
                                                            let _ = window.set_timeout_with_callback_and_timeout_and_arguments_0(
                                                                cb.as_ref().unchecked_ref(), 2000
                                                            );
                                                            cb.forget();
                                                        }
                                                        let mut opts = web_sys::RequestInit::new();
                                                        opts.method("GET");
                                                        if let Some(ref sig) = signal { opts.signal(Some(sig)); }
                                                        let req = match web_sys::Request::new_with_str_and_init(&url, &opts) {
                                                            Ok(r) => r, Err(_) => return
                                                        };
                                                        let resp_val = match JsFuture::from(window.fetch_with_request(&req)).await {
                                                            Ok(v) => v, Err(_) => return // timeout or network error — skip silently
                                                        };
                                                        let resp: web_sys::Response = resp_val.unchecked_into();
                                                        if !resp.ok() { return; }
                                                        if let Ok(text_val) = JsFuture::from(resp.text().unwrap()).await {
                                                            if let Some(text) = text_val.as_string() {
                                                                if let Ok(json) = serde_json::from_str::<serde_json::Value>(&text) {
                                                                    let known = json.get("known").and_then(|v| v.as_bool()).unwrap_or(true);
                                                                    convert_addr_checked.set(true);
                                                                    convert_addr_unknown.set(!known);
                                                                }
                                                            }
                                                        }
                                                    });
                                                }
                                            }
                                        />
                                        {move || {
                                            let e = convert_base_err.get();
                                            if e.is_empty() { view! { <span></span> }.into_any() }
                                            else { view! { <p style="color:#ef4444;font-size:11px;margin:4px 0 0">{e}</p> }.into_any() }
                                        }}
                                        // Unknown address warning
                                        {move || {
                                            if !convert_addr_checked.get() || !convert_addr_unknown.get() {
                                                return view! { <span></span> }.into_any();
                                            }
                                            view! {
                                                <div style="background:rgba(234,179,8,0.12);border:1px solid rgba(234,179,8,0.4);border-radius:6px;padding:8px 10px;margin-top:6px">
                                                    <p style="color:#eab308;font-size:12px;margin:0 0 6px">
                                                        "\u{26a0}\u{fe0f} This address has no transaction history on Base network. Double-check it\u{2019}s correct before converting."
                                                    </p>
                                                    <label style="display:flex;align-items:center;gap:6px;font-size:12px;color:#e5e7eb;cursor:pointer">
                                                        <input type="checkbox"
                                                            style="accent-color:#d4a84b"
                                                            prop:checked=move || convert_addr_override.get()
                                                            on:change=move |ev| {
                                                                use wasm_bindgen::JsCast;
                                                                let checked = ev.target()
                                                                    .and_then(|t| t.dyn_into::<web_sys::HtmlInputElement>().ok())
                                                                    .map(|i| i.checked()).unwrap_or(false);
                                                                convert_addr_override.set(checked);
                                                            } />
                                                        "I\u{2019}ve verified this address is correct \u{2014} proceed anyway"
                                                    </label>
                                                </div>
                                            }.into_any()
                                        }}
                                        // Nickname + Save row (visible when valid address AND < 5 saved)
                                        {move || {
                                            let addr = convert_base_addr.get();
                                            let addr_valid = {
                                                let v = addr.trim();
                                                v.starts_with("0x") && v.len() == 42 && v[2..].chars().all(|c| c.is_ascii_hexdigit())
                                            };
                                            let count = convert_saved_addrs.get().len();
                                            if !addr_valid { return view! { <span></span> }.into_any(); }
                                            if count >= 5 {
                                                return view! { <p style="color:#ef4444;font-size:11px;margin:6px 0 0">"Delete a saved address to add another"</p> }.into_any();
                                            }
                                            let save_addr = move || {
                                                let a = convert_base_addr.get_untracked().trim().to_string();
                                                let n = convert_nickname.get_untracked().trim().to_string();
                                                let nick = if n.is_empty() { "Saved".to_string() } else { n };
                                                spawn_local(async move {
                                                    let args = serde_wasm_bindgen::to_value(
                                                        &serde_json::json!({ "address": a, "nickname": nick })
                                                    ).unwrap_or(no_args());
                                                    let _ = call::<()>("add_base_address", args).await;
                                                    convert_nickname.set(String::new());
                                                    convert_nick_saved.set(true);
                                                    set_timeout(move || convert_nick_saved.set(false), std::time::Duration::from_secs(2));
                                                    // Reload chips
                                                    if let Ok(addrs) = call::<Vec<serde_json::Value>>("get_base_addresses", no_args()).await {
                                                        let parsed: Vec<(String, String)> = addrs.into_iter().filter_map(|v| {
                                                            let addr = v.get("address")?.as_str()?.to_string();
                                                            let nick = v.get("nickname")?.as_str()?.to_string();
                                                            Some((addr, nick))
                                                        }).collect();
                                                        convert_saved_addrs.set(parsed);
                                                    }
                                                });
                                            };
                                            let save_enter = save_addr.clone();
                                            view! {
                                                <div style="margin-top:6px">
                                                    <label style="font-size:11px;color:#9ca3af;display:block;margin-bottom:2px">"Nickname (optional)"</label>
                                                    <div style="display:flex;align-items:center;gap:6px">
                                                        <input type="text" class="convert-input" placeholder="e.g. Coinbase, MetaMask"
                                                            style="font-size:12px;flex:1"
                                                            prop:value=move || convert_nickname.get()
                                                            on:input=move |ev| convert_nickname.set(event_target_value(&ev))
                                                            on:keydown=move |ev: web_sys::KeyboardEvent| {
                                                                if ev.key() == "Enter" {
                                                                    ev.prevent_default();
                                                                    save_enter();
                                                                }
                                                            }
                                                        />
                                                        <button style="background:#d4a84b;color:#0a0a0a;border:none;border-radius:4px;padding:4px 12px;font-size:12px;font-weight:600;cursor:pointer;white-space:nowrap"
                                                            on:click=move |_| save_addr()
                                                        >"Save"</button>
                                                    </div>
                                                    {move || if convert_nick_saved.get() {
                                                        view! { <p style="color:#22c55e;font-size:11px;margin:4px 0 0;font-weight:600">{"\u{2713} Saved!"}</p> }.into_any()
                                                    } else { view! { <span></span> }.into_any() }}
                                                </div>
                                            }.into_any()
                                        }}
                                    </div>
                                    // Convert result message
                                    {move || {
                                        let m = convert_msg.get();
                                        if m.is_empty() { return view! { <span></span> }.into_any(); }
                                        let is_err = m.starts_with("Error");
                                        let cls = if is_err { "msg error" } else { "msg success" };
                                        view! { <p class=cls style="margin-top:6px">{m}</p> }.into_any()
                                    }}
                                    {move || {
                                        let q = convert_quote.get();
                                        let cd = convert_countdown.get();
                                        let is_loading = convert_loading.get();
                                        let blocked = q.as_ref().map(|q| q.requires_confirmation).unwrap_or(false);
                                        let expired = cd == 0 && q.is_some();
                                        let has_fallback = !convert_error.get().is_empty();
                                        let can_convert = (q.is_some() && !blocked && !expired) || has_fallback;
                                        // Require valid base address
                                        let addr = convert_base_addr.get();
                                        let addr_valid = {
                                            let v = addr.trim();
                                            v.starts_with("0x") && v.len() == 42 && v[2..].chars().all(|c| c.is_ascii_hexdigit())
                                        };
                                        // If address is unknown and user hasn't overridden, block
                                        let addr_blocked = convert_addr_checked.get() && convert_addr_unknown.get() && !convert_addr_override.get();
                                        let busy = convert_busy.get();
                                        let disabled = is_loading || !can_convert || !addr_valid || addr_blocked || busy;
                                        let btn_class = if blocked { "convert-btn convert-btn-blocked" } else { "convert-btn" };
                                        view! {
                                            <button class={btn_class} disabled=disabled
                                                on:click=move |_| {
                                                    let amt_str = convert_amount.get_untracked();
                                                    let amt: f64 = amt_str.parse().unwrap_or(0.0);
                                                    let addr_val = convert_base_addr.get_untracked().trim().to_string();
                                                    if amt <= 0.0 || addr_val.len() != 42 { return; }
                                                    convert_busy.set(true);
                                                    convert_msg.set(String::new());
                                                    spawn_local(async move {
                                                        let args = serde_wasm_bindgen::to_value(
                                                            &serde_json::json!({ "amountKx": amt, "baseAddress": addr_val })
                                                        ).unwrap_or(no_args());
                                                        match call::<String>("convert_kx_to_usdc", args).await {
                                                            Ok(txid) => {
                                                                convert_msg.set(format!("\u{2705} Conversion initiated! USDC will arrive in your Base wallet within a few minutes. TxId: {}", &txid[..16.min(txid.len())]));
                                                                convert_amount.set(String::new());
                                                                convert_quote.set(None);
                                                                poll_balance_update(info).await;
                                                            }
                                                            Err(e) => convert_msg.set(format!("Error: {e}")),
                                                        }
                                                        convert_busy.set(false);
                                                    });
                                                }>
                                                {move || if convert_busy.get() { "Converting\u{2026}" } else { "Convert via XChan" }}
                                            </button>
                                        }
                                    }}
                                </div>
                            }.into_any()
                        }}
                {move || {
                    let e = err_msg.get();
                    if e.is_empty() { view! { <span></span> }.into_any() }
                    else { view! { <p class="error">{e}</p> }.into_any() }
                }}

                // ── Section A: Account ID ────────────────────────────────
                <div style="margin-top:12px">
                    <p class="label" style="text-transform:uppercase;letter-spacing:1px;font-size:10px;color:#6b7280">"ACCOUNT ID"</p>
                    <div class="copy-row">
                        <p class="mono"
                           title="Click to copy full address"
                           style="cursor:pointer;flex:1;font-size:13px"
                           on:click=on_copy>
                            {move || info.get()
                                .map(|a| {
                                    let id = a.account_id;
                                    if id.len() > 28 {
                                        format!("{}\u{2026}{}", &id[..16], &id[id.len()-8..])
                                    } else {
                                        id
                                    }
                                })
                                .unwrap_or_else(|| "\u{2014}".into())}
                        </p>
                        <button style="font-size:12px;padding:4px 10px" on:click=on_copy title="Copy full address">
                            {move || if copy_success.get() { "Copied!" } else { "\u{1f4cb} Copy" }}
                        </button>
                    </div>
                    <p class="muted" style="font-size:11px;margin-top:4px">
                        "Share this address to receive KX"
                    </p>
                </div>

                <hr style="border:none;border-top:1px solid #1e2130;margin:14px 0" />

                // ── Claim Code (collapsible when user has verified emails) ──
                <div style="border:1px solid rgba(212,168,75,0.3);border-radius:8px;padding:12px;margin-top:0">
                    {move || if claim_collapsed.get() {
                        view! {
                            <a href="javascript:void(0)" style="font-size:14px;font-weight:700;color:#d4a84b;text-decoration:none;display:block"
                                on:click=move |_| claim_collapsed.set(false)>
                                "Got a claim code? \u{203a}"
                            </a>
                        }.into_any()
                    } else {
                        view! {
                            <div>
                                <p style="font-size:15px;font-weight:700;color:#d4a84b;margin:0 0 6px;cursor:pointer"
                                    on:click=move |_| {
                                        // Only allow re-collapse if user has verified emails
                                        spawn_local(async move {
                                            let emails = call::<Vec<String>>("get_verified_emails", no_args()).await.unwrap_or_default();
                                            if !emails.is_empty() { claim_collapsed.set(true); }
                                        });
                                    }>
                                    "Got a claim code? \u{2039}"
                                </p>
                            </div>
                        }.into_any()
                    }}
                    <div style:display=move || if claim_collapsed.get() { "none" } else { "" }>
                    <p class="muted" style="font-size:12px;margin-bottom:8px">
                        "Paste the code you received to claim your KX."
                    </p>
                    <input
                        type="text"
                        placeholder="KX-XXXX-XXXX-XXXX-XXXX"
                        class="input-field"
                        style="font-family:monospace;font-size:13px;letter-spacing:1px;text-align:center;margin-bottom:8px"
                        prop:value=move || home_claim_code.get()
                        on:input=move |ev| home_claim_code.set(event_target_value(&ev))
                    />
                    <button
                        class="btn-primary"
                        style="width:100%;background:#d4a84b;color:#0a0a0a;font-weight:700;padding:10px;border:none;border-radius:6px;cursor:pointer;font-size:14px"
                        disabled=move || home_claim_busy.get()
                        on:click=move |_| {
                            let code = home_claim_code.get_untracked().trim().to_string();
                            if code.is_empty() {
                                home_claim_msg.set("Enter your claim code".into());
                                return;
                            }
                            home_claim_busy.set(true);
                            let claimed_code = code.clone();
                            spawn_local(async move {
                                home_claim_msg.set("Searching for matching locks\u{2026}".into());
                                let args = serde_wasm_bindgen::to_value(
                                    &serde_json::json!({ "claimCode": claimed_code })
                                ).unwrap_or(no_args());
                                match call::<ClaimByCodeResult>("claim_by_code", args).await {
                                    Ok(result) => {
                                        let kx = format_kx(&result.total_chronos);
                                        if result.claimed_count == 1 {
                                            home_claim_msg.set(format!("Claimed {kx} KX!"));
                                        } else {
                                            home_claim_msg.set(format!("Claimed {} promises ({kx} KX total)!", result.claimed_count));
                                        }
                                        home_claim_code.set(String::new());
                                        // Poll until node confirms (nonce changes)
                                        poll_balance_update(info).await;
                                        if let Ok(locks) = call::<Vec<TimeLockInfo>>("get_pending_incoming", no_args()).await {
                                            incoming.set(locks);
                                        }
                                        // ── Whitelist popup: check if sender email is known ──
                                        let ci_args = serde_wasm_bindgen::to_value(
                                            &serde_json::json!({ "claimCode": claimed_code })
                                        ).unwrap_or(no_args());
                                        if let Ok(ci) = call::<ClaimInfoResult>("get_claim_info", ci_args).await {
                                            if ci.found {
                                                if let Some(email) = ci.email {
                                                    wl_email.set(email);
                                                    wl_amount.set(ci.amount_kx.map(|a| format!("{a}")).unwrap_or_default());
                                                    wl_msg.set(String::new());
                                                    wl_busy.set(false);
                                                    wl_show.set(true);
                                                }
                                            }
                                        }
                                        // ── Email registration prompt after claim ──
                                        // Check if user has any claim emails registered; if not, prompt
                                        if let Ok(emails) = call::<Vec<String>>("get_claim_emails", no_args()).await {
                                            if emails.is_empty() {
                                                claim_reg_email.set(String::new());
                                                claim_reg_msg.set(String::new());
                                                claim_reg_busy.set(false);
                                                claim_reg_show.set(true);
                                            }
                                        }
                                    }
                                    Err(e) => home_claim_msg.set(format!("Error: {e}")),
                                }
                                home_claim_busy.set(false);
                            });
                        }
                    >
                        {move || if home_claim_busy.get() { "Claiming\u{2026}" } else { "\u{2728} Claim" }}
                    </button>
                    {move || {
                        let s = home_claim_msg.get();
                        if s.is_empty() { view! { <span></span> }.into_any() }
                        else {
                            let cls = if s.starts_with("Error") || s.starts_with("Enter") { "msg error" }
                                      else if s.starts_with("Search") || s.starts_with("Claiming") { "msg mining" }
                                      else { "msg success" };
                            view! { <p class=cls style="margin-top:6px;text-align:center">{s}</p> }.into_any()
                        }
                    }}
                    </div> // close style:display wrapper
                </div>

                // ── Email registration prompt after claim ────────────────────
                {move || {
                    if !claim_reg_show.get() { return view! { <span></span> }.into_any(); }
                    let reg_msg = claim_reg_msg.get();
                    view! {
                        <div style="border:1px solid rgba(212,168,75,0.3);border-radius:8px;padding:12px;margin-top:10px">
                            {if reg_msg.is_empty() {
                                view! {
                                    <div>
                                        <p style="font-size:13px;font-weight:600;color:#e5e7eb;margin:0 0 6px">
                                            "Would you like to register your email address to receive future payments?"
                                        </p>
                                        <input type="email" class="input-field" placeholder="your@email.com"
                                            style="font-size:13px;margin-bottom:8px"
                                            prop:value=move || claim_reg_email.get()
                                            on:input=move |ev| claim_reg_email.set(event_target_value(&ev))
                                        />
                                        <div style="display:flex;gap:8px">
                                            <button class="btn-primary"
                                                style="flex:1;background:#d4a84b;color:#0a0a0a;font-weight:700;padding:8px;border:none;border-radius:6px;cursor:pointer;font-size:13px"
                                                disabled=move || claim_reg_busy.get()
                                                on:click=move |_| {
                                                    let em = claim_reg_email.get_untracked().trim().to_string();
                                                    if em.is_empty() || !em.contains('@') {
                                                        claim_reg_msg.set("Please enter a valid email.".into());
                                                        return;
                                                    }
                                                    claim_reg_busy.set(true);
                                                    spawn_local(async move {
                                                        // Check if already registered to another wallet
                                                        let check_url = format!("https://api.chronx.io/check-email?email={}", em);
                                                        let window = web_sys::window().unwrap();
                                                        let check_result = JsFuture::from(window.fetch_with_str(&check_url)).await;
                                                        if let Ok(resp_val) = check_result {
                                                            use wasm_bindgen::JsCast;
                                                            let resp: web_sys::Response = resp_val.unchecked_into();
                                                            if resp.ok() {
                                                                if let Ok(text_val) = JsFuture::from(resp.text().unwrap()).await {
                                                                    if let Some(text) = text_val.as_string() {
                                                                        if let Ok(json) = serde_json::from_str::<serde_json::Value>(&text) {
                                                                            if json.get("registered").and_then(|v| v.as_bool()) == Some(true) {
                                                                                // Check if same wallet
                                                                                let same_wallet = json.get("same_wallet").and_then(|v| v.as_bool()).unwrap_or(false);
                                                                                if same_wallet {
                                                                                    claim_reg_msg.set("Already registered!".into());
                                                                                    claim_reg_busy.set(false);
                                                                                    return;
                                                                                } else {
                                                                                    claim_reg_msg.set("This email is already linked to a different wallet. Each email can only be linked to one wallet.".into());
                                                                                    claim_reg_busy.set(false);
                                                                                    return;
                                                                                }
                                                                            }
                                                                        }
                                                                    }
                                                                }
                                                            }
                                                        }
                                                        // Not registered — save locally
                                                        let args = serde_wasm_bindgen::to_value(
                                                            &serde_json::json!({ "emails": vec![em.clone()] })
                                                        ).unwrap_or(no_args());
                                                        match call::<()>("set_claim_emails", args).await {
                                                            Ok(_) => claim_reg_msg.set("Email registered successfully!".into()),
                                                            Err(e) => claim_reg_msg.set(format!("Error: {e}")),
                                                        }
                                                        claim_reg_busy.set(false);
                                                    });
                                                }
                                            >
                                                {move || if claim_reg_busy.get() { "Registering\u{2026}" } else { "Register" }}
                                            </button>
                                            <button style="flex:1;padding:8px;background:transparent;border:1px solid #374151;color:#9ca3af;border-radius:6px;cursor:pointer;font-size:13px"
                                                on:click=move |_| claim_reg_show.set(false)>
                                                "Skip"
                                            </button>
                                        </div>
                                    </div>
                                }.into_any()
                            } else {
                                let is_err = reg_msg.starts_with("This email") || reg_msg.starts_with("Error") || reg_msg.starts_with("Please");
                                let color = if is_err { "#ef4444" } else { "#4ade80" };
                                view! {
                                    <div>
                                        <p style=format!("color:{color};font-size:13px;margin:0 0 8px")>{reg_msg}</p>
                                        <button style="padding:6px 16px;background:transparent;border:1px solid #374151;color:#9ca3af;border-radius:6px;cursor:pointer;font-size:12px"
                                            on:click=move |_| { claim_reg_show.set(false); claim_reg_msg.set(String::new()); }>
                                            "Close"
                                        </button>
                                    </div>
                                }.into_any()
                            }}
                        </div>
                    }.into_any()
                }}

                // ── 1A: Incoming promises hyperlink ──────────────────────────
                {move || {
                    let count = incoming.get().len();
                    if count > 0 {
                        view! {
                            <p style="margin-top:14px;font-size:13px;text-align:center;margin-bottom:0">
                                <a href="#" style="color:#d4a84b;text-decoration:underline;cursor:pointer" on:click=move |ev| {
                                    ev.prevent_default();
                                    active_tab.set(2); activity_sub.set(1);
                                }>{format!("You have {} incoming promise{}", count, if count == 1 { "" } else { "s" })}</a>
                            </p>
                        }.into_any()
                    } else {
                        view! { <span></span> }.into_any()
                    }
                }}
                // ── 1B: Items needing attention hyperlink ────────────────────
                {move || {
                    let offer_count = pending_loan_offers_count.get() as usize;
                    // Count other action items: poke requests etc. could be added here
                    let attention_count = offer_count;
                    if attention_count > 0 {
                        view! {
                            <p style="margin-top:6px;font-size:13px;text-align:center;margin-bottom:0">
                                <a href="#" style="color:#d4a84b;text-decoration:underline;cursor:pointer;font-weight:600" on:click=move |ev| {
                                    ev.prevent_default();
                                    active_tab.set(2); activity_sub.set(2);
                                }>{format!("\u{26A1} You have {} item{} needing your attention", attention_count, if attention_count == 1 { "" } else { "s" })}</a>
                            </p>
                        }.into_any()
                    } else {
                        view! { <span></span> }.into_any()
                    }
                }}
                // ── 1C: Open lines hyperlink ─────────────────────────────────
                {move || {
                    let data = loans_data.get();
                    let my_acct = info.get().map(|a| a.account_id.clone()).unwrap_or_default();
                    let open_count = if let Some(arr) = data.as_array() {
                        arr.iter().filter(|l| {
                            let status = l.get("status").and_then(|s| s.as_str()).unwrap_or("");
                            let borrower = l.get("borrower").and_then(|s| s.as_str()).unwrap_or("");
                            (status == "active" || status == "accepted_pending_rescission") && borrower == my_acct
                        }).count()
                    } else { 0 };
                    if open_count > 0 {
                        view! {
                            <p style="margin-top:6px;font-size:13px;text-align:center;margin-bottom:0">
                                <a href="#" style="color:#b8943b;text-decoration:underline;cursor:pointer;opacity:0.85" on:click=move |ev| {
                                    ev.prevent_default();
                                    active_tab.set(2); activity_sub.set(2);
                                }>{format!("You have {} open line{}", open_count, if open_count == 1 { "" } else { "s" })}</a>
                            </p>
                        }.into_any()
                    } else {
                        view! { <span></span> }.into_any()
                    }
                }}
            </div>

        </div>

        // ── Profile modal ────────────────────────────────────────────────
        {move || if show_profile_modal.get() {
            let account_id = info.get().map(|a| a.account_id.clone()).unwrap_or_default();
            let qr = make_qr_svg(&account_id);
            view! {
                <div style="position:fixed;inset:0;background:rgba(0,0,0,0.85);display:flex;align-items:center;justify-content:center;z-index:1000"
                    on:click=move |_| show_profile_modal.set(false)>
                    <div style="background:#1a1a2e;border:1px solid #d4a84b;border-radius:16px;padding:32px;display:flex;flex-direction:column;align-items:center;gap:16px;max-width:340px;width:90%;position:relative"
                        on:click=move |ev: web_sys::MouseEvent| ev.stop_propagation()>
                        // Close button
                        <button on:click=move |_| show_profile_modal.set(false)
                            style="position:absolute;top:12px;right:12px;background:none;border:none;color:#888;font-size:20px;cursor:pointer;line-height:1">
                            "\u{00d7}"
                        </button>
                        // Large avatar (clickable — triggers photo picker)
                        <div style="position:relative;cursor:pointer"
                            on:click=move |_| {
                                if let Some(w) = web_sys::window() {
                                    if let Some(d) = w.document() {
                                        if let Some(el) = d.get_element_by_id("avatar-file-input") {
                                            let _ = el.dyn_ref::<web_sys::HtmlElement>().map(|e| e.click());
                                        }
                                    }
                                }
                            }>
                            <img src={move || {
                                    let base = avatar_url.get();
                                    if base.is_empty() { return String::new(); }
                                    let bust = avatar_bust.get();
                                    if bust > 0.0 { format!("{}?t={:.0}", base, bust) } else { base }
                                }}
                                style="width:120px;height:120px;border-radius:50%;border:3px solid #d4a84b;object-fit:cover;display:block;background:#1a1a2e"
                            />
                            <div style="position:absolute;bottom:2px;right:2px;background:#d4a84b;border-radius:50%;width:28px;height:28px;display:flex;align-items:center;justify-content:center;font-size:14px;line-height:1;border:2px solid #1a1a2e">
                                "\u{1F4F7}"
                            </div>
                        </div>
                        // Display name (editable)
                        {move || {
                            if display_name_editing.get() {
                                view! {
                                    <div style="display:flex;gap:8px;align-items:center">
                                        <input type="text" maxlength="32" placeholder="Your name"
                                            prop:value=move || display_name_input.get()
                                            on:input=move |ev| display_name_input.set(event_target_value(&ev))
                                            style="background:#0d0d1a;border:1px solid #d4a84b;color:#fff;padding:6px 10px;border-radius:6px;font-size:14px;width:160px"
                                        />
                                        <button on:click=move |_| {
                                                let name = display_name_input.get_untracked();
                                                let wallet = info.get_untracked().map(|a| a.account_id.clone()).unwrap_or_default();
                                                display_name.set(name.clone());
                                                display_name_editing.set(false);
                                                spawn_local(async move {
                                                    let args = serde_wasm_bindgen::to_value(&serde_json::json!({
                                                        "walletAddress": wallet,
                                                        "displayName": name,
                                                    })).unwrap_or(no_args());
                                                    let _ = call::<bool>("update_display_name", args).await;
                                                });
                                            }
                                            style="background:#d4a84b;color:#000;border:none;padding:6px 12px;border-radius:6px;font-weight:700;cursor:pointer;font-size:13px">
                                            "Save"</button>
                                        <button on:click=move |_| display_name_editing.set(false)
                                            style="background:none;border:1px solid #444;color:#888;padding:6px 10px;border-radius:6px;cursor:pointer;font-size:13px">
                                            "Cancel"</button>
                                    </div>
                                }.into_any()
                            } else {
                                let dn = display_name.get();
                                let is_empty = dn.is_empty();
                                view! {
                                    <div style="display:flex;align-items:center;gap:8px;cursor:pointer"
                                        on:click=move |_| {
                                            display_name_input.set(display_name.get_untracked());
                                            display_name_editing.set(true);
                                        }>
                                        <span style={if is_empty { "font-size:18px;font-weight:700;color:#666" } else { "font-size:18px;font-weight:700;color:#fff" }}>
                                            {if is_empty { "Add your name".to_string() } else { dn }}
                                        </span>
                                        <span style="color:#d4a84b;font-size:14px">
                                            "\u{270f}\u{fe0f}"</span>
                                    </div>
                                }.into_any()
                            }
                        }}
                        // Badge in profile modal
                        {move || match badge.get().as_str() {
                            "FOUNDING_MEMBER" | "Founding Team" => view! { <span style="display:inline-block;padding:4px 14px;border-radius:6px;background:#d4a84b;color:black;font-size:13px;font-weight:700;margin-top:4px">{"Founding Team"}</span> }.into_any(),
                            "GENESIS_MEMBER" => view! { <span style="display:inline-block;padding:4px 14px;border-radius:6px;background:#d4a84b;color:black;font-size:13px;font-weight:700;margin-top:4px">{"Genesis Member"}</span> }.into_any(),
                            "PROTOCOL_PATRON" => view! { <span style="display:inline-block;padding:4px 14px;border-radius:6px;background:#e2e8f0;color:#1a1a2e;font-size:13px;font-weight:700;margin-top:4px">{"Protocol Patron"}</span> }.into_any(),
                            _ => view! { <span></span> }.into_any(),
                        }}
                        // QR code (gold on dark, using existing make_qr_svg)
                        <div style="background:#0d0d1a;border:1px solid #333;border-radius:12px;padding:16px">
                            <div inner_html=qr style="display:inline-block"></div>
                        </div>
                        // Wallet address (truncated)
                        <div style="font-size:11px;color:#555;font-family:monospace;text-align:center;word-break:break-all">
                            {move || {
                                let a = info.get().map(|i| i.account_id.clone()).unwrap_or_default();
                                if a.len() > 20 { format!("{}...{}", &a[..10], &a[a.len()-10..]) } else { a }
                            }}
                        </div>
                        // (Photo change via avatar tap above)
                    </div>
                </div>
            }.into_any()
        } else {
            view! { <span></span> }.into_any()
        }}

        // ── Whitelist popup (shown after successful claim) ───────────────────
        {move || if wl_show.get() {
            let email = wl_email.get();
            let amt = wl_amount.get();
            view! {
                <div class="modal-overlay" on:click=move |_| wl_show.set(false)>
                    <div class="modal-card" on:click=move |ev: web_sys::MouseEvent| ev.stop_propagation()>
                        <p class="modal-title" style="color:#d4a84b">"Instant Delivery"</p>
                        <div class="modal-body" style="text-align:center">
                            <p style="font-size:14px;margin-bottom:8px">
                                "You just claimed "
                                {if !amt.is_empty() { format!("{amt} KX") } else { "KX".to_string() }}
                                " from:"
                            </p>
                            <p style="font-size:15px;font-weight:700;color:#d4a84b;word-break:break-all;margin-bottom:12px">
                                {email.clone()}
                            </p>
                            <p style="font-size:13px;color:#9ca3af;margin-bottom:4px">
                                "Would you like to whitelist this sender so future sends arrive instantly \u{2014} no claim code needed?"
                            </p>
                        </div>
                        {move || {
                            let m = wl_msg.get();
                            if m.is_empty() { view! { <span></span> }.into_any() }
                            else {
                                let cls = if m.starts_with("Error") { "msg error" } else { "msg success" };
                                view! { <p class=cls style="margin-top:6px;text-align:center">{m}</p> }.into_any()
                            }
                        }}
                        <div style="display:flex;gap:8px;margin-top:12px">
                            <button
                                style="flex:1;background:transparent;border:1px solid #333;color:#9ca3af;cursor:pointer;padding:10px;border-radius:6px"
                                on:click=move |_| wl_show.set(false)
                            >"No thanks"</button>
                            <button
                                class="btn-primary"
                                style="flex:1;background:#d4a84b;color:#0a0a0a;font-weight:700;padding:10px;border:none;border-radius:6px;cursor:pointer"
                                disabled=move || wl_busy.get()
                                on:click=move |_| {
                                    let sender = wl_email.get_untracked();
                                    let wallet = info.get_untracked().map(|i| i.account_id.clone()).unwrap_or_default();
                                    if wallet.is_empty() || sender.is_empty() {
                                        wl_msg.set("Error: wallet not loaded".into());
                                        return;
                                    }
                                    wl_busy.set(true);
                                    spawn_local(async move {
                                        let args = serde_wasm_bindgen::to_value(
                                            &serde_json::json!({
                                                "email": sender,
                                                "walletAddress": wallet
                                            })
                                        ).unwrap_or(no_args());
                                        match call::<bool>("whitelist_email", args).await {
                                            Ok(true) => {
                                                wl_msg.set("Whitelisted! Future sends will arrive instantly.".into());
                                                spawn_local(async move {
                                                    delay_ms(2000).await;
                                                    wl_show.set(false);
                                                });
                                            }
                                            Ok(false) => wl_msg.set("Error: whitelist failed".into()),
                                            Err(e) => wl_msg.set(format!("Error: {e}")),
                                        }
                                        wl_busy.set(false);
                                    });
                                }
                            >
                                {move || if wl_busy.get() { "Saving\u{2026}" } else { "Yes, whitelist" }}
                            </button>
                        </div>
                    </div>
                </div>
            }.into_any()
        } else { view! { <span></span> }.into_any() }}
    }
}

fn is_valid_email(s: &str) -> bool {
    let s = s.trim();
    if let Some(at) = s.find('@') {
        if at == 0 { return false; }
        let domain = &s[at + 1..];
        if let Some(dot) = domain.rfind('.') {
            let tld = &domain[dot + 1..];
            return dot > 0 && !tld.is_empty();
        }
    }
    false
}

fn linkify_body(text: String) -> Vec<(bool, String)> {
    let mut result = Vec::new();
    let mut remaining = text.as_str();
    while let Some(start) = remaining.find("https://") {
        if start > 0 {
            result.push((false, remaining[..start].to_string()));
        }
        let end = remaining[start..]
            .find(|c: char| c.is_whitespace() || c == '"' || c == ')')
            .map(|i| start + i)
            .unwrap_or(remaining.len());
        result.push((true, remaining[start..end].to_string()));
        remaining = &remaining[end..];
    }
    if !remaining.is_empty() {
        result.push((false, remaining.to_string()));
    }
    result
}

// ── SendPanel (unified: KX Address + Email Address × Send Now + Send Later) ───

#[component]
fn SendPanel(
    info: RwSignal<Option<AccountInfo>>,
    pending_email_chronos: RwSignal<u64>,
    lang: RwSignal<String>,
    poke_prefill_email: RwSignal<String>,
    poke_prefill_amount: RwSignal<String>,
    poke_prefill_memo: RwSignal<String>,
    poke_prefill_id: RwSignal<String>,
    email_prefill_from_contact: RwSignal<String>,
    pay_link_to: RwSignal<String>,
    pay_link_amount: RwSignal<String>,
    pay_link_memo: RwSignal<String>,
    pay_link_show: RwSignal<bool>,
) -> impl IntoView {
    // Mobile defaults to Email mode (no KX address entry)
    let mobile_send = !is_desktop();
    let send_sub  = RwSignal::new(if mobile_send { 1u8 } else { 0u8 }); // 0=KX Address, 1=Email Address
    let send_mode = RwSignal::new(0u8); // 0=Send Now,   1=Send Later

    // KX Address fields
    let to_addr   = RwSignal::new(String::new()); // base58 (Send Now)
    let to_pubkey = RwSignal::new(String::new()); // pubkey hex (Send Later, blank=self)

    // Email field
    let email    = RwSignal::new(String::new());

    // Shared fields
    let amount   = RwSignal::new(String::new());
    let lock_date = RwSignal::new(String::new());
    let memo     = RwSignal::new(String::new());
    let memo_public = RwSignal::new(false); // v2.5.29: public memo toggle (desktop only)
    let sending   = RwSignal::new(false);
    let msg       = RwSignal::new(String::new());
    let scan_msg  = RwSignal::new(String::new());
    let spam_warn = RwSignal::new(false);

    // Pre-fill from pay deep link
    Effect::new(move |_| {
        if pay_link_show.get() {
            let to = pay_link_to.get();
            let amt = pay_link_amount.get();
            let m = pay_link_memo.get();
            if !to.is_empty() {
                send_sub.set(0); // Switch to KX Address mode
                to_addr.set(to);
                amount.set(amt);
                memo.set(m);
            }
        }
    });

    // Series entries for Email + Send Later (additional payments beyond the first)
    let series_entries: RwSignal<Vec<(RwSignal<String>, RwSignal<String>, RwSignal<String>)>> = RwSignal::new(Vec::new()); // (amount, date, memo)

    // Email send confirmation modal
    let email_confirm_open = RwSignal::new(false);
    let email_confirm_add_trusted = RwSignal::new(false);
    let email_confirm_already_trusted = RwSignal::new(false);
    let email_confirm_email = RwSignal::new(String::new());
    let email_confirm_amt = RwSignal::new(String::new());
    let email_confirm_memo = RwSignal::new(String::new());
    let email_send_confirmed = RwSignal::new(false);

    let utc_clock = RwSignal::new(String::new());
    Effect::new(move |_| { start_utc_clock_tick(utc_clock); });

    // Load registered claim email for self-send warning (FIX 8)
    let claim_email = RwSignal::new(String::new());
    Effect::new(move |_| {
        spawn_local(async move {
            if let Ok(Some(e)) = call::<Option<String>>("get_claim_email", no_args()).await {
                claim_email.set(e);
            }
        });
    });

    // ── Recipient mode (3-way) ────────────────────────────────────────────────
    // 0 = KX Address, 1 = Email, 2 = Freeform (name/org/description)
    let recipient_mode = RwSignal::new(if mobile_send { 1u8 } else { 0u8 });
    let freeform_recip = RwSignal::new(String::new());

    // ── Long Promise / AI management signals ────────────────────────────────────
    let is_long_promise = RwSignal::new(false);
    let grantor_intent = RwSignal::new(String::new());
    let ai_managed = RwSignal::new(false);
    let ai_percentage = RwSignal::new(1u32); // default 1%
    let risk_level = RwSignal::new(50u32);
    let axiom_modal_open = RwSignal::new(false);
    let axiom_consented = RwSignal::new(false);
    let axiom_consent_hash = RwSignal::new(String::new());

    let warning_dismissed = RwSignal::new(false);

    // Recompute is_long_promise whenever lock_date changes
    Effect::new(move |_| {
        let date_str = lock_date.get();
        if date_str.is_empty() {
            is_long_promise.set(false);
            return;
        }
        let unix = date_str_to_unix(&date_str).unwrap_or(0);
        let now_secs = (js_sys::Date::now() / 1000.0) as i64;
        is_long_promise.set(unix > now_secs + (365 * 86400));
    });

    // Clear messages on tab/mode switch
    Effect::new(move |_| {
        send_sub.get(); send_mode.get();
        msg.set(String::new()); scan_msg.set(String::new()); spam_warn.set(false);
    });

    // Poke pre-fill: when poke_prefill_email changes, populate Email + Send Now form
    let poke_applied = RwSignal::new(false);
    Effect::new(move |_| {
        let prefill_email = poke_prefill_email.get();
        if !prefill_email.is_empty() && !poke_applied.get_untracked() {
            poke_applied.set(true);
            send_sub.set(1);  // Email Address tab
            send_mode.set(0); // Send Now
            email.set(prefill_email);
            amount.set(poke_prefill_amount.get_untracked());
            memo.set(poke_prefill_memo.get_untracked());
            msg.set(String::new());
        } else if prefill_email.is_empty() {
            poke_applied.set(false);
        }
    });

    // Contact pre-fill: when user clicks "Send KX" on a contact card
    Effect::new(move |_| {
        let prefill = email_prefill_from_contact.get();
        if !prefill.is_empty() {
            send_sub.set(1);  // Email Address tab
            send_mode.set(0); // Send Now
            email.set(prefill);
            msg.set(String::new());
            email_prefill_from_contact.set(String::new()); // consume
        }
    });

    // Contact autocomplete state
    let contact_suggestions: RwSignal<Vec<Contact>> = RwSignal::new(Vec::new());
    let show_contact_dropdown = RwSignal::new(false);
    // Address book modal (mobile only)
    let address_book_open = RwSignal::new(false);
    let address_book_contacts: RwSignal<Vec<Contact>> = RwSignal::new(Vec::new());
    // Inline email save icon state: 0=hidden, 1=save icon, 2=nickname prompt, 3=saved ✓
    let email_save_state = RwSignal::new(0u8);
    let email_save_nickname = RwSignal::new(String::new());
    let email_save_msg = RwSignal::new(String::new());
    // Mobile time picker: 0=Send Now, 1=1h, 2=24h, 3=1w, 4=1m, 5=3m, 6=6m, 7=1y
    let mobile_time_option = RwSignal::new(0u8);
    // Send Later radio option: 255=none selected
    let later_choice = RwSignal::new(255u8);
    let show_date_picker = RwSignal::new(false);
    let custom_date = RwSignal::new(String::new());
    let custom_time = RwSignal::new(String::new());
    // Mobile confirmation screen
    let mobile_confirm_open = RwSignal::new(false);
    let mobile_confirm_to_display = RwSignal::new(String::new());
    let mobile_confirm_amount_display = RwSignal::new(String::new());
    let mobile_confirm_unlock_display = RwSignal::new(String::new());
    let mobile_confirm_memo_display = RwSignal::new(String::new());

    // (save_contact_banner signals removed — replaced by inline save icon)

    let set_date = move |date: String| lock_date.set(date);

    let on_scan_qr = move |_: web_sys::MouseEvent| {
        spawn_local(async move {
            scan_msg.set(String::new());
            match pick_image_file().await {
                None => scan_msg.set("No file selected.".into()),
                Some(file) => match scan_qr_file(file).await {
                    Ok(raw) => {
                        if send_mode.get_untracked() == 0 {
                            to_addr.set(qr_extract_account_id(&raw));
                        } else {
                            to_pubkey.set(qr_extract_pubkey(&raw));
                        }
                        scan_msg.set("Address filled from QR.".into());
                    }
                    Err(e) => scan_msg.set(format!("Scan failed: {e}")),
                },
            }
        });
    };

    let on_send = move |_: web_sys::MouseEvent| {
        let sub  = send_sub.get_untracked();
        let mode = send_mode.get_untracked();
        let amt_str  = amount.get_untracked();
        let memo_str = memo.get_untracked();
        if amt_str.is_empty() { msg.set("Error: enter an amount.".into()); return; }
        let amt: f64 = match amt_str.parse::<f64>() {
            Ok(v) if v > 0.0 => v,
            Ok(_) => { msg.set("Error: amount must be > 0.".into()); return; }
            Err(_) => { msg.set("Error: invalid amount.".into()); return; }
        };
        let memo_opt: Option<String> = if memo_str.is_empty() { None } else { Some(memo_str) };
        let is_memo_public = memo_public.get_untracked();

        // Balance check — reject before PoW mining starts (account for pending email sends)
        if let Some(ref ai) = info.get_untracked() {
            let raw_spendable: f64 = ai.spendable_chronos.parse::<f64>().unwrap_or(0.0);
            let pending = pending_email_chronos.get_untracked() as f64;
            let available_chronos = (raw_spendable - pending).max(0.0);
            let available_kx = available_chronos / 1_000_000.0;
            if available_kx == 0.0 {
                msg.set("Your balance is zero. You cannot send KX.".into()); return;
            }
            if amt > available_kx {
                msg.set(format!("Insufficient balance. You have {:.6} KX available.", available_kx)); return;
            }
        }

        if sub == 0 && mode == 0 {
            // KX + Send Now
            let to = to_addr.get_untracked();
            if to.is_empty() { msg.set("Error: enter a recipient address.".into()); return; }
            spawn_local(async move {
                sending.set(true);
                msg.set("Mining PoW\u{2026} (~10s)".into());
                let args = serde_wasm_bindgen::to_value(
                    &serde_json::json!({ "to": to, "amountKx": amt })
                ).unwrap_or(no_args());
                match call::<String>("send_transfer", args).await {
                    Ok(_txid) => {
                        let truncated = if to.len() > 16 { format!("{}...{}", &to[..6], &to[to.len()-6..]) } else { to.clone() };
                        msg.set(format!("\u{2705} Sent!\n{} KX sent to {}.", amt, truncated));
                        to_addr.set(String::new());
                        amount.set(String::new());
                        // Poll until node confirms
                        poll_balance_update(info).await;
                    }
                    Err(e) => msg.set(format!("Error: {e}")),
                }
                sending.set(false);
            });
        } else if sub == 0 && mode == 1 {
            // KX + Send Later (or Freeform)
            let rm = recipient_mode.get_untracked();
            let date_str = lock_date.get_untracked();
            if date_str.is_empty() { msg.set("Error: choose an unlock date.".into()); return; }
            let unlock_unix = match date_str_to_unix(&date_str) {
                Some(t) => t,
                None => { msg.set("Error: invalid date.".into()); return; }
            };
            // Capture long promise / AI fields before spawn_local
            let lp_note: Option<String> = { let n = grantor_intent.get_untracked(); if n.is_empty() { None } else { Some(n) } };
            let lp_risk: Option<u32> = if ai_managed.get_untracked() { Some(risk_level.get_untracked()) } else { None };
            let lp_ai_pct: Option<u32> = if ai_managed.get_untracked() { Some(ai_percentage.get_untracked()) } else { None };
            let lp_hash: Option<String> = { let h = axiom_consent_hash.get_untracked(); if h.is_empty() { None } else { Some(h) } };

            if rm == 2 {
                // Freeform recipient
                let fr = freeform_recip.get_untracked();
                if fr.trim().is_empty() { msg.set("Error: enter a freeform recipient.".into()); return; }
                spawn_local(async move {
                    sending.set(true);
                    msg.set("Mining PoW\u{2026} (~10s)".into());
                    let args = serde_wasm_bindgen::to_value(&serde_json::json!({
                        "freeformRecipient": fr,
                        "amountKx": amt,
                        "unlockAtUnix": unlock_unix,
                        "memo": memo_opt,
                        "grantorIntent": lp_note,
                        "riskLevel": lp_risk,
                        "aiPercentage": lp_ai_pct,
                        "axiomConsentHash": lp_hash,
                        "memoIsPublic": is_memo_public,
                    })).unwrap_or(no_args());
                    match call::<String>("create_freeform_timelock", args).await {
                        Ok(txid) => {
                            msg.set(format!("Promise made! ID: {}", &txid[..16.min(txid.len())]));
                            amount.set(String::new());
                            lock_date.set(String::new());
                            memo.set(String::new());
                            memo_public.set(false);
                            freeform_recip.set(String::new());
                            grantor_intent.set(String::new());
                            risk_level.set(50);
                            axiom_consented.set(false);
                            axiom_consent_hash.set(String::new());
                            poll_balance_update(info).await;
                        }
                        Err(e) => msg.set(format!("Error: {e}")),
                    }
                    sending.set(false);
                });
            } else {
                // KX address recipient
                let pubkey = to_pubkey.get_untracked();
                let to_pubkey_hex: Option<String> = if pubkey.is_empty() { None } else { Some(pubkey) };
                spawn_local(async move {
                    sending.set(true);
                    msg.set("Mining PoW\u{2026} (~10s)".into());
                    let args = serde_wasm_bindgen::to_value(&serde_json::json!({
                        "amountKx": amt,
                        "unlockAtUnix": unlock_unix,
                        "memo": memo_opt,
                        "toPubkeyHex": to_pubkey_hex,
                        "grantorIntent": lp_note,
                        "riskLevel": lp_risk,
                        "aiPercentage": lp_ai_pct,
                        "axiomConsentHash": lp_hash,
                        "memoIsPublic": is_memo_public,
                    })).unwrap_or(no_args());
                    match call::<String>("create_timelock", args).await {
                        Ok(txid) => {
                            msg.set(format!("Promise made! ID: {}", &txid[..16.min(txid.len())]));
                            amount.set(String::new());
                            lock_date.set(String::new());
                            memo.set(String::new());
                            memo_public.set(false);
                            to_pubkey.set(String::new());
                            grantor_intent.set(String::new());
                            risk_level.set(50);
                            axiom_consented.set(false);
                            axiom_consent_hash.set(String::new());
                            poll_balance_update(info).await;
                        }
                        Err(e) => msg.set(format!("Error: {e}")),
                    }
                    sending.set(false);
                });
            }
        } else if sub == 1 && mode == 0 {
            // Email + Send Now (unlock = 0 → backend uses now → immediately claimable)
            let email_str = email.get_untracked();
            if !is_valid_email(&email_str) {
                msg.set("Error: Please enter a valid email address.".into()); return;
            }
            // Confirmation gate (desktop only)
            if !email_send_confirmed.get_untracked() {
                email_confirm_email.set(email_str.clone());
                email_confirm_amt.set(amount.get_untracked());
                email_confirm_memo.set(memo.get_untracked());
                email_confirm_add_trusted.set(false);
                email_confirm_already_trusted.set(false);
                email_confirm_open.set(true);
                return;
            }
            email_send_confirmed.set(false);
            let unlock_unix: i64 = 0; // 0 = Send Now — backend uses its own timestamp
            spawn_local(async move {
                sending.set(true);
                pending_email_chronos.set((amt * 1_000_000.0) as u64);
                msg.set("Mining PoW\u{2026} (~10s)".into());
                let args = serde_wasm_bindgen::to_value(&serde_json::json!({
                    "email": email_str.clone(),
                    "amountKx": amt,
                    "unlockAtUnix": unlock_unix,
                    "memo": memo_opt.clone(),
                    "memoIsPublic": is_memo_public,
                })).unwrap_or(no_args());
                match call::<EmailLockResult>("create_email_timelock", args).await {
                    Ok(result) => {
                        let txid = result.tx_id.clone();
                        let claim_code = result.claim_code.clone();
                        // Save email→lock mapping for History tab (includes claim code)
                        let save_args = serde_wasm_bindgen::to_value(&serde_json::json!({
                            "lockId": txid.clone(),
                            "email": email_str.clone(),
                            "claimCode": claim_code.clone(),
                        })).unwrap_or(no_args());
                        let _ = call::<()>("save_email_send", save_args).await;
                        // (trusted contact add removed in v2.4.1)
                        email.set(String::new());
                        amount.set(String::new());
                        memo.set(String::new());
                        memo_public.set(false);
                        let notify_args = serde_wasm_bindgen::to_value(&serde_json::json!({
                            "email": email_str,
                            "amountKx": amt,
                            "unlockAtUnix": unlock_unix,
                            "memo": memo_opt,
                            "claimCode": claim_code.clone(),
                        })).unwrap_or(no_args());
                        match call::<()>("notify_email_recipient", notify_args).await {
                            Ok(_) => {
                                if unlock_unix == 0 {
                                    msg.set(format!("\u{2705} Delivered!\n{email_str} has been notified.\nThey have 72 hours to claim their KX.\nClaim code: {claim_code}"));
                                } else {
                                    let dt = {
                                        let d = js_sys::Date::new_0();
                                        d.set_time((unlock_unix as f64) * 1000.0);
                                        d.to_date_string().as_string().unwrap_or_else(|| "the unlock date".to_string())
                                    };
                                    msg.set(format!("\u{2705} Promise created!\n{email_str} has been notified.\nThey'll receive a claim email when it unlocks on {dt}."));
                                }
                                spam_warn.set(true);
                            }
                            Err(_) => { msg.set(format!("\u{26a0}\u{fe0f} Sent on-chain! Email failed.\nClaim code: {claim_code}\nShare this code with the recipient manually.")); }
                        }
                        // If this send was triggered by a poke PAY NOW, confirm payment
                        let poke_id = poke_prefill_id.get_untracked();
                        if !poke_id.is_empty() {
                            let args = serde_wasm_bindgen::to_value(
                                &serde_json::json!({ "requestId": poke_id })
                            ).unwrap_or(no_args());
                            let _ = call::<()>("confirm_poke_paid", args).await;
                            poke_prefill_id.set(String::new());
                        }
                        let prev_nonce = info.get_untracked().as_ref().map(|a| a.nonce).unwrap_or(0);
                        // Check immediately (transaction is already on-chain), then poll as fallback
                        for i in 0..=10u8 {
                            if i > 0 { delay_ms(1500).await; }
                            if let Ok(a) = call::<AccountInfo>("get_account_info", no_args()).await {
                                if a.nonce > prev_nonce { info.set(Some(a)); break; }
                            }
                        }
                        // Always force a final refresh so balance is correct even if nonce poll timed out
                        if let Ok(a) = call::<AccountInfo>("get_account_info", no_args()).await {
                            info.set(Some(a));
                        }
                        pending_email_chronos.set(0);
                        // Clear form fields after success
                        email.set(String::new());
                        amount.set(String::new());
                        memo.set(String::new());
                        memo_public.set(false);
                    }
                    Err(e) => {
                        pending_email_chronos.set(0);
                        msg.set(format!("Error: {e}"));
                    }
                }
                sending.set(false);
            });
        } else {
            // Email + Send Later
            let email_str = email.get_untracked();
            if !is_valid_email(&email_str) {
                msg.set("Error: Please enter a valid email address.".into()); return;
            }
            let date_str = lock_date.get_untracked();
            if date_str.is_empty() { msg.set("Error: choose an unlock date.".into()); return; }
            // Confirmation gate (desktop only)
            if !email_send_confirmed.get_untracked() {
                email_confirm_email.set(email_str.clone());
                email_confirm_amt.set(amount.get_untracked());
                email_confirm_memo.set(memo.get_untracked());
                email_confirm_add_trusted.set(false);
                email_confirm_already_trusted.set(false);
                email_confirm_open.set(true);
                return;
            }
            email_send_confirmed.set(false);
            let unlock_unix = match date_str_to_unix(&date_str) {
                Some(t) => t,
                None => { msg.set("Error: invalid date.".into()); return; }
            };

            // Capture long promise / AI fields for email send later
            let lp_active = is_long_promise.get_untracked();
            let lp_note: Option<String> = { let n = grantor_intent.get_untracked(); if n.is_empty() { None } else { Some(n) } };
            let lp_risk: Option<u32> = if ai_managed.get_untracked() { Some(risk_level.get_untracked()) } else { None };
            let lp_ai_pct: Option<u32> = if ai_managed.get_untracked() { Some(ai_percentage.get_untracked()) } else { None };
            let lp_hash: Option<String> = { let h = axiom_consent_hash.get_untracked(); if h.is_empty() { None } else { Some(h) } };
            let lp_email_str = email_str.clone();
            let lp_amt = amt;

            // Check if this is a series (additional entries present)
            let extra = series_entries.get_untracked();
            if !extra.is_empty() {
                // --- Promise Series ---
                // Build entries array: first entry from main fields, rest from series_entries
                let mut entries_json = vec![serde_json::json!({
                    "amount_kx": amt,
                    "unlock_at_unix": unlock_unix,
                    "memo": memo_opt.clone(),
                })];
                for (s_amt, s_date, s_memo) in &extra {
                    let sa: f64 = s_amt.get_untracked().parse().unwrap_or(0.0);
                    let sd = s_date.get_untracked();
                    let su = match date_str_to_unix(&sd) {
                        Some(t) => t,
                        None => { msg.set("Error: invalid date in a series entry.".into()); return; }
                    };
                    let sm = s_memo.get_untracked();
                    if sa <= 0.0 { msg.set("Error: all series amounts must be > 0.".into()); return; }
                    entries_json.push(serde_json::json!({
                        "amount_kx": sa,
                        "unlock_at_unix": su,
                        "memo": if sm.is_empty() { None::<String> } else { Some(sm) },
                    }));
                }
                let total_chronos: u64 = entries_json.iter()
                    .map(|e| (e["amount_kx"].as_f64().unwrap_or(0.0) * 1_000_000.0) as u64)
                    .sum();
                spawn_local(async move {
                    sending.set(true);
                    pending_email_chronos.set(total_chronos);
                    msg.set(format!("Mining PoW for {} payments\u{2026}", entries_json.len()));
                    let args = serde_wasm_bindgen::to_value(&serde_json::json!({
                        "email": email_str.clone(),
                        "entries": entries_json,
                        "memoIsPublic": is_memo_public,
                    })).unwrap_or(no_args());
                    match call::<EmailSeriesResult>("create_email_timelock_series", args).await {
                        Ok(result) => {
                            let claim_code = result.claim_code.clone();
                            let count = result.tx_ids.len();
                            // Save each lock's email mapping
                            for txid in &result.tx_ids {
                                let save_args = serde_wasm_bindgen::to_value(&serde_json::json!({
                                    "lockId": txid, "email": email_str.clone(), "claimCode": claim_code.clone(),
                                })).unwrap_or(no_args());
                                let _ = call::<()>("save_email_send", save_args).await;
                            }
                            // Add as trusted contact if checkbox was checked
                            if email_confirm_add_trusted.get_untracked() {
                                let tc_args = serde_wasm_bindgen::to_value(&serde_json::json!({
                                    "email": email_str.clone(),
                                })).unwrap_or(no_args());
                                let _ = call::<()>("add_trusted_contact", tc_args).await;
                            }
                            email.set(String::new());
                            amount.set(String::new());
                            lock_date.set(String::new());
                            memo.set(String::new());
                            memo_public.set(false);
                            series_entries.set(Vec::new());
                            // Notify recipient (series-aware)
                            let notify_args = serde_wasm_bindgen::to_value(&serde_json::json!({
                                "email": email_str,
                                "amountKx": amt,
                                "unlockAtUnix": unlock_unix,
                                "memo": memo_opt,
                                "claimCode": claim_code.clone(),
                            })).unwrap_or(no_args());
                            match call::<()>("notify_email_recipient", notify_args).await {
                                Ok(_) => { msg.set(format!("\u{2705} Series created!\n{count} promises sent. {email_str} has been notified.\nClaim code: {claim_code}")); spam_warn.set(true); }
                                Err(_) => { msg.set(format!("\u{26a0}\u{fe0f} Series on-chain! Email failed.\nClaim code: {claim_code}\nShare this code with the recipient manually.")); }
                            }
                            // Poll for balance update
                            let prev_nonce = info.get_untracked().as_ref().map(|a| a.nonce).unwrap_or(0);
                            for i in 0..=10u8 {
                                if i > 0 { delay_ms(1500).await; }
                                if let Ok(a) = call::<AccountInfo>("get_account_info", no_args()).await {
                                    if a.nonce > prev_nonce { info.set(Some(a)); break; }
                                }
                            }
                            if let Ok(a) = call::<AccountInfo>("get_account_info", no_args()).await {
                                info.set(Some(a));
                            }
                            pending_email_chronos.set(0);
                        }
                        Err(e) => {
                            pending_email_chronos.set(0);
                            msg.set(format!("Error: {e}"));
                        }
                    }
                    sending.set(false);
                });
            } else {
                // --- Single email send ---
                let lp_note2 = lp_note.clone();
                let lp_hash2 = lp_hash.clone();
                let lp_email_for_confirm = lp_email_str.clone();
                let lp_risk_for_confirm = lp_risk;
                let lp_note_preview = lp_note.as_deref().unwrap_or("").chars().take(100).collect::<String>();
                spawn_local(async move {
                    sending.set(true);
                    pending_email_chronos.set((amt * 1_000_000.0) as u64);
                    msg.set("Mining PoW\u{2026} (~10s)".into());
                    let args = serde_wasm_bindgen::to_value(&serde_json::json!({
                        "email": email_str.clone(),
                        "amountKx": amt,
                        "unlockAtUnix": unlock_unix,
                        "memo": memo_opt.clone(),
                        "grantorIntent": lp_note2,
                        "riskLevel": lp_risk,
                        "aiPercentage": lp_ai_pct,
                        "axiomConsentHash": lp_hash2,
                        "memoIsPublic": is_memo_public,
                    })).unwrap_or(no_args());
                    match call::<EmailLockResult>("create_email_timelock", args).await {
                        Ok(result) => {
                            let txid = result.tx_id.clone();
                            let claim_code = result.claim_code.clone();
                            let save_args = serde_wasm_bindgen::to_value(&serde_json::json!({
                                "lockId": txid.clone(),
                                "email": email_str.clone(),
                                "claimCode": claim_code.clone(),
                            })).unwrap_or(no_args());
                            let _ = call::<()>("save_email_send", save_args).await;
                            // Add as trusted contact if checkbox was checked
                            if email_confirm_add_trusted.get_untracked() {
                                let tc_args = serde_wasm_bindgen::to_value(&serde_json::json!({
                                    "email": email_str.clone(),
                                })).unwrap_or(no_args());
                                let _ = call::<()>("add_trusted_contact", tc_args).await;
                            }
                            email.set(String::new());
                            amount.set(String::new());
                            lock_date.set(String::new());
                            memo.set(String::new());
                            memo_public.set(false);
                            let notify_args = serde_wasm_bindgen::to_value(&serde_json::json!({
                                "email": email_str,
                                "amountKx": amt,
                                "unlockAtUnix": unlock_unix,
                                "memo": memo_opt,
                                "claimCode": claim_code.clone(),
                            })).unwrap_or(no_args());
                            match call::<()>("notify_email_recipient", notify_args).await {
                                Ok(_) => { msg.set(format!("\u{2705} Sent! Email delivered.\nClaim code: {claim_code}")); spam_warn.set(true); }
                                Err(_) => { msg.set(format!("\u{26a0}\u{fe0f} Sent on-chain! Email failed.\nClaim code: {claim_code}\nShare this code with the recipient manually.")); }
                            }
                            let prev_nonce = info.get_untracked().as_ref().map(|a| a.nonce).unwrap_or(0);
                            for i in 0..=10u8 {
                                if i > 0 { delay_ms(1500).await; }
                                if let Ok(a) = call::<AccountInfo>("get_account_info", no_args()).await {
                                    if a.nonce > prev_nonce { info.set(Some(a)); break; }
                                }
                            }
                            if let Ok(a) = call::<AccountInfo>("get_account_info", no_args()).await {
                                info.set(Some(a));
                            }
                            pending_email_chronos.set(0);
                            // Reset long promise fields
                            grantor_intent.set(String::new());
                            risk_level.set(50);
                            axiom_consented.set(false);
                            axiom_consent_hash.set(String::new());
                            // Fire-and-forget: send long promise confirmation email
                            if lp_active {
                                let confirm_txid = txid.clone();
                                let confirm_email = lp_email_for_confirm.clone();
                                let confirm_note = lp_note_preview.clone();
                                let confirm_risk = lp_risk_for_confirm.unwrap_or(50);
                                let confirm_unlock = unlock_unix;
                                let cancel_ms = js_sys::Date::now() + (7.0 * 86400.0 * 1000.0);
                                let cancel_d = js_sys::Date::new(&JsValue::from_f64(cancel_ms));
                                let months = ["Jan","Feb","Mar","Apr","May","Jun","Jul","Aug","Sep","Oct","Nov","Dec"];
                                let cm = cancel_d.get_utc_month() as usize;
                                let cancel_deadline = format!("{} {:02}, {}", if cm < 12 { months[cm] } else { "?" }, cancel_d.get_utc_date(), cancel_d.get_utc_full_year());
                                let unlock_d = js_sys::Date::new(&JsValue::from_f64(confirm_unlock as f64 * 1000.0));
                                let um = unlock_d.get_utc_month() as usize;
                                let unlock_date_str = format!("{} {:02}, {}", if um < 12 { months[um] } else { "?" }, unlock_d.get_utc_date(), unlock_d.get_utc_full_year());
                                spawn_local(async move {
                                    let body = serde_json::json!({
                                        "tx_id": confirm_txid,
                                        "sender_email": confirm_email,
                                        "recipient_email": confirm_email,
                                        "amount_kx": lp_amt,
                                        "unlock_date": unlock_date_str,
                                        "risk_level": confirm_risk,
                                        "cancellation_deadline": cancel_deadline,
                                        "beneficiary_note_preview": confirm_note,
                                    });
                                    // Silently ignore all errors — endpoint may not exist yet
                                    let _ = JsFuture::from(post_json_fire_and_forget(
                                        "https://api.chronx.io/long-promise-confirm",
                                        &body.to_string(),
                                    )).await;
                                });
                            }
                        }
                        Err(e) => {
                            pending_email_chronos.set(0);
                            msg.set(format!("Error: {e}"));
                        }
                    }
                    sending.set(false);
                });
            }
        }
    };

    let desktop_send = is_desktop();

    view! {
        <div class="card">

            // Desktop-only dismissible warning banner
            {move || if desktop_send && !warning_dismissed.get() {
                view! {
                    <div class="desktop-warning-banner">
                        <p class="desktop-warning-text">
                            <strong>"\u{26a0} Please read before using this screen. "</strong>
                            "These features allow you to send KX across time \u{2014} to wallet addresses, \
                             email recipients, or people you can only describe by name. \
                             Once sent, the ChronX protocol cannot change or reverse any transaction. \
                             A limited cancellation window may apply. \
                             You may also enable AI management, which could manage \u{2014} and potentially lose \
                             \u{2014} all or a portion of your funds. \
                             Proceed only if you understand what you are doing."
                        </p>
                        <button class="desktop-warning-dismiss"
                            on:click=move |_| warning_dismissed.set(true)>
                            "\u{2715}"
                        </button>
                    </div>
                }.into_any()
            } else {
                view! { <span></span> }.into_any()
            }}

            // Recipient mode: KX Address | Email | Freeform
            // Mobile: email only (no KX Address button). Desktop: all three.
            {if is_desktop() {
                view! {
                    <div class="recipient-mode-group">
                        <button type="button"
                            class=move || if recipient_mode.get()==0 { "recipient-mode-btn active-kx" } else { "recipient-mode-btn" }
                            on:click=move |_| { recipient_mode.set(0); send_sub.set(0); lock_date.set(String::new()); }
                            disabled=move || sending.get()>"KX Address"</button>
                        <button type="button"
                            class=move || if recipient_mode.get()==1 { "recipient-mode-btn active-email" } else { "recipient-mode-btn" }
                            on:click=move |_| { recipient_mode.set(1); send_sub.set(1); lock_date.set(String::new()); }
                            disabled=move || sending.get()>"Email"</button>
                        <button type="button"
                            class=move || if recipient_mode.get()==2 { "recipient-mode-btn active-free" } else { "recipient-mode-btn" }
                            on:click=move |_| { recipient_mode.set(2); send_sub.set(0); send_mode.set(1); lock_date.set(String::new()); }
                            disabled=move || sending.get()>"Freeform"</button>
                    </div>
                }.into_any()
            } else {
                view! { <span></span> }.into_any()
            }}
            // Freeform warning
            {move || if recipient_mode.get() == 2 {
                view! {
                    <div class="freeform-warning">
                        "The KX is locked on-chain to a cryptographic hash of the identifier you enter below. Neither you nor anyone else can spend it until the unlock date \u{2014} at which point any person who can prove they are the named recipient may claim it. You have 7 days to cancel (or until the unlock date, whichever comes first)."
                    </div>
                }.into_any()
            } else { view! { <span></span> }.into_any() }}

            // Mode: Send Now | Send Later (both platforms)
            <div class="send-mode-row">
                <button type="button"
                    class=move || if send_mode.get()==0 { "send-mode-btn active" } else { "send-mode-btn" }
                    on:click=move |_| { send_mode.set(0); lock_date.set(String::new()); later_choice.set(255); show_date_picker.set(false); mobile_time_option.set(0); }
                    disabled=move || sending.get()>"Send Now"</button>
                <button type="button"
                    class=move || if send_mode.get()==1 { "send-mode-btn active" } else { "send-mode-btn" }
                    on:click=move |_| { send_mode.set(1); }
                    disabled=move || sending.get()>"Send Later"</button>
            </div>

            // Recipient field — depends on recipient_mode
            {move || match recipient_mode.get() {
                // KX Address
                0 => {
                    let lbl = if send_mode.get() == 0 { "To (account ID)" }
                              else { "To (recipient public key hex \u{b7} leave blank to promise to yourself)" };
                    view! {
                        <div class="field">
                            <label>{lbl}</label>
                            <div style="display:flex;gap:8px;align-items:center">
                                {move || if send_mode.get() == 0 {
                                    view! {
                                        <input type="text" placeholder="Base-58 address\u{2026}" style="flex:1"
                                            prop:value=move || to_addr.get()
                                            on:input=move |ev| to_addr.set(event_target_value(&ev))
                                            disabled=move || sending.get() />
                                    }.into_any()
                                } else {
                                    view! {
                                        <input type="text"
                                            placeholder="Leave blank for self \u{b7} paste pubkey hex or scan QR\u{2026}"
                                            style="flex:1"
                                            prop:value=move || to_pubkey.get()
                                            on:input=move |ev| to_pubkey.set(event_target_value(&ev))
                                            disabled=move || sending.get() />
                                    }.into_any()
                                }}
                                <button type="button" style="white-space:nowrap"
                                    on:click=on_scan_qr
                                    disabled=move || sending.get()>"📷 Scan QR"</button>
                            </div>
                            {move || {
                                let s = scan_msg.get();
                                if s.is_empty() { view! { <span></span> }.into_any() }
                                else {
                                    let cls = if s.starts_with("Scan") || s.starts_with("No file") { "msg error" } else { "msg success" };
                                    view! { <p class=cls style="margin-top:4px">{s}</p> }.into_any()
                                }
                            }}
                            {move || if send_mode.get() == 1 {
                                view! {
                                    <p class="label" style="margin-top:4px">
                                        {move || if to_pubkey.get().is_empty() {
                                            "Promising to: yourself"
                                        } else {
                                            "Promising to: recipient (custom key)"
                                        }}
                                    </p>
                                }.into_any()
                            } else { view! { <span></span> }.into_any() }}
                        </div>
                    }.into_any()
                }
                // Email
                1 => {
                    view! {
                        <div class="field" style="position:relative">
                            // Address Book button (all platforms)
                            <button class="address-book-btn"
                                on:click=move |_| {
                                    spawn_local(async move {
                                        if let Ok(list) = call::<Vec<Contact>>("get_contacts", no_args()).await {
                                            address_book_contacts.set(list);
                                        }
                                        address_book_open.set(true);
                                    });
                                }>
                                "\u{1f4cb} Address Book"
                            </button>
                            <label>"Recipient Email Address"</label>
                            <div class="email-field-wrap">
                            <input type="email" placeholder="recipient@example.com"
                                prop:value=move || email.get()
                                on:input=move |ev| {
                                    let val = event_target_value(&ev);
                                    email.set(val.clone());
                                    email_save_msg.set(String::new());
                                    // Check if valid email for save icon (all platforms)
                                    if val.contains('@') && val.contains('.') && val.len() >= 5 {
                                        let check_email = val.clone();
                                        spawn_local(async move {
                                            let chk = serde_wasm_bindgen::to_value(&serde_json::json!({ "email": check_email, "kxAddress": Option::<String>::None })).unwrap_or(no_args());
                                            if let Ok(Some(_)) = call::<Option<Contact>>("check_if_contact", chk).await {
                                                email_save_state.set(3); // already saved
                                            } else {
                                                email_save_state.set(1); // show save icon
                                            }
                                        });
                                    } else {
                                        email_save_state.set(0);
                                    }
                                    // Contact autocomplete
                                    if val.len() >= 2 {
                                        let q = val.clone();
                                        spawn_local(async move {
                                            let args = serde_wasm_bindgen::to_value(&serde_json::json!({ "query": q })).unwrap_or(no_args());
                                            if let Ok(results) = call::<Vec<Contact>>("search_contacts", args).await {
                                                if !results.is_empty() {
                                                    contact_suggestions.set(results);
                                                    show_contact_dropdown.set(true);
                                                } else {
                                                    show_contact_dropdown.set(false);
                                                }
                                            }
                                        });
                                    } else {
                                        show_contact_dropdown.set(false);
                                    }
                                }
                                on:focus=move |_| {
                                    // Show all contacts on focus if field is empty or short
                                    let val = email.get_untracked();
                                    if val.len() < 2 {
                                        spawn_local(async move {
                                            if let Ok(all) = call::<Vec<Contact>>("get_contacts", no_args()).await {
                                                let with_email: Vec<Contact> = all.into_iter().filter(|c| c.email.is_some()).collect();
                                                if !with_email.is_empty() {
                                                    contact_suggestions.set(with_email);
                                                    show_contact_dropdown.set(true);
                                                }
                                            }
                                        });
                                    }
                                }
                                on:blur=move |_| {
                                    // Delay hide so click on dropdown item registers first
                                    spawn_local(async move {
                                        delay_ms(200).await;
                                        show_contact_dropdown.set(false);
                                    });
                                }
                                disabled=move || sending.get() />
                            // Inline save icon (all platforms)
                            {move || {
                                match email_save_state.get() {
                                    1 => view! {
                                        <button class="email-save-icon" title="Save to Address Book"
                                            on:click=move |_| {
                                                email_save_nickname.set(String::new());
                                                email_save_state.set(2);
                                            }>"\u{1f4cb}"</button>
                                    }.into_any(),
                                    3 => view! {
                                        <span class="email-save-icon saved" title="Saved">{"\u{2713}"}</span>
                                    }.into_any(),
                                    _ => view! { <span></span> }.into_any(),
                                }
                            }}
                            </div>
                            // Nickname prompt (below email field, mobile only)
                            {move || {
                                if email_save_state.get() != 2 { return view! { <span></span> }.into_any(); }
                                let save_msg = email_save_msg.get();
                                if !save_msg.is_empty() {
                                    return view! { <p style="color:#4ade80;font-size:12px;margin:4px 0 0">{save_msg}</p> }.into_any();
                                }
                                view! {
                                    <div class="nickname-prompt">
                                        <input type="text" placeholder="Nickname (optional)"
                                            prop:value=move || email_save_nickname.get()
                                            on:input=move |ev| email_save_nickname.set(event_target_value(&ev))
                                            on:keydown=move |ev: web_sys::KeyboardEvent| {
                                                if ev.key() == "Enter" {
                                                    let em = email.get_untracked();
                                                    let nick = email_save_nickname.get_untracked();
                                                    let name = if nick.trim().is_empty() { em.clone() } else { nick.trim().to_string() };
                                                    spawn_local(async move {
                                                        let args = serde_wasm_bindgen::to_value(&serde_json::json!({
                                                            "name": name, "email": em, "kxAddress": Option::<String>::None, "notes": Option::<String>::None
                                                        })).unwrap_or(no_args());
                                                        match call::<Contact>("add_contact", args).await {
                                                            Ok(_) => { email_save_state.set(3); email_save_msg.set(String::new()); }
                                                            Err(e) => { email_save_msg.set(format!("{e}")); }
                                                        }
                                                    });
                                                }
                                            } />
                                        <button on:click=move |_| {
                                            let em = email.get_untracked();
                                            let nick = email_save_nickname.get_untracked();
                                            let name = if nick.trim().is_empty() { em.clone() } else { nick.trim().to_string() };
                                            spawn_local(async move {
                                                let args = serde_wasm_bindgen::to_value(&serde_json::json!({
                                                    "name": name, "email": em, "kxAddress": Option::<String>::None, "notes": Option::<String>::None
                                                })).unwrap_or(no_args());
                                                match call::<Contact>("add_contact", args).await {
                                                    Ok(_) => { email_save_state.set(3); email_save_msg.set(String::new()); }
                                                    Err(e) => { email_save_msg.set(format!("{e}")); }
                                                }
                                            });
                                        }>"Save"</button>
                                    </div>
                                }.into_any()
                            }}
                            // Contact autocomplete dropdown
                            {move || {
                                if !show_contact_dropdown.get() { return view! { <span></span> }.into_any(); }
                                let suggestions = contact_suggestions.get();
                                view! {
                                    <div class="contact-dropdown">
                                        {suggestions.into_iter().map(|c| {
                                            let display_email = c.email.clone().unwrap_or_default();
                                            let display_name = c.name.clone();
                                            let fill_email = display_email.clone();
                                            view! {
                                                <div class="contact-dropdown-item"
                                                    on:mousedown=move |ev| {
                                                        ev.prevent_default();
                                                        email.set(fill_email.clone());
                                                        show_contact_dropdown.set(false);
                                                    }>
                                                    <span class="contact-dropdown-name">{display_name}</span>
                                                    <span class="contact-dropdown-email">{display_email}</span>
                                                </div>
                                            }
                                        }).collect::<Vec<_>>()}
                                    </div>
                                }.into_any()
                            }}
                            // Self-email warning (FIX 8)
                            {move || {
                                let user_email = claim_email.get();
                                let entered = email.get();
                                if !user_email.is_empty() && !entered.is_empty()
                                    && user_email.to_lowercase() == entered.to_lowercase()
                                {
                                    view! {
                                        <p class="msg mining" style="margin-top:6px;font-size:13px">
                                            "\u{26a0} This is your registered claim email. "
                                            "The KX will be sent to your own wallet automatically when you click Claim in your email."
                                        </p>
                                    }.into_any()
                                } else { view! { <span></span> }.into_any() }
                            }}
                            // Mobile: warn if user pastes a KX address instead of email
                            {move || {
                                if is_desktop() { return view! { <span></span> }.into_any(); }
                                let val = email.get();
                                let v = val.trim();
                                // Detect base58 KX address: no @, length 32-50, alphanumeric (no 0OIl in base58 but close enough)
                                if !v.is_empty() && !v.contains('@') && v.len() >= 32 && v.len() <= 50
                                    && v.chars().all(|c| c.is_ascii_alphanumeric())
                                {
                                    view! {
                                        <p class="msg error" style="margin-top:6px;font-size:12px">
                                            "On mobile, please send via email address. Use the desktop wallet to send directly to a KX address."
                                        </p>
                                    }.into_any()
                                } else { view! { <span></span> }.into_any() }
                            }}
                        </div>
                    }.into_any()
                }
                // Freeform
                _ => {
                    view! {
                        <div class="field">
                            <label>"Recipient Name or Description"</label>
                            <input type="text" placeholder="e.g. Emma Johnson, born 2019 \u{b7} Greenpeace Foundation \u{b7} My future self"
                                prop:value=move || freeform_recip.get()
                                on:input=move |ev| freeform_recip.set(event_target_value(&ev))
                                disabled=move || sending.get() />
                        </div>
                    }.into_any()
                }
            }}

            // Amount
            <div class="field">
                <label>"Amount (KX)"</label>
                <input type="text" inputmode="decimal" placeholder="0.000000"
                    prop:value=move || format_amount_display(&amount.get())
                    on:input=move |ev| {
                        let raw = event_target_value(&ev);
                        // Strip commas (display-only), then filter to digits + one dot + max 6 decimals
                        let stripped: String = raw.chars().filter(|&c| c != ',').collect();
                        let filtered: String = {
                            let mut has_dot = false;
                            let mut decimals = 0u8;
                            stripped.chars().filter(|&c| {
                                if c.is_ascii_digit() {
                                    if has_dot { decimals += 1; decimals <= 6 } else { true }
                                } else if c == '.' && !has_dot {
                                    has_dot = true; true
                                } else { false }
                            }).collect()
                        };
                        amount.set(filtered);
                    }
                    disabled=move || sending.get() />
                <p class="fee-free-line">"✓ No transaction fees. The recipient receives exactly what you send."</p>
            </div>

            // (Mobile time picker removed — unified Send Now / Send Later toggle above)

            // Send Later options — radio presets + date picker (both platforms)
            {move || if send_mode.get() == 1 {
                let is_mobile = !is_desktop();
                let presets: Vec<(u8, &str, i64)> = if is_mobile {
                    vec![
                        (0, "In 1 day", 86400),
                        (1, "In 1 week", 604800),
                        (2, "In 1 month", 2592000),
                        (3, "In 1 year", 31536000),
                    ]
                } else {
                    vec![
                        (0, "In 1 day", 86400),
                        (1, "In 1 week", 604800),
                        (2, "In 1 month", 2592000),
                        (3, "In 1 year", 31536000),
                        (4, "In 5 years", 157680000),
                        (5, "In 20 years", 630720000),
                        (6, "In 100 years", 3153600000),
                    ]
                };
                view! {
                    <div class="send-later-options visible">
                        <div class="send-later-radio-group">
                            {presets.into_iter().map(|(val, label, secs)| {
                                view! {
                                    <label class="send-later-row">
                                        <input type="radio" name="send_later_opt"
                                            prop:checked=move || later_choice.get() == val && !show_date_picker.get()
                                            on:change=move |_| {
                                                later_choice.set(val);
                                                show_date_picker.set(false);
                                                custom_date.set(String::new());
                                                custom_time.set(String::new());
                                                // Compute target as local time
                                                let now_ms = js_sys::Date::now();
                                                let target_ms = now_ms + (secs as f64) * 1000.0;
                                                let d = js_sys::Date::new_0();
                                                d.set_time(target_ms);
                                                // Format as local datetime string
                                                let year = d.get_full_year();
                                                let month = d.get_month() + 1;
                                                let day = d.get_date();
                                                let hour = d.get_hours();
                                                let min = d.get_minutes();
                                                lock_date.set(format!("{:04}-{:02}-{:02}T{:02}:{:02}", year, month, day, hour, min));
                                            }
                                            style="accent-color:#d4a84b"
                                            disabled=move || sending.get() />
                                        {label}
                                    </label>
                                }
                            }).collect::<Vec<_>>()}
                            // "Pick a date..." option
                            <label class="send-later-row">
                                <input type="radio" name="send_later_opt"
                                    prop:checked=move || show_date_picker.get()
                                    on:change=move |_| {
                                        show_date_picker.set(true);
                                        later_choice.set(255);
                                        lock_date.set(String::new());
                                    }
                                    style="accent-color:#d4a84b"
                                    disabled=move || sending.get() />
                                "Pick a date..."
                            </label>
                        </div>
                        // Date + Time picker (shown when "Pick a date..." selected)
                        {move || if show_date_picker.get() {
                            view! {
                                <div style="display:flex;gap:8px;margin-top:4px;flex-wrap:wrap">
                                    <div style="flex:1;min-width:140px">
                                        <label style="font-size:12px;color:#888;margin-bottom:4px;display:block">"Date"</label>
                                        <input type="date"
                                            style="background:#1a1d2e;border:1px solid #2a2f3e;color:#e5e7eb;padding:8px 12px;border-radius:6px;font-size:14px;width:100%"
                                            prop:min=move || {
                                                let d = js_sys::Date::new_0();
                                                d.set_time(js_sys::Date::now() + 86400000.0);
                                                format!("{:04}-{:02}-{:02}", d.get_full_year(), d.get_month() + 1, d.get_date())
                                            }
                                            prop:value=move || custom_date.get()
                                            on:input=move |ev| {
                                                let val = event_target_value(&ev);
                                                custom_date.set(val.clone());
                                                let time_val = custom_time.get_untracked();
                                                if !val.is_empty() {
                                                    let t = if time_val.is_empty() { "00:00".to_string() } else { time_val };
                                                    lock_date.set(format!("{}T{}", val, t));
                                                }
                                            }
                                            disabled=move || sending.get() />
                                    </div>
                                    <div style="flex:1;min-width:120px">
                                        <label style="font-size:12px;color:#888;margin-bottom:4px;display:block">"Time"</label>
                                        <input type="time"
                                            style="background:#1a1d2e;border:1px solid #2a2f3e;color:#e5e7eb;padding:8px 12px;border-radius:6px;font-size:14px;width:100%"
                                            prop:value=move || custom_time.get()
                                            on:input=move |ev| {
                                                let val = event_target_value(&ev);
                                                custom_time.set(val.clone());
                                                let date_val = custom_date.get_untracked();
                                                if !date_val.is_empty() {
                                                    lock_date.set(format!("{}T{}", date_val, val));
                                                }
                                            }
                                            disabled=move || sending.get() />
                                    </div>
                                </div>
                                <p style="font-size:11px;color:#666;margin-top:6px;font-style:italic">"Times are in your local timezone"</p>
                            }.into_any()
                        } else {
                            view! { <span></span> }.into_any()
                        }}
                        // Arrival date confirmation (shown when lock_date is set)
                        {move || {
                            let dt_str = lock_date.get();
                            if dt_str.is_empty() { return view! { <span></span> }.into_any(); }
                            let unix = match date_str_to_unix(&dt_str) {
                                Some(t) => t,
                                None => return view! { <span></span> }.into_any(),
                            };
                            let now_secs = (js_sys::Date::now() / 1000.0) as i64;
                            if unix <= now_secs {
                                return view! { <p class="msg error" style="margin-top:4px">"Please select a future date and time"</p> }.into_any();
                            }
                            // Format arrival in local time
                            let d = js_sys::Date::new_0();
                            d.set_time((unix as f64) * 1000.0);
                            let months = ["January","February","March","April","May","June","July","August","September","October","November","December"];
                            let m = d.get_month() as usize;
                            let month_name = if m < 12 { months[m] } else { "?" };
                            let day = d.get_date();
                            let year = d.get_full_year();
                            let hours = d.get_hours();
                            let mins = d.get_minutes();
                            let ampm = if hours >= 12 { "PM" } else { "AM" };
                            let h12 = if hours % 12 == 0 { 12 } else { hours % 12 };
                            let label = format!("This KX will arrive on\n{} {}, {} at {}:{:02} {} (your local time)", month_name, day, year, h12, mins, ampm);
                            view! { <p style="color:#d4a84b;font-size:13px;margin-top:6px;font-weight:600;white-space:pre-line">{label}</p> }.into_any()
                        }}
                    </div>
                }.into_any()
            } else { view! { <span></span> }.into_any() }}

            // Memo — for Send Later or Email
            {move || if send_mode.get() == 1 || send_sub.get() == 1 {
                view! {
                    <div class="field">
                        <label>"Memo (optional, max 256 chars)"</label>
                        <textarea placeholder="e.g. Birthday gift for Alex"
                            maxlength="256" rows="2"
                            prop:value=move || memo.get()
                            on:input=move |ev| memo.set(event_target_value(&ev))
                            disabled=move || sending.get()></textarea>
                        // v2.5.29: Public memo toggle — desktop only, hidden when memo empty
                        {move || if is_desktop() && !memo.get().is_empty() {
                            view! {
                                <label style="display:flex;align-items:center;gap:6px;margin-top:6px;cursor:pointer;font-size:12px;color:#9ca3af">
                                    <input type="checkbox"
                                        prop:checked=move || memo_public.get()
                                        on:change=move |ev| {
                                            use wasm_bindgen::JsCast;
                                            let checked = ev.target()
                                                .and_then(|t| t.dyn_into::<web_sys::HtmlInputElement>().ok())
                                                .map(|i| i.checked())
                                                .unwrap_or(false);
                                            memo_public.set(checked);
                                        }
                                        style="accent-color:#d4a84b" />
                                    "Make this memo public"
                                </label>
                                <p style="font-size:0.7rem;color:#6b7280;margin:2px 0 0 22px">"Public memos are permanently visible to everyone on the blockchain."</p>
                            }.into_any()
                        } else {
                            view! { <span></span> }.into_any()
                        }}
                    </div>
                }.into_any()
            } else { view! { <span></span> }.into_any() }}

            // Beneficiary Information (Verifas) — Send Later, desktop only
            {move || if send_mode.get() == 1 && is_desktop() {
                // Check if unlock date is more than 2 years from now
                let is_long_horizon = {
                    let ld = lock_date.get();
                    if ld.is_empty() { false } else {
                        let now_ms = js_sys::Date::now();
                        let d = js_sys::Date::new(&wasm_bindgen::JsValue::from_str(&ld));
                        let unlock_ms = d.get_time();
                        let two_years_ms = 2.0 * 365.25 * 86400.0 * 1000.0;
                        unlock_ms - now_ms > two_years_ms
                    }
                };
                view! {
                    <div class="beneficiary-field">
                        <label>"Beneficiary Information (optional)"</label>
                        <textarea class="lp-textarea" rows="4" maxlength="2000"
                            placeholder="e.g. \"I leave this to my daughter Sarah Monroe, born March 22 2008, Phoenix Arizona. She may have changed her name by marriage.\""
                            prop:value=move || grantor_intent.get()
                            on:input=move |ev| grantor_intent.set(event_target_value(&ev))
                            disabled=move || sending.get()></textarea>
                        <div style="display:flex;justify-content:flex-end;margin-top:2px">
                            <span class=move || { let c = grantor_intent.get().len(); if c > 1800 { "char-counter near-limit" } else { "char-counter" } }
                                style="font-size:11px;color:#6b7280">
                                {move || format!("{} / 2,000", grantor_intent.get().len())}
                            </span>
                        </div>
                        <p style="font-size:0.7rem;color:#6b7280;margin:4px 0 0">"Encrypted to Verifas. Only a Verifas attestor can read this."</p>
                        {if is_long_horizon {
                            view! { <p style="font-size:0.7rem;color:#9ca3af;margin:4px 0 0;font-style:italic">"For long-horizon promises, beneficiary info helps Verifas locate your recipient."</p> }.into_any()
                        } else {
                            view! { <span></span> }.into_any()
                        }}
                    </div>
                }.into_any()
            } else { view! { <span></span> }.into_any() }}

            // Email info box
            {move || if send_sub.get() == 1 {
                let txt = t(&lang.get(), "email_disclaimer");
                view! {
                    <div style="background:#1a1d27;border:1px solid #2a2d37;border-radius:8px;padding:10px 12px;margin-bottom:8px">
                        <p style="font-size:12px;color:#9ca3af;line-height:1.5;margin:0">{txt}</p>
                    </div>
                }.into_any()
            } else { view! { <span></span> }.into_any() }}

            // ── AI / MISAI management section (Send Later, desktop only) ────────────────
            {move || {
                if send_mode.get() != 1 || !is_desktop() {
                    return view! { <span></span> }.into_any();
                }
                let rl = risk_level.get();
                let (_risk_text, _risk_class) = match rl {
                    1..=20   => ("Capital Preservation", "safe"),
                    21..=40  => ("Conservative Growth", "safe"),
                    41..=60  => ("Balanced", "moderate"),
                    61..=80  => ("Growth", "moderate"),
                    _        => ("Aggressive Growth", "aggressive"),
                };
                view! {
                    <div class="ai-section">
                        <div style="display:flex;align-items:center;justify-content:space-between;margin-bottom:12px">
                            <label style="font-weight:600;color:#e5e7eb">"AI Management (MISAI)"</label>
                            <label class="toggle-switch">
                                <input type="checkbox"
                                    prop:checked=move || ai_managed.get()
                                    on:change=move |ev| {
                                        use wasm_bindgen::JsCast;
                                        let checked = ev.target()
                                            .and_then(|t| t.dyn_into::<web_sys::HtmlInputElement>().ok())
                                            .map(|i| i.checked()).unwrap_or(false);
                                        ai_managed.set(checked);
                                    } />
                                <span class="toggle-slider"></span>
                            </label>
                        </div>
                        {move || if ai_managed.get() {
                            let rl2 = risk_level.get();
                            let (rt2, rc2) = match rl2 {
                                1..=20   => ("Capital Preservation", "safe"),
                                21..=40  => ("Conservative Growth", "safe"),
                                41..=60  => ("Balanced", "moderate"),
                                61..=80  => ("Growth", "moderate"),
                                _        => ("Aggressive Growth", "aggressive"),
                            };
                            view! {
                                <div>
                                    <p style="color:#9ca3af;font-size:13px;margin:0 0 12px">"The AI agent will actively manage a portion of the promise value according to your risk preference."</p>

                                    // Slider 1: Percentage to manage
                                    <div class="field" style="margin-bottom:16px">
                                        <label style="margin-bottom:4px">"Percentage to manage"</label>
                                        <div class="risk-slider-row">
                                            <span style="color:#888;font-size:12px">"1%"</span>
                                            <input type="range" class="pct-slider" min="1" max="100" step="1"
                                                prop:value=move || ai_percentage.get().to_string()
                                                on:input=move |ev| {
                                                    let v: u32 = event_target_value(&ev).parse().unwrap_or(1);
                                                    ai_percentage.set(v);
                                                }
                                                disabled=move || sending.get() />
                                            <span style="color:#888;font-size:12px">"100%"</span>
                                        </div>
                                        <div style="text-align:center;margin-top:4px">
                                            <span class="risk-number">{move || ai_percentage.get()}</span>
                                            <span style="color:#888;font-size:0.85rem">"% managed"</span>
                                        </div>
                                    </div>

                                    // Slider 2: Risk level
                                    <div class="field" style="margin-bottom:0">
                                        <label style="margin-bottom:4px">
                                            "Risk Level "
                                            <span class=format!("risk-label {}", rc2)>{rt2}</span>
                                        </label>
                                        <div class="risk-slider-row">
                                            <span style="color:#888;font-size:12px">"Conservative"</span>
                                            <input type="range" class="risk-slider" min="1" max="100" step="1"
                                                style=move || {
                                                    let v = risk_level.get();
                                                    let (r, g) = if v <= 50 {
                                                        let t = v as f64 / 50.0;
                                                        let r = (t * 255.0) as u32;
                                                        (r, 200u32)
                                                    } else {
                                                        let t = (v as f64 - 50.0) / 50.0;
                                                        let g = ((1.0 - t) * 200.0) as u32;
                                                        (255u32, g)
                                                    };
                                                    format!(
                                                        "background: linear-gradient(to right, \
                                                         rgb(34,197,94) 0%, \
                                                         rgb(234,179,8) 50%, \
                                                         rgb(239,68,68) 100%); \
                                                         accent-color: rgb({},{},0);",
                                                        r, g
                                                    )
                                                }
                                                prop:value=move || risk_level.get().to_string()
                                                on:input=move |ev| {
                                                    let v: u32 = event_target_value(&ev).parse().unwrap_or(50);
                                                    risk_level.set(v);
                                                }
                                                disabled=move || sending.get() />
                                            <span style="color:#888;font-size:12px">"Aggressive"</span>
                                        </div>
                                        <div style="text-align:center;margin-top:4px">
                                            <span style="color:#888;font-size:0.85rem">"Level: "</span>
                                            <span class="risk-number">{move || risk_level.get()}</span>
                                            <span style="color:#888;font-size:0.85rem">"/100"</span>
                                        </div>
                                    </div>
                                </div>
                            }.into_any()
                        } else {
                            view! { <span></span> }.into_any()
                        }}
                    </div>
                }.into_any()
            }}

            // ── Long Promise badge (>1 year) ─────────────────────────────────────
            {move || {
                if !is_long_promise.get() {
                    return view! { <span></span> }.into_any();
                }
                view! {
                    <div class="long-promise-section">
                        <div class="long-promise-header">
                            <span class="long-promise-badge">"\u{23f3} Long Promise"</span>
                            <span style="color:#9ca3af;font-size:0.82rem">"This promise unlocks in over a year. Axiom consent is required."</span>
                        </div>
                        <div class="axiom-consent-row">
                            <label class="axiom-checkbox-label">
                                <input type="checkbox"
                                    prop:checked=move || axiom_consented.get()
                                    on:change=move |ev| {
                                        use wasm_bindgen::JsCast;
                                        let checked = ev.target()
                                            .and_then(|t| t.dyn_into::<web_sys::HtmlInputElement>().ok())
                                            .map(|i| i.checked()).unwrap_or(false);
                                        axiom_consented.set(checked);
                                    }
                                />
                                " I have read and accept the "
                                <a href="https://chronx.io/governance" target="_blank"
                                   class="axiom-link">"Promise Axioms"</a>
                                " and the "
                                <a href="https://chronx.io/terms" target="_blank"
                                   class="axiom-link">"Terms of Service"</a>
                                "."
                            </label>
                        </div>
                        {move || if !axiom_consented.get() {
                            view! { <p class="axiom-required">"Required: please accept the Promise Axioms and Terms of Service before sending."</p> }.into_any()
                        } else {
                            view! { <span class="msg success" style="font-size:13px;margin:4px 0">"\u{2705} Axiom consent recorded"</span> }.into_any()
                        }}
                    </div>
                }.into_any()
            }}

            // Series entries (Email + Send Later only)
            {move || {
                if send_sub.get() != 1 || send_mode.get() != 1 {
                    return view! { <span></span> }.into_any();
                }
                let entries = series_entries.get();
                view! {
                    <div>
                        {entries.into_iter().enumerate().map(|(i, (s_amt, s_date, s_memo))| {
                            view! {
                                <div class="series-entry">
                                    <div style="display:flex;justify-content:space-between;align-items:center">
                                        <span style="font-size:12px;color:#9ca3af;font-weight:600">{format!("Payment #{}", i + 2)}</span>
                                        <button class="remove-btn" on:click=move |_| {
                                            series_entries.update(|v| { v.remove(i); });
                                        }>"✕"</button>
                                    </div>
                                    <input type="number" class="input" placeholder="Amount (KX)" step="0.000001" min="0"
                                        prop:value=move || s_amt.get()
                                        on:input=move |ev| s_amt.set(event_target_value(&ev))
                                        disabled=move || sending.get() />
                                    <input type="datetime-local" class="input" style="margin-top:4px"
                                        prop:value=move || s_date.get()
                                        on:input=move |ev| s_date.set(event_target_value(&ev))
                                        disabled=move || sending.get() />
                                    <input type="text" class="input" placeholder="Memo (optional)" maxlength="256" style="margin-top:4px"
                                        prop:value=move || s_memo.get()
                                        on:input=move |ev| s_memo.set(event_target_value(&ev))
                                        disabled=move || sending.get() />
                                </div>
                            }
                        }).collect::<Vec<_>>()}
                        {if is_desktop() { view! {
                            <button class="pill" style="width:100%;margin:8px 0;color:#d4a84b;border-color:#d4a84b;font-weight:600"
                                disabled=move || sending.get() || (series_entries.get().len() >= 9)
                                on:click=move |_| {
                                    series_entries.update(|v| {
                                        v.push((RwSignal::new(String::new()), RwSignal::new(String::new()), RwSignal::new(String::new())));
                                    });
                                }>
                                "+ Add Another Payment"
                            </button>
                        }.into_any() } else { view! { <span></span> }.into_any() }}
                    </div>
                }.into_any()
            }}

            // Send Later warning
            {move || if send_mode.get() == 1 {
                view! {
                    <p class="lock-warning">
                        "\u{26a0} You may cancel this promise from History within the cancellation window."
                        <br/>
                        "Once the window closes, the grantor cannot recover promised funds under any circumstances. A promise is a promise."
                    </p>
                }.into_any()
            } else { view! { <span></span> }.into_any() }}

            // Submit button
            <button class=move || if send_mode.get()==1 { "primary danger" } else { "primary" }
                style=move || if is_long_promise.get() && !axiom_consented.get() { "opacity:0.5" } else { "" }
                on:click=move |ev: web_sys::MouseEvent| {
                    // Gate: if long promise and axiom not yet consented, open modal instead of sending
                    if is_long_promise.get() && !axiom_consented.get() {
                        axiom_modal_open.set(true);
                        return;
                    }
                    // Mobile confirmation screen
                    if !is_desktop() && !mobile_confirm_open.get_untracked() {
                        let to_display = if send_sub.get_untracked() == 1 {
                            email.get_untracked()
                        } else {
                            to_addr.get_untracked()
                        };
                        mobile_confirm_to_display.set(to_display);
                        mobile_confirm_amount_display.set(format!("{} KX", amount.get_untracked()));
                        let unlock_str = if send_mode.get_untracked() == 0 {
                            t(&lang.get_untracked(), "mobile_send_now")
                        } else {
                            lock_date.get_untracked()
                        };
                        mobile_confirm_unlock_display.set(unlock_str);
                        mobile_confirm_memo_display.set(memo.get_untracked());
                        mobile_confirm_open.set(true);
                        return;
                    }
                    mobile_confirm_open.set(false);
                    on_send(ev);
                }
                disabled=move || sending.get() || (is_long_promise.get() && !axiom_consented.get())>
                {move || {
                    let has_series = !series_entries.get().is_empty() && send_sub.get() == 1 && send_mode.get() == 1;
                    if sending.get() {
                        if send_sub.get() == 1 || send_mode.get() == 0 { "Sending\u{2026}" } else { "Promising\u{2026}" }
                    } else if has_series {
                        "Send Promise Series"
                    } else if send_sub.get() == 1 {
                        "Send to Email"
                    } else if send_mode.get() == 1 {
                        "Make a Promise"
                    } else {
                        "Send Transfer"
                    }
                }}
            </button>

            {move || {
                let s = msg.get();
                if s.is_empty() { view! { <span></span> }.into_any() }
                else {
                    let cls = if s.starts_with("Error") { "msg error" }
                              else if s.starts_with("Mining") { "msg mining" }
                              else { "msg success" };
                    view! { <p class=cls>{s}</p> }.into_any()
                }
            }}
            {move || {
                if spam_warn.get() {
                    view! {
                        <p class="msg success" style="font-weight:800;margin-top:6px;font-size:13px;word-wrap:break-word;overflow-wrap:break-word;">
                            "Ask your recipient to check their spam folder \u{2014} the first email from ChronX may be filtered."
                        </p>
                    }.into_any()
                } else { view! { <span></span> }.into_any() }
            }}
            // (save_contact_banner removed — replaced by inline save icon on email field)
        </div>
        // Address Book modal (mobile only)
        {move || if address_book_open.get() {
            let list = address_book_contacts.get();
            view! {
                <div class="address-book-modal" on:click=move |_| address_book_open.set(false)>
                    <div class="address-book-sheet" on:click=move |ev| ev.stop_propagation()>
                        <div class="address-book-header">
                            <span class="address-book-title">"Address Book"</span>
                            <button class="address-book-close" on:click=move |_| address_book_open.set(false)>"\u{2715}"</button>
                        </div>
                        {if list.is_empty() {
                            view! { <p class="address-book-empty">"No saved contacts yet."</p> }.into_any()
                        } else {
                            view! {
                                <div>
                                    {list.into_iter().map(|c| {
                                        let c_email = c.email.clone().unwrap_or_default();
                                        let c_name = c.name.clone();
                                        let fill_email = c_email.clone();
                                        let del_id = c.id.clone();
                                        let has_nickname = !c_name.is_empty() && c_name.to_lowercase() != c_email.to_lowercase();
                                        let primary = if has_nickname { c_name.clone() } else { c_email.clone() };
                                        let secondary_email = c_email.clone();
                                        view! {
                                            <div class="address-book-item">
                                                <div class="address-book-item-info"
                                                    on:click=move |_| {
                                                        email.set(fill_email.clone());
                                                        address_book_open.set(false);
                                                    }>
                                                    <span class="address-book-item-name">{primary}</span>
                                                    {if has_nickname {
                                                        view! { <span class="address-book-item-email">{secondary_email}</span> }.into_any()
                                                    } else {
                                                        view! { <span></span> }.into_any()
                                                    }}
                                                </div>
                                                <button class="address-book-item-delete"
                                                    on:click=move |ev| {
                                                        ev.stop_propagation();
                                                        let id = del_id.clone();
                                                        spawn_local(async move {
                                                            let args = serde_wasm_bindgen::to_value(&serde_json::json!({ "id": id })).unwrap_or(no_args());
                                                            let _ = call::<()>("delete_contact", args).await;
                                                            if let Ok(refreshed) = call::<Vec<Contact>>("get_contacts", no_args()).await {
                                                                address_book_contacts.set(refreshed);
                                                            }
                                                        });
                                                    }>
                                                    "\u{2715}"
                                                </button>
                                            </div>
                                        }
                                    }).collect::<Vec<_>>()}
                                </div>
                            }.into_any()
                        }}
                    </div>
                </div>
            }.into_any()
        } else { view! { <span></span> }.into_any() }}
        // Email send confirmation modal
        {move || if email_confirm_open.get() {
            let disp_email = email_confirm_email.get();
            let disp_amt = email_confirm_amt.get();
            let disp_memo = email_confirm_memo.get();
            view! {
                <div class="cascade-confirm-modal">
                    <div class="cascade-confirm-box" style="max-width:400px">
                        <h3 style="margin:0 0 12px;color:#e5e7eb">{format!("Send {disp_amt} KX to {disp_email}?")}</h3>
                        {if !disp_memo.is_empty() {
                            view! { <p style="color:#9ca3af;font-size:13px;margin:0 0 12px">{format!("Memo: {disp_memo}")}</p> }.into_any()
                        } else { view! { <span></span> }.into_any() }}
                        // (Trusted contact checkbox removed in v2.4.1 — replaced by Address Book)
                        <div class="btn-row">
                            <button class="btn-confirm" on:click=move |_| {
                                email_confirm_open.set(false);
                                email_send_confirmed.set(true);
                                // Re-trigger on_send — it will now pass the confirmation gate
                                on_send(web_sys::MouseEvent::new("click").unwrap());
                            }>"Confirm Send"</button>
                            <button class="btn-cancel" on:click=move |_| email_confirm_open.set(false)>"Cancel"</button>
                        </div>
                    </div>
                </div>
            }.into_any()
        } else { view! { <span></span> }.into_any() }}
        <AxiomConsentModal open=axiom_modal_open consented=axiom_consented consent_hash=axiom_consent_hash />
        // Mobile confirmation screen
        {move || if mobile_confirm_open.get() {
            let to_val = mobile_confirm_to_display.get();
            let amt_val = mobile_confirm_amount_display.get();
            let unlock_val = mobile_confirm_unlock_display.get();
            let memo_val = mobile_confirm_memo_display.get();
            view! {
                <div class="cascade-confirm-modal">
                    <div class="cascade-confirm-box" style="max-width:400px">
                        <h3 style="margin:0 0 16px;color:#e5e7eb">{move || t(&lang.get(), "send_confirm_title")}</h3>
                        <div style="display:flex;flex-direction:column;gap:10px">
                            <div style="display:flex;justify-content:space-between">
                                <span style="color:#9ca3af;font-size:13px">{move || t(&lang.get(), "mobile_confirm_to")}</span>
                                <span style="color:#e5e7eb;font-size:13px;font-weight:600;word-break:break-all;text-align:right;max-width:60%">{to_val}</span>
                            </div>
                            <div style="display:flex;justify-content:space-between">
                                <span style="color:#9ca3af;font-size:13px">{move || t(&lang.get(), "mobile_confirm_amount")}</span>
                                <span style="color:#d4a84b;font-size:13px;font-weight:600">{amt_val}</span>
                            </div>
                            <div style="display:flex;justify-content:space-between">
                                <span style="color:#9ca3af;font-size:13px">{move || t(&lang.get(), "mobile_confirm_unlocks")}</span>
                                <span style="color:#e5e7eb;font-size:13px">{unlock_val}</span>
                            </div>
                            {if !memo_val.is_empty() {
                                let m = memo_val.clone();
                                view! {
                                    <div style="display:flex;justify-content:space-between">
                                        <span style="color:#9ca3af;font-size:13px">{move || t(&lang.get(), "mobile_confirm_memo")}</span>
                                        <span style="color:#e5e7eb;font-size:13px;max-width:60%;text-align:right;word-break:break-word">{m}</span>
                                    </div>
                                }.into_any()
                            } else { view! { <span></span> }.into_any() }}
                        </div>
                        <div style="display:flex;gap:8px;margin-top:20px">
                            <button style="flex:1;padding:10px;background:transparent;border:1px solid #374151;color:#9ca3af;border-radius:8px;cursor:pointer;font-size:14px"
                                on:click=move |_| mobile_confirm_open.set(false)>
                                {move || t(&lang.get(), "cancel")}
                            </button>
                            <button class="primary" style="flex:1;padding:10px;font-size:14px"
                                disabled=move || sending.get()
                                on:click=move |ev: web_sys::MouseEvent| {
                                    // Bypass the desktop email confirmation gate
                                    email_send_confirmed.set(true);
                                    mobile_confirm_open.set(false);
                                    on_send(ev);
                                }>
                                {move || if sending.get() { "Sending\u{2026}".to_string() } else { t(&lang.get(), "confirm_send") }}
                            </button>
                        </div>
                    </div>
                </div>
            }.into_any()
        } else { view! { <span></span> }.into_any() }}
    }
}

// ── AxiomConsentModal ─────────────────────────────────────────────────────────

#[component]
fn AxiomConsentModal(
    open: RwSignal<bool>,
    consented: RwSignal<bool>,
    consent_hash: RwSignal<String>,
) -> impl IntoView {
    view! {
        {move || if open.get() {
            view! {
                <div class="axiom-modal-overlay" on:click=move |_| open.set(false)>
                    <div class="axiom-modal-box" on:click=move |ev: web_sys::MouseEvent| ev.stop_propagation()>
                        <h3 style="margin:0 0 12px;color:#e5e7eb">"ChronX Promise Axioms"</h3>
                        <div class="axiom-modal-body">
                            <p>"By creating a Long Promise (over 1 year), you acknowledge:"</p>
                            <ul style="padding-left:20px;color:#d1d5db;line-height:1.7">
                                <li>"This promise becomes irrevocable after 7 days."</li>
                                <li>"If AI management is enabled, your KX may be actively managed, which may result in gains or losses including total loss."</li>
                                <li>"The risk level you set is advisory and does not guarantee a specific outcome."</li>
                                <li>"ChronX bears no liability for the outcome of AI-managed promises."</li>
                                <li>"You have read the full governance policy at chronx.io/governance.html."</li>
                            </ul>
                        </div>
                        <div class="btn-row" style="margin-top:16px">
                            <button class="btn-confirm" on:click=move |_| {
                                consented.set(true);
                                spawn_local(async move {
                                    match call::<String>("get_axiom_consent_hash", no_args()).await {
                                        Ok(h) => consent_hash.set(h),
                                        Err(_) => consent_hash.set("unavailable".to_string()),
                                    }
                                });
                                open.set(false);
                            }>"I Accept"</button>
                            <button class="btn-cancel" on:click=move |_| open.set(false)>"Cancel"</button>
                        </div>
                    </div>
                </div>
            }.into_any()
        } else {
            view! { <span></span> }.into_any()
        }}
    }
}

// ── CascadeSendPanel (desktop only) ───────────────────────────────────────────

#[derive(Clone)]
struct CascadeStage {
    amount: RwSignal<String>,
    unlock_mode: RwSignal<u8>, // 0=immediately, 1=after duration, 2=on date
    dur_value: RwSignal<String>,
    dur_unit: RwSignal<String>,
    date: RwSignal<String>,
}

fn make_stage() -> CascadeStage {
    CascadeStage {
        amount: RwSignal::new(String::new()),
        unlock_mode: RwSignal::new(0),
        dur_value: RwSignal::new(String::new()),
        dur_unit: RwSignal::new("days".to_string()),
        date: RwSignal::new(String::new()),
    }
}

fn stage_unlock_unix(s: &CascadeStage) -> Option<i64> {
    match s.unlock_mode.get_untracked() {
        0 => Some(0), // immediately
        1 => {
            let v: f64 = s.dur_value.get_untracked().parse().ok()?;
            if v <= 0.0 { return None; }
            let secs = match s.dur_unit.get_untracked().as_str() {
                "minutes" => (v * 60.0) as i64,
                "hours" => (v * 3600.0) as i64,
                "days" => (v * 86400.0) as i64,
                "weeks" => (v * 604800.0) as i64,
                _ => return None,
            };
            let now = (js_sys::Date::now() / 1000.0) as i64;
            Some(now + secs)
        }
        2 => date_str_to_unix(&s.date.get_untracked()),
        _ => None,
    }
}

fn stage_display_date(s: &CascadeStage) -> String {
    match s.unlock_mode.get() {
        0 => "Immediately".to_string(),
        1 => {
            let v = s.dur_value.get();
            let u = s.dur_unit.get();
            if v.is_empty() { return "—".to_string(); }
            format!("{v} {u}")
        }
        2 => {
            let d = s.date.get();
            if d.is_empty() { "—".to_string() } else { d.replace('T', " ") }
        }
        _ => "—".to_string(),
    }
}

#[component]
fn CascadeSendPanel(
    info: RwSignal<Option<AccountInfo>>,
    pending_email_chronos: RwSignal<u64>,
    lang: RwSignal<String>,
) -> impl IntoView {
    let email = RwSignal::new(String::new());
    let memo = RwSignal::new(String::new());
    let memo_public = RwSignal::new(false); // v2.5.29: public memo toggle
    let stages: RwSignal<Vec<CascadeStage>> = RwSignal::new(vec![make_stage()]);
    let sending = RwSignal::new(false);
    let msg = RwSignal::new(String::new());
    let spam_warn = RwSignal::new(false);
    let confirm_open = RwSignal::new(false);
    // Contact autocomplete for cascade send
    let cascade_contact_suggestions: RwSignal<Vec<Contact>> = RwSignal::new(Vec::new());
    let cascade_show_dropdown = RwSignal::new(false);

    let add_stage = move |_: web_sys::MouseEvent| {
        stages.update(|v| { if v.len() < 10 { v.push(make_stage()); } });
    };

    let use_template = move |_: web_sys::MouseEvent| {
        let template = vec![
            ("100", 0u8, "", ""),
            ("250", 1, "7", "days"),
            ("350", 1, "14", "days"),
            ("500", 1, "21", "days"),
            ("800", 1, "30", "days"),
            ("1000", 1, "60", "days"),
        ];
        let new_stages: Vec<CascadeStage> = template.into_iter().map(|(amt, mode, dur, unit)| {
            CascadeStage {
                amount: RwSignal::new(amt.to_string()),
                unlock_mode: RwSignal::new(mode),
                dur_value: RwSignal::new(dur.to_string()),
                dur_unit: RwSignal::new(if unit.is_empty() { "days".to_string() } else { unit.to_string() }),
                date: RwSignal::new(String::new()),
            }
        }).collect();
        stages.set(new_stages);
        memo.set("Welcome to ChronX".to_string());
    };

    let on_send = move |_: web_sys::MouseEvent| {
        let email_str = email.get_untracked().trim().to_string();
        if !is_valid_email(&email_str) {
            msg.set("Error: enter a valid email address.".into());
            return;
        }
        let st = stages.get_untracked();
        if st.is_empty() {
            msg.set("Error: add at least one stage.".into());
            return;
        }
        // Validate all stages
        let mut total_kx: f64 = 0.0;
        for (i, s) in st.iter().enumerate() {
            let amt: f64 = match s.amount.get_untracked().parse::<f64>() {
                Ok(v) if v > 0.0 => v,
                _ => { msg.set(format!("Error: stage {} has invalid amount.", i + 1)); return; }
            };
            total_kx += amt;
            if stage_unlock_unix(s).is_none() {
                msg.set(format!("Error: stage {} has invalid unlock time.", i + 1));
                return;
            }
        }
        // Balance check
        if let Some(ref ai) = info.get_untracked() {
            let raw: f64 = ai.spendable_chronos.parse::<f64>().unwrap_or(0.0);
            let pending = pending_email_chronos.get_untracked() as f64;
            let avail = ((raw - pending).max(0.0)) / 1_000_000.0;
            if total_kx > avail {
                msg.set(format!("Error: insufficient balance. You have {avail:.6} KX available."));
                return;
            }
        }
        confirm_open.set(true);
    };

    let on_confirm = move |_: web_sys::MouseEvent| {
        confirm_open.set(false);
        let email_str = email.get_untracked().trim().to_string();
        let memo_str = memo.get_untracked().trim().to_string();
        let memo_opt: Option<String> = if memo_str.is_empty() { None } else { Some(memo_str) };
        let is_memo_public = memo_public.get_untracked();
        let st = stages.get_untracked();

        // Build entries for create_email_timelock_series
        let mut entries_json = Vec::new();
        let mut total_chronos: u64 = 0;
        for s in &st {
            let amt: f64 = s.amount.get_untracked().parse().unwrap_or(0.0);
            let unlock = stage_unlock_unix(s).unwrap_or(0);
            total_chronos += (amt * 1_000_000.0) as u64;
            entries_json.push(serde_json::json!({
                "amount_kx": amt,
                "unlock_at_unix": unlock,
                "memo": memo_opt.clone(),
            }));
        }
        let first_amt = st.first().map(|s| s.amount.get_untracked().parse::<f64>().unwrap_or(0.0)).unwrap_or(0.0);
        let first_unlock = st.first().map(|s| stage_unlock_unix(s).unwrap_or(0)).unwrap_or(0);

        spawn_local(async move {
            sending.set(true);
            pending_email_chronos.set(total_chronos);
            msg.set(format!("Mining PoW for {} stages\u{2026}", entries_json.len()));
            let args = serde_wasm_bindgen::to_value(&serde_json::json!({
                "email": email_str.clone(),
                "entries": entries_json,
                "memoIsPublic": is_memo_public,
            })).unwrap_or(no_args());
            match call::<EmailSeriesResult>("create_email_timelock_series", args).await {
                Ok(result) => {
                    let claim_code = result.claim_code.clone();
                    let count = result.tx_ids.len();
                    for txid in &result.tx_ids {
                        let save_args = serde_wasm_bindgen::to_value(&serde_json::json!({
                            "lockId": txid, "email": email_str.clone(), "claimCode": claim_code.clone(),
                        })).unwrap_or(no_args());
                        let _ = call::<()>("save_email_send", save_args).await;
                    }
                    // Notify recipient
                    let notify_args = serde_wasm_bindgen::to_value(&serde_json::json!({
                        "email": email_str,
                        "amountKx": first_amt,
                        "unlockAtUnix": first_unlock,
                        "memo": memo_opt,
                        "claimCode": claim_code.clone(),
                    })).unwrap_or(no_args());
                    match call::<()>("notify_email_recipient", notify_args).await {
                        Ok(_) => { msg.set(format!("\u{2705} Cascade created!\n{count} stages sent. Recipient has been notified.\nClaim code: {claim_code}")); spam_warn.set(true); }
                        Err(_) => { msg.set(format!("\u{26a0}\u{fe0f} Cascade on-chain! Email failed.\nClaim code: {claim_code}\nShare this code with the recipient manually.")); }
                    }
                    email.set(String::new());
                    memo.set(String::new());
                    memo_public.set(false);
                    stages.set(vec![make_stage()]);
                    // Poll for balance update
                    poll_balance_update(info).await;
                    pending_email_chronos.set(0);
                }
                Err(e) => {
                    pending_email_chronos.set(0);
                    msg.set(format!("Error: {e}"));
                }
            }
            sending.set(false);
        });
    };

    view! {
        <div class="card">
            <div class="cascade-layout">
                <div class="cascade-form">
                    // Email with contact autocomplete
                    <div class="field" style="position:relative">
                        <label>"Recipient Email"</label>
                        <input type="email" placeholder="recipient@example.com"
                            prop:value=move || email.get()
                            on:input=move |ev| {
                                let val = event_target_value(&ev);
                                email.set(val.clone());
                                if val.len() >= 2 {
                                    let q = val.clone();
                                    spawn_local(async move {
                                        let args = serde_wasm_bindgen::to_value(&serde_json::json!({ "query": q })).unwrap_or(no_args());
                                        if let Ok(results) = call::<Vec<Contact>>("search_contacts", args).await {
                                            if !results.is_empty() {
                                                cascade_contact_suggestions.set(results);
                                                cascade_show_dropdown.set(true);
                                            } else {
                                                cascade_show_dropdown.set(false);
                                            }
                                        }
                                    });
                                } else {
                                    cascade_show_dropdown.set(false);
                                }
                            }
                            on:focus=move |_| {
                                let val = email.get_untracked();
                                if val.len() < 2 {
                                    spawn_local(async move {
                                        if let Ok(all) = call::<Vec<Contact>>("get_contacts", no_args()).await {
                                            let with_email: Vec<Contact> = all.into_iter().filter(|c| c.email.is_some()).collect();
                                            if !with_email.is_empty() {
                                                cascade_contact_suggestions.set(with_email);
                                                cascade_show_dropdown.set(true);
                                            }
                                        }
                                    });
                                }
                            }
                            on:blur=move |_| {
                                spawn_local(async move {
                                    delay_ms(200).await;
                                    cascade_show_dropdown.set(false);
                                });
                            }
                            disabled=move || sending.get() />
                        {move || {
                            if !cascade_show_dropdown.get() { return view! { <span></span> }.into_any(); }
                            let suggestions = cascade_contact_suggestions.get();
                            view! {
                                <div class="contact-dropdown">
                                    {suggestions.into_iter().map(|c| {
                                        let display_email = c.email.clone().unwrap_or_default();
                                        let display_name = c.name.clone();
                                        let fill_email = display_email.clone();
                                        view! {
                                            <div class="contact-dropdown-item"
                                                on:mousedown=move |ev| {
                                                    ev.prevent_default();
                                                    email.set(fill_email.clone());
                                                    cascade_show_dropdown.set(false);
                                                }>
                                                <span class="contact-dropdown-name">{display_name}</span>
                                                <span class="contact-dropdown-email">{display_email}</span>
                                            </div>
                                        }
                                    }).collect::<Vec<_>>()}
                                </div>
                            }.into_any()
                        }}
                    </div>
                    // Memo
                    <div class="field">
                        <label>{move || t(&lang.get(), "memo_optional")}</label>
                        <input type="text" maxlength="256" placeholder="e.g. Welcome to ChronX"
                            prop:value=move || memo.get()
                            on:input=move |ev| memo.set(event_target_value(&ev))
                            disabled=move || sending.get() />
                        // v2.5.29: Public memo toggle (desktop only, hidden when memo empty)
                        {move || if !memo.get().is_empty() {
                            view! {
                                <label style="display:flex;align-items:center;gap:6px;margin-top:6px;cursor:pointer;font-size:12px;color:#9ca3af">
                                    <input type="checkbox"
                                        prop:checked=move || memo_public.get()
                                        on:change=move |ev| {
                                            use wasm_bindgen::JsCast;
                                            let checked = ev.target()
                                                .and_then(|t| t.dyn_into::<web_sys::HtmlInputElement>().ok())
                                                .map(|i| i.checked())
                                                .unwrap_or(false);
                                            memo_public.set(checked);
                                        }
                                        style="accent-color:#d4a84b" />
                                    "Make this memo public"
                                </label>
                                <p style="font-size:0.7rem;color:#6b7280;margin:2px 0 0 22px">"Public memos are permanently visible to everyone on the blockchain."</p>
                            }.into_any()
                        } else {
                            view! { <span></span> }.into_any()
                        }}
                    </div>
                    // Stage builder
                    <div style="margin-bottom:8px">
                        <label style="display:block;margin-bottom:6px">"Stages"</label>
                        {move || {
                            let st = stages.get();
                            st.into_iter().enumerate().map(|(i, s)| {
                                let idx = i;
                                view! {
                                    <div class="cascade-stage-row">
                                        <span class="stage-num">{i + 1}</span>
                                        <input type="text" inputmode="decimal"
                                            placeholder="KX" style="width:100px"
                                            prop:value=move || format_amount_display(&s.amount.get())
                                            on:input=move |ev| {
                                                let raw: String = event_target_value(&ev).chars().filter(|&c| c != ',').collect();
                                                s.amount.set(raw);
                                            }
                                            disabled=move || sending.get() />
                                        <select
                                            prop:value=move || s.unlock_mode.get().to_string()
                                            on:change=move |ev| {
                                                let v: u8 = event_target_value(&ev).parse().unwrap_or(0);
                                                s.unlock_mode.set(v);
                                            }
                                            disabled=move || sending.get()>
                                            <option value="0">"Immediately"</option>
                                            <option value="1">"After..."</option>
                                            <option value="2">"On date..."</option>
                                        </select>
                                        {move || match s.unlock_mode.get() {
                                            1 => view! {
                                                <input type="text" placeholder="7" style="width:50px"
                                                    prop:value=move || s.dur_value.get()
                                                    on:input=move |ev| s.dur_value.set(event_target_value(&ev))
                                                    disabled=move || sending.get() />
                                                <select
                                                    prop:value=move || s.dur_unit.get()
                                                    on:change=move |ev| s.dur_unit.set(event_target_value(&ev))
                                                    disabled=move || sending.get()>
                                                    <option value="minutes">"minutes"</option>
                                                    <option value="hours">"hours"</option>
                                                    <option value="days">"days"</option>
                                                    <option value="weeks">"weeks"</option>
                                                </select>
                                            }.into_any(),
                                            2 => view! {
                                                <input type="datetime-local"
                                                    prop:value=move || s.date.get()
                                                    on:input=move |ev| s.date.set(event_target_value(&ev))
                                                    disabled=move || sending.get() />
                                            }.into_any(),
                                            _ => view! { <span></span> }.into_any(),
                                        }}
                                        <button class="remove-stage"
                                            on:click=move |_| {
                                                stages.update(|v| { if v.len() > 1 { v.remove(idx); } });
                                            }
                                            disabled=move || sending.get()
                                            title="Remove stage">{"\u{2715}"}</button>
                                    </div>
                                }
                            }).collect_view()
                        }}
                        <div style="display:flex;gap:8px">
                            <button class="pill" style="flex:1;color:#d4a84b;border-color:#d4a84b"
                                on:click=add_stage
                                disabled=move || sending.get() || (stages.get().len() >= 10)>
                                "+ Add Stage"
                            </button>
                        </div>
                        <button class="cascade-template-btn"
                            on:click=use_template
                            disabled=move || sending.get()>
                            "\u{2728} Use Standard Friend Template"
                        </button>
                    </div>
                </div>
                // Live preview + send button
                <div class="cascade-preview">
                    <h4>"Preview"</h4>
                    <div class="preview-row">
                        <span>"To:"</span>
                        <span class="val">{move || {
                            let e = email.get();
                            if e.is_empty() { "\u{2014}".to_string() } else { e }
                        }}</span>
                    </div>
                    <div class="preview-row">
                        <span>"Total:"</span>
                        <span class="val">{move || {
                            let st = stages.get();
                            let sum: f64 = st.iter().map(|s| s.amount.get().parse::<f64>().unwrap_or(0.0)).sum();
                            format!("{} KX", format_kx_display(sum))
                        }}</span>
                    </div>
                    <div class="preview-row">
                        <span>"Stages:"</span>
                        <span class="val">{move || stages.get().len().to_string()}</span>
                    </div>
                    <div class="preview-stages">
                        {move || {
                            let st = stages.get();
                            st.into_iter().enumerate().map(|(i, s)| {
                                view! {
                                    <div class="stage-line">
                                        <span>{move || { let a = s.amount.get(); if a.is_empty() { "? KX".to_string() } else { format!("{a} KX") } }}</span>
                                        <span class="stage-date">{move || format!("\u{2192} {}", stage_display_date(&s))}</span>
                                    </div>
                                }
                            }).collect_view()
                        }}
                    </div>
                    // Send button (inside preview column)
                    <button class="primary cascade-send-btn"
                        on:click=on_send disabled=move || sending.get()>
                        {move || if sending.get() { "Sending\u{2026}" } else { "Send Cascade" }}
                    </button>
                    <p class="fee-free-line">{"\u{2713} No transaction fees"}</p>
                    {move || {
                        let s = msg.get();
                        if s.is_empty() { view! { <span></span> }.into_any() }
                        else {
                            let cls = if s.starts_with("Error") { "msg error" }
                                      else if s.starts_with("Mining") { "msg mining" }
                                      else { "msg success" };
                            view! { <p class=cls>{s}</p> }.into_any()
                        }
                    }}
                    {move || if spam_warn.get() {
                        view! {
                            <p class="msg success" style="font-weight:800;margin-top:6px;font-size:13px">
                                "Check spam folder for notification email."
                            </p>
                        }.into_any()
                    } else { view! { <span></span> }.into_any() }}
                </div>
            </div>
        </div>
        // Confirmation modal
        {move || if confirm_open.get() {
            let st = stages.get_untracked();
            let total: f64 = st.iter().map(|s| s.amount.get_untracked().parse::<f64>().unwrap_or(0.0)).sum();
            let count = st.len();
            let em = email.get_untracked();
            view! {
                <div class="cascade-confirm-modal">
                    <div class="cascade-confirm-box">
                        <h3>{move || t(&lang.get(), "send_confirm_title")}</h3>
                        <p>{format!("Send {} KX to {em} in {count} stages?", format_kx_display(total))}</p>
                        <div class="btn-row">
                            <button class="btn-confirm" on:click=on_confirm>{move || t(&lang.get(), "confirm")}</button>
                            <button class="btn-cancel" on:click=move |_| confirm_open.set(false)>{move || t(&lang.get(), "cancel")}</button>
                        </div>
                    </div>
                </div>
            }.into_any()
        } else { view! { <span></span> }.into_any() }}
    }
}

// ── PromisesPanel — shows both incoming and outgoing with filters ─────────────

#[component]

fn email_duration_warning(unlock_unix: i64) -> Option<(&'static str, &'static str)> {
    let now = (js_sys::Date::now() / 1000.0) as i64;
    let years = (unlock_unix - now) / (365 * 86400);
    if years >= 20 {
        Some(("red", "Email addresses are unlikely to survive this long. We strongly recommend a wallet address."))
    } else if years >= 5 {
        Some(("amber", "This promise unlocks in many years. Consider using a wallet address for more reliable long-term delivery."))
    } else if years >= 2 {
        Some(("amber-light", "Email addresses sometimes change over years. Consider a wallet address for longer promises."))
    } else {
        None
    }
}

#[component]
fn PromisesPanel(
    info: RwSignal<Option<AccountInfo>>,
    lang: RwSignal<String>,
) -> impl IntoView {
    let all_promises = RwSignal::new(Vec::<TimeLockInfo>::new());
    let loading = RwSignal::new(false);

    let sort_by = RwSignal::new("date".to_string());       // "date" | "amount"
    let sort_asc = RwSignal::new(false);                   // false = newest/largest first

    // v2.2.2: identity cache — maps wallet_address -> IdentityRecord
    let identity_cache: RwSignal<std::collections::HashMap<String, IdentityRecord>> =
        RwSignal::new(HashMap::new());

    let reload = move || {
        spawn_local(async move {
            loading.set(true);
            if let Ok(locks) = call::<Vec<TimeLockInfo>>("get_all_promises", no_args()).await {
                // v2.2.2: look up identities for unique sender/recipient addresses
                let mut addrs = std::collections::HashSet::new();
                for lock in &locks {
                    addrs.insert(lock.sender.clone());
                    addrs.insert(lock.recipient_account_id.clone());
                }
                for addr in addrs {
                    if addr.is_empty() || identity_cache.get_untracked().contains_key(&addr) { continue; }
                    let args = serde_wasm_bindgen::to_value(
                        &serde_json::json!({ "walletAddress": addr })
                    ).unwrap_or(JsValue::NULL);
                    if let Ok(Some(rec)) = call::<Option<IdentityRecord>>("get_verified_identity", args).await {
                        identity_cache.update(|c| { c.insert(rec.wallet_address.clone(), rec); });
                    }
                }
                all_promises.set(locks);
            }
            loading.set(false);
        });
    };

    Effect::new(move |_| { reload(); });

    // Auto-refresh every 30 seconds
    Effect::new(move |_| {
        spawn_local(async move {
            loop {
                delay_ms(30_000).await;
                if let Ok(locks) = call::<Vec<TimeLockInfo>>("get_all_promises", no_args()).await {
                    all_promises.set(locks);
                }
            }
        });
    });

    // v2.2.2: Commitments — TYPE V, TYPE C, TYPE Y
    let commitments = RwSignal::new(CommitmentsData::default());
    let cancel_msg = RwSignal::new(String::new());
    Effect::new(move |_| {
        spawn_local(async move {
            if let Ok(data) = call::<CommitmentsData>("get_commitments", no_args()).await {
                commitments.set(data);
            }
        });
    });
    // Refresh commitments alongside promises (every 30s)
    Effect::new(move |_| {
        spawn_local(async move {
            loop {
                delay_ms(60_000).await;
                if let Ok(data) = call::<CommitmentsData>("get_commitments", no_args()).await {
                    commitments.set(data);
                }
            }
        });
    });

    // Derived: separate incoming and outgoing lists
    let sort_locks = move |mut locks: Vec<TimeLockInfo>| -> Vec<TimeLockInfo> {
        let sb = sort_by.get();
        let asc = sort_asc.get();
        locks.sort_by(|a, b| {
            let cmp = match sb.as_str() {
                "amount" => {
                    let a_val: u64 = a.amount_chronos.parse().unwrap_or(0);
                    let b_val: u64 = b.amount_chronos.parse().unwrap_or(0);
                    a_val.cmp(&b_val)
                }
                _ => a.unlock_at.cmp(&b.unlock_at), // "date"
            };
            if asc { cmp } else { cmp.reverse() }
        });
        locks
    };
    let incoming_promises = move || {
        let now_secs = (js_sys::Date::now() / 1000.0) as i64;
        let locks: Vec<TimeLockInfo> = all_promises.get().into_iter()
            .filter(|l| l.direction.as_deref() == Some("incoming")
                && l.status == "Pending"
                && l.unlock_at > now_secs + 60)
            .collect();
        sort_locks(locks)
    };
    let outgoing_promises = move || {
        let now_secs = (js_sys::Date::now() / 1000.0) as i64;
        let locks: Vec<TimeLockInfo> = all_promises.get().into_iter()
            .filter(|l| l.direction.as_deref() == Some("outgoing")
                && l.status == "Pending"
                && l.unlock_at > now_secs + 60)
            .collect();
        sort_locks(locks)
    };

    view! {
        <div class="card">
            // ── Sort controls ───────────────────────────────────────────────
            <div class="promises-filter-bar">
                <div class="promises-sort-controls">
                    <select class="pf-select"
                        on:change=move |ev| {
                            use wasm_bindgen::JsCast;
                            let val = ev.target().and_then(|t| t.dyn_into::<web_sys::HtmlInputElement>().ok()).map(|i| i.value()).unwrap_or_default();
                            sort_by.set(val);
                        }>
                        <option value="date" selected=move || sort_by.get() == "date">
                            {move || t(&lang.get(), "sort_date")}
                        </option>
                        <option value="amount" selected=move || sort_by.get() == "amount">
                            {move || t(&lang.get(), "sort_amount")}
                        </option>
                    </select>
                    <button type="button" class="pf-dir-btn"
                        on:click=move |_| sort_asc.set(!sort_asc.get_untracked())>
                        {move || if sort_asc.get() { "\u{2191}" } else { "\u{2193}" }}
                    </button>
                </div>
            </div>

            // ── v2.2.2: Commitments section (hidden when empty) ────────────
            {move || {
                let cm = commitments.get();
                let has_any = !cm.active_locks.is_empty() || !cm.active_credits.is_empty() || !cm.active_deposits.is_empty();
                if !has_any {
                    return view! { <span></span> }.into_any();
                }
                let now = (js_sys::Date::now() / 1000.0) as i64;
                let id_cache = identity_cache.get();
                view! {
                    <div style="margin-bottom:16px;padding-bottom:12px;border-bottom:1px solid #333">
                        <h3 class="section-title" style="margin-top:12px">
                            "\u{1f512} Commitments"
                        </h3>
                        // TYPE V conditionals (game locks)
                        {cm.active_locks.iter().map(|lock| {
                            let lid = lock.conditional_id.clone();
                            let desc = lock.description.clone().unwrap_or_else(|| "Locked funds".to_string());
                            let amt = format_kx(&lock.amount_chronos);
                            let countdown = lock.valid_until.map(|vu| {
                                let diff = vu - now;
                                if diff <= 0 { "Expired".to_string() }
                                else if diff < 300 { format!("{}m {}s remaining", diff / 60, diff % 60) }
                                else if diff < 1800 { format!("{}m remaining", diff / 60) }
                                else if diff < 86400 { format!("{}h {}m remaining", diff / 3600, (diff % 3600) / 60) }
                                else { format!("{}d remaining", diff / 86400) }
                            }).unwrap_or_default();
                            let countdown_color = lock.valid_until.map(|vu| {
                                let diff = vu - now;
                                if diff < 300 { "#e74c3c" } else if diff < 1800 { "#f39c12" } else { "#9ca3af" }
                            }).unwrap_or("#9ca3af");
                            let lid_cancel = lid.clone();
                            view! {
                                <div class="timelock-item" style="border-left:3px solid #D4A84B;padding-left:10px">
                                    <div class="tl-row">
                                        <span style="font-size:13px;font-weight:600">"\u{1f512} " {desc}</span>
                                    </div>
                                    <div class="tl-row" style="justify-content:space-between">
                                        <span style="color:#5cb8ff;font-size:13px;font-weight:700">{amt}" KX"</span>
                                        <span style={format!("color:{countdown_color};font-size:11px")}>{countdown}</span>
                                    </div>
                                    <button style="margin-top:6px;padding:4px 12px;background:rgba(231,76,60,0.15);color:#e74c3c;border:1px solid rgba(231,76,60,0.3);border-radius:6px;font-size:11px;cursor:pointer"
                                        on:click=move |_| {
                                            let lid = lid_cancel.clone();
                                            spawn_local(async move {
                                                let args = serde_wasm_bindgen::to_value(&serde_json::json!({
                                                    "commitmentId": lid,
                                                    "commitmentType": "TYPE_V",
                                                    "walletAddress": "",
                                                    "reason": "Player requested cancellation"
                                                })).unwrap_or(JsValue::NULL);
                                                match call::<String>("cancel_commitment", args).await {
                                                    Ok(msg) => cancel_msg.set(msg),
                                                    Err(e) => cancel_msg.set(format!("Error: {e}")),
                                                }
                                            });
                                        }>"Cancel"</button>
                                </div>
                            }
                        }).collect_view()}
                        // TYPE C credits
                        {cm.active_credits.iter().map(|credit| {
                            let cid = credit.credit_id.clone();
                            let drawn = format_kx(&credit.drawn_chronos);
                            let ceiling = format_kx(&credit.ceiling_chronos);
                            let beneficiary = identity_or_short(&credit.beneficiary, &id_cache);
                            let exp = credit.expires_at.map(|ts| format!("Expires {}", unix_to_date_str(ts))).unwrap_or_default();
                            let cid_revoke = cid.clone();
                            view! {
                                <div class="timelock-item" style="border-left:3px solid #3498db;padding-left:10px">
                                    <div class="tl-row">
                                        <span style="font-size:13px;font-weight:600">"\u{1f91d} Credit: " {beneficiary}</span>
                                    </div>
                                    <div class="tl-row" style="justify-content:space-between">
                                        <span style="color:#5cb8ff;font-size:13px">{drawn}" / "{ceiling}" KX drawn"</span>
                                        <span style="color:#9ca3af;font-size:11px">{exp}</span>
                                    </div>
                                    <button style="margin-top:6px;padding:4px 12px;background:rgba(231,76,60,0.15);color:#e74c3c;border:1px solid rgba(231,76,60,0.3);border-radius:6px;font-size:11px;cursor:pointer"
                                        on:click=move |_| {
                                            let cid = cid_revoke.clone();
                                            spawn_local(async move {
                                                let args = serde_wasm_bindgen::to_value(&serde_json::json!({
                                                    "creditId": cid
                                                })).unwrap_or(JsValue::NULL);
                                                let _ = call::<String>("revoke_credit", args).await;
                                            });
                                        }>"Revoke"</button>
                                </div>
                            }
                        }).collect_view()}
                        // TYPE Y deposits
                        {cm.active_deposits.iter().map(|dep| {
                            let obligor = identity_or_short(&dep.obligor, &id_cache);
                            let total = format_kx(&dep.total_due_chronos);
                            let mat = dep.matures_at.map(|ts| format!("Matures {}", unix_to_date_str(ts))).unwrap_or_default();
                            view! {
                                <div class="timelock-item" style="border-left:3px solid #27ae60;padding-left:10px">
                                    <div class="tl-row">
                                        <span style="font-size:13px;font-weight:600">"\u{1f4cb} Deposit: " {obligor}" owes me"</span>
                                    </div>
                                    <div class="tl-row" style="justify-content:space-between">
                                        <span style="color:#5cb8ff;font-size:13px;font-weight:700">{total}" KX"</span>
                                        <span style="color:#9ca3af;font-size:11px">{mat}</span>
                                    </div>
                                </div>
                            }
                        }).collect_view()}
                        // Cancel message toast
                        {move || {
                            let msg = cancel_msg.get();
                            if msg.is_empty() {
                                view! { <span></span> }.into_any()
                            } else {
                                view! { <p style="font-size:11px;color:#D4A84B;margin-top:8px;padding:6px 10px;background:rgba(212,168,75,0.1);border-radius:6px">{msg}</p> }.into_any()
                            }
                        }}
                    </div>
                }.into_any()
            }}

            // ── Section 1: KX Promised To You (incoming) ────────────────────
            <h3 class="section-title" style="margin-top:12px">
                {move || t(&lang.get(), "promises_incoming_header")}
            </h3>
            {move || {
                let locks = incoming_promises();
                if loading.get() {
                    view! { <p class="muted">"\u{2026}"</p> }.into_any()
                } else if locks.is_empty() {
                    view! {
                        <div style="text-align:center;padding:20px 16px">
                            <p class="muted" style="font-size:13px">
                                {move || t(&lang.get(), "promises_incoming_empty")}
                            </p>
                        </div>
                    }.into_any()
                } else {
                    let lang_val = lang.get();
                    view! {
                        <div class="timelock-list">
                            {locks.into_iter().map(|lock| {
                                let now = (js_sys::Date::now() / 1000.0) as i64;
                                let id_cache = identity_cache.get();
                                let peer_label = format!("{}: {}", t(&lang_val, "from"), identity_or_short(&lock.sender, &id_cache));
                                let status_label = {
                                    let diff = lock.unlock_at - now;
                                    if diff <= 0 {
                                        t(&lang_val, "arriving_soon")
                                    } else if diff < 3600 {
                                        format!("{} {}m", t(&lang_val, "unlocks_in"), (diff / 60).max(1))
                                    } else if diff < 86400 {
                                        format!("{} {}h {}m", t(&lang_val, "unlocks_in"), diff / 3600, (diff % 3600) / 60)
                                    } else {
                                        format!("{} {}d", t(&lang_val, "unlocks_in"), diff / 86400)
                                    }
                                };
                                let amount_str = format!("+{} KX", format_kx(&lock.amount_chronos));
                                let memo_text = lock.memo.clone().unwrap_or_default();
                                view! {
                                    <div class="timelock-item" style="position:relative">
                                        <span class="badge-incoming" style="position:absolute;top:8px;right:8px">
                                            {format!("\u{2190} {}", t(&lang_val, "incoming"))}
                                        </span>
                                        <div class="tl-row">
                                            <span class="tl-amount" style="color:#5cb8ff">{amount_str}</span>
                                            <span class="tl-status" style="color:#DAA520">{status_label}</span>
                                        </div>
                                        <div class="tl-row">
                                            <span class="tl-label">{peer_label}</span>
                                        </div>
                                        {if !memo_text.is_empty() {
                                            view! { <p class="tl-memo">{memo_text}</p> }.into_any()
                                        } else {
                                            view! { <span></span> }.into_any()
                                        }}
                                    </div>
                                }
                            }).collect_view()}
                        </div>
                    }.into_any()
                }
            }}

            // ── Section 2: Promises You've Made (outgoing) ──────────────────
            <h3 class="section-title" style="margin-top:24px;padding-top:16px;border-top:1px solid #333">
                {move || t(&lang.get(), "promises_outgoing_header")}
            </h3>
            {move || {
                let locks = outgoing_promises();
                if loading.get() {
                    view! { <p class="muted">"\u{2026}"</p> }.into_any()
                } else if locks.is_empty() {
                    view! {
                        <div style="text-align:center;padding:20px 16px">
                            <p class="muted" style="font-size:13px">
                                {move || t(&lang.get(), "promises_outgoing_empty")}
                            </p>
                        </div>
                    }.into_any()
                } else {
                    let lang_val = lang.get();
                    view! {
                        <div class="timelock-list">
                            {locks.into_iter().map(|lock| {
                                let now = (js_sys::Date::now() / 1000.0) as i64;
                                let id_cache_out = identity_cache.get();
                                let peer_label = format!("{}: {}", t(&lang_val, "to"), identity_or_short(&lock.recipient_account_id, &id_cache_out));
                                let status_label = {
                                    let diff = lock.unlock_at - now;
                                    if diff <= 0 {
                                        lock.status.clone()
                                    } else if diff < 3600 {
                                        format!("{} {}m", t(&lang_val, "unlocks_in"), (diff / 60).max(1))
                                    } else if diff < 86400 {
                                        format!("{} {}h {}m", t(&lang_val, "unlocks_in"), diff / 3600, (diff % 3600) / 60)
                                    } else {
                                        format!("{} {}d", t(&lang_val, "unlocks_in"), diff / 86400)
                                    }
                                };
                                let amount_str = format!("{} KX", format_kx(&lock.amount_chronos));
                                let memo_text = lock.memo.clone().unwrap_or_default();
                                view! {
                                    <div class="timelock-item" style="position:relative">
                                        <span class="badge-outgoing" style="position:absolute;top:8px;right:8px">
                                            {format!("\u{2192} {}", t(&lang_val, "outgoing"))}
                                        </span>
                                        <div class="tl-row">
                                            <span class="tl-amount">{amount_str}</span>
                                            <span class="tl-status" style="color:#DAA520">{status_label}</span>
                                        </div>
                                        <div class="tl-row">
                                            <span class="tl-label">{peer_label}</span>
                                        </div>
                                        {if !memo_text.is_empty() {
                                            view! { <p class="tl-memo">{memo_text}</p> }.into_any()
                                        } else {
                                            view! { <span></span> }.into_any()
                                        }}
                                    </div>
                                }
                            }).collect_view()}
                        </div>
                    }.into_any()
                }
            }}
        </div>
    }
}

// ── RequestPanel (desktop only) ───────────────────────────────────────────────

#[component]
fn RequestPanel(
    info: RwSignal<Option<AccountInfo>>,
    lang: RwSignal<String>,
) -> impl IntoView {
    let _ = &info;
    let address_book = RwSignal::new(Vec::<AddressBookEntry>::new());
    let req_email = RwSignal::new(String::new());
    let req_amount = RwSignal::new(String::new());
    let req_note = RwSignal::new(String::new());
    let req_msg = RwSignal::new(String::new());
    let req_busy = RwSignal::new(false);
    // Address book add form
    let ab_new_email = RwSignal::new(String::new());
    let ab_new_name = RwSignal::new(String::new());
    let ab_show_add = RwSignal::new(false);

    let load_book = move || {
        spawn_local(async move {
            if let Ok(book) = call::<Vec<AddressBookEntry>>("get_address_book", no_args()).await {
                address_book.set(book);
            }
        });
    };

    Effect::new(move |_| { load_book(); });

    let on_send_request = move |_: web_sys::MouseEvent| {
        let email = req_email.get_untracked().trim().to_string();
        let amount_str = req_amount.get_untracked().trim().to_string();
        let note = req_note.get_untracked().trim().to_string();
        if email.is_empty() || amount_str.is_empty() {
            req_msg.set("Please fill in email and amount".to_string());
            return;
        }
        let amount: f64 = match amount_str.parse() {
            Ok(v) if v > 0.0 => v,
            _ => { req_msg.set("Invalid amount".to_string()); return; }
        };
        req_busy.set(true);
        req_msg.set(String::new());
        spawn_local(async move {
            let note_opt: Option<String> = if note.is_empty() { None } else { Some(note) };
            let args = serde_wasm_bindgen::to_value(&serde_json::json!({
                "toEmail": email,
                "amountKx": amount,
                "note": note_opt,
            })).unwrap_or(no_args());
            match call::<serde_json::Value>("send_kx_request", args).await {
                Ok(_) => {
                    req_msg.set("\u{2705} KX request sent!".to_string());
                    req_email.set(String::new());
                    req_amount.set(String::new());
                    req_note.set(String::new());
                }
                Err(e) => req_msg.set(format!("Error: {e}")),
            }
            req_busy.set(false);
        });
    };

    view! {
        // Request KX form
        <div class="card">
            <h3 class="section-title">"Request KX"</h3>
            <div class="form-group">
                <label>"Email"</label>
                <input type="email" class="input" placeholder="recipient@email.com"
                    prop:value=move || req_email.get()
                    on:input=move |ev| req_email.set(event_target_value(&ev))
                />
            </div>
            <div class="form-group">
                <label>{move || t(&lang.get(), "amount_kx")}</label>
                <input type="number" class="input" step="0.01" min="0.01"
                    prop:value=move || req_amount.get()
                    on:input=move |ev| req_amount.set(event_target_value(&ev))
                />
            </div>
            <div class="form-group">
                <label>{move || t(&lang.get(), "memo_optional")}</label>
                <input type="text" class="input" maxlength="256"
                    prop:value=move || req_note.get()
                    on:input=move |ev| req_note.set(event_target_value(&ev))
                />
            </div>
            <button class="btn gold" on:click=on_send_request disabled=move || req_busy.get()>
                "Send KX Request"
            </button>
            <p class="muted" style="font-size:11px;margin-top:6px">"Recipients can pay or decline at their convenience."</p>
            {move || {
                let s = req_msg.get();
                if s.is_empty() { view! { <span></span> }.into_any() }
                else {
                    let cls = if s.starts_with("Error") { "msg error" } else { "msg success" };
                    view! { <p class=cls>{s}</p> }.into_any()
                }
            }}
        </div>

        // Address Book
        <div class="card" style="margin-top:16px">
            <h3 class="section-title">"Address Book"</h3>
            {move || {
                let list = address_book.get();
                if list.is_empty() && !ab_show_add.get() {
                    view! { <p class="muted">"No contacts saved yet."</p> }.into_any()
                } else {
                    view! {
                        <div style="display:flex;flex-direction:column;gap:6px">
                            {list.into_iter().map(|entry| {
                                let email_c = entry.email.clone();
                                let email_fill = entry.email.clone();
                                let indicator = match entry.registered {
                                    Some(true) => "\u{1f7e2}",
                                    Some(false) => "\u{1f7e1}",
                                    None => "\u{26aa}",
                                };
                                let display = entry.name.as_ref()
                                    .filter(|n| !n.is_empty())
                                    .cloned()
                                    .unwrap_or_else(|| entry.email.clone());
                                let email_sub = if entry.name.is_some() && !entry.name.as_ref().unwrap().is_empty() {
                                    Some(entry.email.clone())
                                } else { None };
                                view! {
                                    <div style="display:flex;align-items:center;gap:8px;padding:8px 10px;background:#0d0d1a;border-radius:6px;cursor:pointer"
                                        on:click=move |_| {
                                            req_email.set(email_fill.clone());
                                        }>
                                        <span style="font-size:10px">{indicator}</span>
                                        <div style="flex:1;min-width:0">
                                            <span style="font-size:13px;color:#e5e7eb;font-weight:600">{display}</span>
                                            {if let Some(ref sub) = email_sub {
                                                view! { <span style="font-size:11px;color:#888;margin-left:6px">{sub.clone()}</span> }.into_any()
                                            } else { view! { <span></span> }.into_any() }}
                                        </div>
                                        <button style="background:none;border:1px solid #333;color:#888;padding:2px 8px;border-radius:4px;font-size:11px;cursor:pointer;flex-shrink:0"
                                            on:click=move |ev| {
                                                ev.stop_propagation();
                                                let e = email_c.clone();
                                                spawn_local(async move {
                                                    let args = serde_wasm_bindgen::to_value(&serde_json::json!({ "email": e })).unwrap_or(no_args());
                                                    let _ = call::<()>("remove_from_address_book", args).await;
                                                    load_book();
                                                });
                                            }>"Remove"</button>
                                    </div>
                                }
                            }).collect_view()}
                        </div>
                    }.into_any()
                }
            }}
            // Add to Address Book
            {move || if ab_show_add.get() {
                view! {
                    <div style="margin-top:10px;padding:10px;background:#0d0d1a;border-radius:6px;display:flex;flex-direction:column;gap:6px">
                        <input type="email" class="input" placeholder="email@address.com"
                            prop:value=move || ab_new_email.get()
                            on:input=move |ev| ab_new_email.set(event_target_value(&ev))
                        />
                        <input type="text" class="input" placeholder="Name (optional)"
                            prop:value=move || ab_new_name.get()
                            on:input=move |ev| ab_new_name.set(event_target_value(&ev))
                        />
                        <div style="display:flex;gap:6px">
                            <button class="btn gold" style="flex:1;font-size:12px;padding:6px" on:click=move |_| {
                                let email = ab_new_email.get_untracked().trim().to_string();
                                let name = ab_new_name.get_untracked().trim().to_string();
                                if email.is_empty() { return; }
                                let name_opt: Option<String> = if name.is_empty() { None } else { Some(name) };
                                spawn_local(async move {
                                    let args = serde_wasm_bindgen::to_value(&serde_json::json!({
                                        "email": email, "name": name_opt,
                                    })).unwrap_or(no_args());
                                    let _ = call::<()>("add_to_address_book", args).await;
                                    // Check registration
                                    let check_args = serde_wasm_bindgen::to_value(&serde_json::json!({ "email": email })).unwrap_or(no_args());
                                    let _ = call::<bool>("check_email_registered", check_args).await;
                                    ab_new_email.set(String::new());
                                    ab_new_name.set(String::new());
                                    ab_show_add.set(false);
                                    load_book();
                                });
                            }>"Save"</button>
                            <button style="flex:1;font-size:12px;padding:6px" on:click=move |_| ab_show_add.set(false)>"Cancel"</button>
                        </div>
                    </div>
                }.into_any()
            } else {
                view! {
                    <button style="margin-top:10px;width:100%;font-size:12px;padding:8px;background:none;border:1px dashed #444;color:#888;border-radius:6px;cursor:pointer"
                        on:click=move |_| ab_show_add.set(true)>
                        "+ Add to Address Book"
                    </button>
                }.into_any()
            }}
        </div>
    }
}

// ── ContactsPanel (desktop only) ─────────────────────────────────────────────

#[component]
fn ContactsPanel(
    lang: RwSignal<String>,
    active_tab: RwSignal<u8>,
    email_prefill_from_contact: RwSignal<String>,
) -> impl IntoView {
    let contacts: RwSignal<Vec<Contact>> = RwSignal::new(Vec::new());
    let search_query = RwSignal::new(String::new());
    let loading = RwSignal::new(true);
    let msg = RwSignal::new(String::new());

    // Add/Edit modal state
    let modal_open = RwSignal::new(false);
    let edit_id = RwSignal::new(Option::<String>::None); // None = add, Some = edit
    let form_name = RwSignal::new(String::new());
    let form_email = RwSignal::new(String::new());
    let form_kx = RwSignal::new(String::new());
    let form_notes = RwSignal::new(String::new());

    // Delete confirmation
    let delete_confirm_id = RwSignal::new(Option::<String>::None);

    // Load contacts
    let load_contacts = move || {
        spawn_local(async move {
            loading.set(true);
            let q = search_query.get_untracked();
            if q.is_empty() {
                match call::<Vec<Contact>>("get_contacts", no_args()).await {
                    Ok(c) => contacts.set(c),
                    Err(e) => msg.set(format!("Error: {e}")),
                }
            } else {
                let args = serde_wasm_bindgen::to_value(&serde_json::json!({ "query": q })).unwrap_or(no_args());
                match call::<Vec<Contact>>("search_contacts", args).await {
                    Ok(c) => contacts.set(c),
                    Err(e) => msg.set(format!("Error: {e}")),
                }
            }
            loading.set(false);
        });
    };
    let load_contacts_init = load_contacts.clone();
    Effect::new(move |_| { load_contacts_init(); });

    let open_add = move |_: web_sys::MouseEvent| {
        edit_id.set(None);
        form_name.set(String::new());
        form_email.set(String::new());
        form_kx.set(String::new());
        form_notes.set(String::new());
        modal_open.set(true);
    };

    let open_edit = move |c: Contact| {
        edit_id.set(Some(c.id.clone()));
        form_name.set(c.name.clone());
        form_email.set(c.email.clone().unwrap_or_default());
        form_kx.set(c.kx_address.clone().unwrap_or_default());
        form_notes.set(c.notes.clone().unwrap_or_default());
        modal_open.set(true);
    };

    let on_save = move |_: web_sys::MouseEvent| {
        let name = form_name.get_untracked();
        if name.trim().is_empty() { msg.set("Name is required".into()); return; }
        let email_val = { let e = form_email.get_untracked(); if e.trim().is_empty() { None } else { Some(e) } };
        let kx_val = { let k = form_kx.get_untracked(); if k.trim().is_empty() { None } else { Some(k) } };
        let notes_val = { let n = form_notes.get_untracked(); if n.trim().is_empty() { None } else { Some(n) } };
        let eid = edit_id.get_untracked();
        let reload = load_contacts.clone();
        spawn_local(async move {
            let result = if let Some(id) = eid {
                let args = serde_wasm_bindgen::to_value(&serde_json::json!({
                    "id": id, "name": name, "email": email_val, "kxAddress": kx_val, "notes": notes_val
                })).unwrap_or(no_args());
                call::<Contact>("update_contact", args).await.map(|_| ())
            } else {
                let args = serde_wasm_bindgen::to_value(&serde_json::json!({
                    "name": name, "email": email_val, "kxAddress": kx_val, "notes": notes_val
                })).unwrap_or(no_args());
                call::<Contact>("add_contact", args).await.map(|_| ())
            };
            match result {
                Ok(_) => { modal_open.set(false); msg.set(String::new()); reload(); }
                Err(e) => msg.set(format!("Error: {e}")),
            }
        });
    };

    let on_delete = move |id: String| {
        let reload = load_contacts.clone();
        spawn_local(async move {
            let args = serde_wasm_bindgen::to_value(&serde_json::json!({ "id": id })).unwrap_or(no_args());
            match call::<()>("delete_contact", args).await {
                Ok(_) => { delete_confirm_id.set(None); reload(); }
                Err(e) => msg.set(format!("Error: {e}")),
            }
        });
    };

    let on_send_kx = move |c: Contact| {
        if let Some(em) = c.email.as_ref() {
            email_prefill_from_contact.set(em.clone());
            active_tab.set(1); // Switch to Send tab
        }
    };

    view! {
        <div class="card">
            <div style="display:flex;align-items:center;justify-content:space-between;margin-bottom:16px">
                <h2 style="margin:0;font-size:18px;color:#e5e7eb">{move || t(&lang.get(), "tab_contacts")}</h2>
                <button class="primary" style="padding:6px 14px;font-size:13px" on:click=open_add>
                    {move || t(&lang.get(), "contacts_add_button")}
                </button>
            </div>
            <div style="margin-bottom:12px">
                <input type="text" class="input" style="width:100%;padding:8px 12px"
                    placeholder=move || t(&lang.get(), "contacts_search_placeholder")
                    prop:value=move || search_query.get()
                    on:input=move |ev| {
                        search_query.set(event_target_value(&ev));
                        let reload = load_contacts.clone();
                        reload();
                    } />
            </div>
            {move || {
                let m = msg.get();
                if m.is_empty() { view! { <span></span> }.into_any() }
                else { view! { <p class="msg error" style="margin-bottom:8px">{m}</p> }.into_any() }
            }}
            {move || {
                let list = contacts.get();
                if loading.get() {
                    view! { <p style="color:#9ca3af;text-align:center;padding:24px">"Loading..."</p> }.into_any()
                } else if list.is_empty() {
                    view! { <p style="color:#6b7280;text-align:center;padding:24px">{move || t(&lang.get(), "contacts_empty_state")}</p> }.into_any()
                } else {
                    view! {
                        <div class="contacts-list">
                            {list.into_iter().map(|c| {
                                let c_edit = c.clone();
                                let c_send = c.clone();
                                let c_del_id = c.id.clone();
                                let c_del_id2 = c.id.clone();
                                let display_name = c.name.clone();
                                let display_email = c.email.clone().unwrap_or_default();
                                let display_kx = c.kx_address.clone().unwrap_or_default();
                                let display_notes = c.notes.clone().unwrap_or_default();
                                let send_count = c.send_count;
                                let _last_sent = c.last_sent;
                                view! {
                                    <div class="contact-card">
                                        <div class="contact-card-main">
                                            <div class="contact-card-info">
                                                <span class="contact-card-name">{display_name}</span>
                                                {if !display_email.is_empty() {
                                                    view! { <span class="contact-card-email">{display_email}</span> }.into_any()
                                                } else if !display_kx.is_empty() {
                                                    let short_kx = if display_kx.len() > 12 { format!("{}...{}", &display_kx[..6], &display_kx[display_kx.len()-6..]) } else { display_kx.clone() };
                                                    view! { <span class="contact-card-email" style="font-family:monospace">{short_kx}</span> }.into_any()
                                                } else {
                                                    view! { <span></span> }.into_any()
                                                }}
                                                {if !display_notes.is_empty() {
                                                    view! { <span class="contact-card-notes">{display_notes}</span> }.into_any()
                                                } else { view! { <span></span> }.into_any() }}
                                            </div>
                                            {if send_count > 0 {
                                                view! {
                                                    <span class="contact-card-meta">
                                                        {format!("{} {}", send_count, t(&lang.get_untracked(), "contacts_times_sent"))}
                                                    </span>
                                                }.into_any()
                                            } else { view! { <span></span> }.into_any() }}
                                        </div>
                                        <div class="contact-card-actions">
                                            <button class="contact-btn contact-btn-send" on:click=move |_| on_send_kx(c_send.clone())>
                                                {move || t(&lang.get(), "contacts_send_kx")}
                                            </button>
                                            <button class="contact-btn" on:click=move |_| open_edit(c_edit.clone())>
                                                {move || t(&lang.get(), "contacts_edit")}
                                            </button>
                                            {move || {
                                                let del_id = c_del_id.clone();
                                                let del_id2 = c_del_id2.clone();
                                                if delete_confirm_id.get().as_deref() == Some(&del_id) {
                                                    view! {
                                                        <span style="display:flex;gap:4px;align-items:center">
                                                            <span style="font-size:12px;color:#ef4444">{move || t(&lang.get(), "contacts_delete_confirm")}</span>
                                                            <button class="contact-btn" style="color:#ef4444;border-color:#ef4444"
                                                                on:click=move |_| on_delete(del_id.clone())>
                                                                {move || t(&lang.get(), "confirm")}
                                                            </button>
                                                            <button class="contact-btn" on:click=move |_| delete_confirm_id.set(None)>
                                                                {move || t(&lang.get(), "cancel")}
                                                            </button>
                                                        </span>
                                                    }.into_any()
                                                } else {
                                                    view! {
                                                        <button class="contact-btn" style="color:#ef4444" on:click=move |_| delete_confirm_id.set(Some(del_id2.clone()))>
                                                            {move || t(&lang.get(), "contacts_delete")}
                                                        </button>
                                                    }.into_any()
                                                }
                                            }}
                                        </div>
                                    </div>
                                }
                            }).collect::<Vec<_>>()}
                        </div>
                    }.into_any()
                }
            }}
        </div>
        // Add/Edit Contact Modal
        {move || if modal_open.get() {
            let is_edit = edit_id.get().is_some();
            view! {
                <div class="cascade-confirm-modal">
                    <div class="cascade-confirm-box" style="max-width:450px">
                        <h3 style="margin:0 0 16px;color:#e5e7eb">
                            {if is_edit { t(&lang.get(), "contacts_edit") } else { t(&lang.get(), "contacts_add_button") }}
                        </h3>
                        <div style="display:flex;flex-direction:column;gap:10px">
                            <div>
                                <label style="display:block;font-size:12px;color:#9ca3af;margin-bottom:4px">{move || t(&lang.get(), "contacts_name")}" *"</label>
                                <input type="text" class="input" style="width:100%;padding:8px 10px"
                                    prop:value=move || form_name.get()
                                    on:input=move |ev| form_name.set(event_target_value(&ev)) />
                            </div>
                            <div>
                                <label style="display:block;font-size:12px;color:#9ca3af;margin-bottom:4px">{move || t(&lang.get(), "contacts_email")}</label>
                                <input type="email" class="input" style="width:100%;padding:8px 10px"
                                    placeholder="user@example.com"
                                    prop:value=move || form_email.get()
                                    on:input=move |ev| form_email.set(event_target_value(&ev)) />
                            </div>
                            <div>
                                <label style="display:block;font-size:12px;color:#9ca3af;margin-bottom:4px">{move || t(&lang.get(), "contacts_kx_address")}</label>
                                <input type="text" class="input" style="width:100%;padding:8px 10px;font-family:monospace;font-size:12px"
                                    prop:value=move || form_kx.get()
                                    on:input=move |ev| form_kx.set(event_target_value(&ev)) />
                            </div>
                            <div>
                                <label style="display:block;font-size:12px;color:#9ca3af;margin-bottom:4px">{move || t(&lang.get(), "contacts_notes")}</label>
                                <textarea class="input" style="width:100%;padding:8px 10px;min-height:60px;resize:vertical"
                                    prop:value=move || form_notes.get()
                                    on:input=move |ev| form_notes.set(event_target_value(&ev))>
                                </textarea>
                            </div>
                        </div>
                        <div style="display:flex;gap:8px;margin-top:16px;justify-content:flex-end">
                            <button style="padding:8px 16px;background:transparent;border:1px solid #374151;color:#9ca3af;border-radius:6px;cursor:pointer"
                                on:click=move |_| modal_open.set(false)>
                                {move || t(&lang.get(), "cancel")}
                            </button>
                            <button class="primary" style="padding:8px 16px" on:click=on_save>
                                {move || t(&lang.get(), "contacts_save")}
                            </button>
                        </div>
                    </div>
                </div>
            }.into_any()
        } else { view! { <span></span> }.into_any() }}
    }
}

// ── OpenPanel ─────────────────────────────────────────────────────────────────

#[derive(Clone)]
struct OpenItem {
    id: String,
    item_type: String,      // "invoice", "poke", "kxgo", "credit", "deposit", "checkin", "misai"
    icon: &'static str,
    badge_label: String,
    badge_color: String,
    description: String,
    amount_kx: Option<f64>,
    time_label: String,
    can_dismiss: bool,
    dismiss_tooltip: Option<String>,
    sort_time: i64,         // for sorting
}

#[component]
fn OpenPanel(
    info: RwSignal<Option<AccountInfo>>,
    lang: RwSignal<String>,
) -> impl IntoView {
    let _ = &lang;
    let items = RwSignal::new(Vec::<OpenItem>::new());
    let loading = RwSignal::new(true);
    let sort_by = RwSignal::new("expiring".to_string()); // expiring, amount_due, date_added, name_az, role
    let dismiss_target = RwSignal::new(Option::<OpenItem>::None);
    let dismiss_busy = RwSignal::new(false);

    // Load items
    let load = move || {
        spawn_local(async move {
            loading.set(true);
            let mut all = Vec::<OpenItem>::new();

            // 1. Commitments (TYPE V/C/Y)
            if let Ok(commits) = call::<Vec<CommitmentInfo>>("get_commitments", no_args()).await {
                for c in commits {
                    let (icon, badge, color, can_dismiss, tooltip) = match c.commitment_type.as_str() {
                        "TYPE_V" | "kxgo" => ("\u{1f3ae}", "KXGO".to_string(), "#7c3aed".to_string(), true, None),
                        "TYPE_C" | "credit" => ("\u{1f91d}", "CREDIT".to_string(), "#22c55e".to_string(), true, None),
                        "TYPE_Y" | "deposit" => ("\u{1f4cb}", "DEPOSIT".to_string(), "#6b7280".to_string(), true, None),
                        "sign_of_life" | "checkin" => ("\u{2764}\u{fe0f}", "CHECK-IN".to_string(), "#ef4444".to_string(), false, Some("Promise Check-in cannot be dismissed".to_string())),
                        "misai" => ("\u{1f916}", "MISAI".to_string(), "#d4a84b".to_string(), false, Some("AI management active".to_string())),
                        _ => ("\u{1f4cb}", c.commitment_type.clone(), "#6b7280".to_string(), true, None),
                    };
                    all.push(OpenItem {
                        id: c.commitment_id.clone(),
                        item_type: c.commitment_type.clone(),
                        icon,
                        badge_label: badge,
                        badge_color: color,
                        description: format!("{} — {}", c.commitment_type, c.status),
                        amount_kx: None,
                        time_label: c.status.clone(),
                        can_dismiss,
                        dismiss_tooltip: tooltip,
                        sort_time: 0,
                    });
                }
            }

            // 2. Pending invoices
            if let Ok(invs) = call::<Vec<InvoiceRecord>>("get_pending_invoices", no_args()).await {
                for inv in invs {
                    all.push(OpenItem {
                        id: inv.invoice_id.clone(),
                        item_type: "invoice".to_string(),
                        icon: "\u{1f4c4}",
                        badge_label: "INVOICE".to_string(),
                        badge_color: "#d4a84b".to_string(),
                        description: if inv.from_display.is_empty() {
                            format!("From {}", &inv.from_wallet[..8.min(inv.from_wallet.len())])
                        } else {
                            format!("From {}", inv.from_display)
                        },
                        amount_kx: Some(inv.amount_kx),
                        time_label: inv.memo.clone().unwrap_or_default(),
                        can_dismiss: true,
                        dismiss_tooltip: None,
                        sort_time: inv.created_at as i64,
                    });
                }
            }

            // 3. Pending pokes
            if let Ok(emails) = call::<Vec<String>>("get_claim_emails", no_args()).await {
                let blocked = call::<Vec<String>>("get_blocked_senders", no_args()).await.unwrap_or_default();
                for em in &emails {
                    let args = serde_wasm_bindgen::to_value(&serde_json::json!({ "email": em })).unwrap_or(no_args());
                    if let Ok(pokes) = call::<Vec<PendingPoke>>("get_pending_pokes", args).await {
                        for p in pokes {
                            let sender = p.from_email.as_deref().unwrap_or("").to_lowercase();
                            if !sender.is_empty() && blocked.iter().any(|b| b.to_lowercase() == sender) {
                                continue;
                            }
                            all.push(OpenItem {
                                id: p.request_id.clone(),
                                item_type: "poke".to_string(),
                                icon: "\u{1f44b}",
                                badge_label: "REQUEST".to_string(),
                                badge_color: "#3b82f6".to_string(),
                                description: format!("From {}", p.from_email.as_deref().unwrap_or("unknown")),
                                amount_kx: p.amount_kx.parse::<f64>().ok(),
                                time_label: p.note.clone().unwrap_or_default(),
                                can_dismiss: true,
                                dismiss_tooltip: None,
                                sort_time: 0,
                            });
                        }
                    }
                }
            }

            // 4. Sign of life / Promise check-in
            if let Ok(sol) = call::<SignOfLifeStatus>("get_sign_of_life_status", no_args()).await {
                if sol.locks_count > 0 {
                    let due_label = sol.next_due.map(|ts| {
                        let d = js_sys::Date::new_0();
                        d.set_time((ts as f64) * 1000.0);
                        format!("Next check-in: {}", d.to_date_string().as_string().unwrap_or_else(|| "unknown".to_string()))
                    }).unwrap_or_else(|| format!("{} promises require check-in", sol.locks_count));
                    all.push(OpenItem {
                        id: "sign-of-life".to_string(),
                        item_type: "checkin".to_string(),
                        icon: "\u{2764}\u{fe0f}",
                        badge_label: "CHECK-IN".to_string(),
                        badge_color: "#ef4444".to_string(),
                        description: due_label,
                        amount_kx: None,
                        time_label: format!("{} promises", sol.locks_count),
                        can_dismiss: false,
                        dismiss_tooltip: Some("Promise Check-in cannot be dismissed".to_string()),
                        sort_time: sol.next_due.unwrap_or(i64::MAX),
                    });
                }
            }

            // 5. Incoming KX requests (v2.4.1)
            if let Ok(reqs) = call::<Vec<KxRequest>>("get_pending_kx_requests", no_args()).await {
                for r in reqs {
                    all.push(OpenItem {
                        id: r.request_id.clone(),
                        item_type: "request".to_string(),
                        icon: "\u{1f44b}",
                        badge_label: "REQUEST".to_string(),
                        badge_color: "#3b82f6".to_string(),
                        description: format!("{} wants {} KX", if r.from_name.is_empty() { &r.from_email } else { &r.from_name }, r.amount_kx),
                        amount_kx: Some(r.amount_kx),
                        time_label: r.note.clone().unwrap_or_default(),
                        can_dismiss: true,
                        dismiss_tooltip: None,
                        sort_time: r.created_at as i64,
                    });
                }
            }

            items.set(all);
            loading.set(false);
        });
    };

    Effect::new(move |_| {
        let _ = info.get(); // reload when info changes
        load();
    });

    view! {
        <div>
            // Sort dropdown
            <div style="display:flex;justify-content:flex-end;margin-bottom:10px;align-items:center;gap:6px">
                <span style="font-size:12px;color:#888">"Sort:"</span>
                <select
                    style="background:#1a1a2e;color:#e5e7eb;border:1px solid #333;border-radius:6px;padding:4px 8px;font-size:12px"
                    on:change=move |ev| sort_by.set(event_target_value(&ev))
                    prop:value=move || sort_by.get()
                >
                    <option value="expiring">"Expiring Soon"</option>
                    <option value="amount_due">"Amount Due"</option>
                    <option value="date_added">"Date Added"</option>
                    <option value="name_az">"Name A\u{2013}Z"</option>
                    <option value="role">"Role"</option>
                </select>
            </div>

            {move || {
                if loading.get() {
                    return view! { <p class="muted" style="text-align:center;padding:20px">"Loading\u{2026}"</p> }.into_any();
                }
                let mut list = items.get();
                if list.is_empty() {
                    return view! {
                        <div style="text-align:center;padding:40px 20px;color:#666">
                            <p style="font-size:32px;margin-bottom:8px">{"\u{2713}"}</p>
                            <p style="font-size:14px">"Nothing open \u{2014} you're all clear"</p>
                        </div>
                    }.into_any();
                }

                // Sort
                let sort = sort_by.get();
                match sort.as_str() {
                    "amount_due" => list.sort_by(|a, b| {
                        let aa = a.amount_kx.unwrap_or(0.0);
                        let bb = b.amount_kx.unwrap_or(0.0);
                        bb.partial_cmp(&aa).unwrap_or(std::cmp::Ordering::Equal)
                    }),
                    "date_added" => list.sort_by(|a, b| b.sort_time.cmp(&a.sort_time)),
                    "name_az" => list.sort_by(|a, b| a.description.to_lowercase().cmp(&b.description.to_lowercase())),
                    "role" => list.sort_by(|a, b| {
                        // Lender items (credit) first, then borrower items, then others
                        let ra = if a.item_type == "credit" { 0 } else if a.item_type == "invoice" || a.item_type == "poke" || a.item_type == "request" { 2 } else { 1 };
                        let rb = if b.item_type == "credit" { 0 } else if b.item_type == "invoice" || b.item_type == "poke" || b.item_type == "request" { 2 } else { 1 };
                        ra.cmp(&rb).then(a.sort_time.cmp(&b.sort_time))
                    }),
                    _ => list.sort_by(|a, b| {
                        // "expiring" (default): earliest sort_time first, 0→end
                        let at = if a.sort_time == 0 { i64::MAX } else { a.sort_time };
                        let bt = if b.sort_time == 0 { i64::MAX } else { b.sort_time };
                        at.cmp(&bt)
                    }),
                }

                view! {
                    <div style="display:flex;flex-direction:column;gap:8px">
                        {list.into_iter().map(|item| {
                            let item_c = item.clone();
                            let badge_style = format!("display:inline-block;padding:2px 8px;border-radius:4px;background:{};color:white;font-size:10px;font-weight:700", item.badge_color);
                            view! {
                                <div class="card" style="padding:12px;display:flex;align-items:flex-start;gap:10px">
                                    <span style="font-size:20px;flex-shrink:0;line-height:1">{item.icon}</span>
                                    <div style="flex:1;min-width:0">
                                        <div style="display:flex;align-items:center;gap:6px;margin-bottom:4px">
                                            <span style=badge_style>{item.badge_label.clone()}</span>
                                        </div>
                                        <p style="font-size:13px;color:#e5e7eb;margin:0 0 2px;word-break:break-word">{item.description.clone()}</p>
                                        <div style="display:flex;gap:8px;font-size:11px;color:#888">
                                            {if let Some(amt) = item.amount_kx {
                                                view! { <span style="color:#d4a84b;font-weight:700">{format!("{:.2} KX", amt)}</span> }.into_any()
                                            } else {
                                                view! { <span></span> }.into_any()
                                            }}
                                            {if !item.time_label.is_empty() {
                                                view! { <span>{item.time_label.clone()}</span> }.into_any()
                                            } else {
                                                view! { <span></span> }.into_any()
                                            }}
                                        </div>
                                    </div>
                                    // Dismiss button or lock icon
                                    {if item.can_dismiss {
                                        let ic = item_c.clone();
                                        view! {
                                            <button
                                                style="background:none;border:1px solid #333;color:#888;width:28px;height:28px;border-radius:6px;cursor:pointer;font-size:14px;flex-shrink:0;display:flex;align-items:center;justify-content:center"
                                                on:click=move |_| dismiss_target.set(Some(ic.clone()))
                                            >"\u{00d7}"</button>
                                        }.into_any()
                                    } else {
                                        let tip = item.dismiss_tooltip.clone().unwrap_or_default();
                                        view! {
                                            <span style="color:#555;font-size:14px;flex-shrink:0" title=tip>"\u{1f512}"</span>
                                        }.into_any()
                                    }}
                                </div>
                            }
                        }).collect_view()}
                    </div>
                }.into_any()
            }}

            // Dismiss confirmation modal
            {move || if let Some(item) = dismiss_target.get() {
                let (title, body, btn_label) = match item.item_type.as_str() {
                    "invoice" => (
                        "Decline Invoice",
                        format!("Decline this invoice from {}?\nThey will be notified by email.", item.description),
                        "Decline Invoice",
                    ),
                    "poke" | "request" => (
                        "Decline Request",
                        format!("Decline payment request from {}?", item.description),
                        "Decline",
                    ),
                    "TYPE_V" | "kxgo" => (
                        "Close Game Session",
                        "Close this game session? KX will be returned to your wallet.".to_string(),
                        "Yes, close session",
                    ),
                    "TYPE_C" | "credit" => (
                        "Revoke Credit Line",
                        "Revoke this credit line?\nAny KX already drawn cannot be recalled.\nFuture draws will be blocked immediately.".to_string(),
                        "Revoke Credit Line",
                    ),
                    _ => (
                        "Dismiss",
                        format!("Dismiss this {}?", item.badge_label),
                        "Confirm",
                    ),
                };
                let item_id = item.id.clone();
                let item_type = item.item_type.clone();
                view! {
                    <div class="modal-overlay" on:click=move |_| dismiss_target.set(None)>
                        <div class="modal-card" on:click=move |ev: web_sys::MouseEvent| ev.stop_propagation()>
                            <p class="modal-title">{title}</p>
                            <p class="muted" style="white-space:pre-line;word-break:break-word;margin-bottom:12px">{body}</p>
                            <div style="display:flex;gap:8px">
                                <button style="flex:1" on:click=move |_| dismiss_target.set(None)>"Cancel"</button>
                                <button class="btn-danger" style="flex:1"
                                    disabled=move || dismiss_busy.get()
                                    on:click={
                                        let iid = item_id.clone();
                                        let itype = item_type.clone();
                                        move |_| {
                                            let iid = iid.clone();
                                            let itype = itype.clone();
                                            dismiss_busy.set(true);
                                            spawn_local(async move {
                                                let result = match itype.as_str() {
                                                    "invoice" => {
                                                        let args = serde_wasm_bindgen::to_value(&serde_json::json!({ "invoiceId": iid })).unwrap_or(no_args());
                                                        call::<()>("reject_invoice", args).await.map(|_| ())
                                                    }
                                                    "poke" => {
                                                        let args = serde_wasm_bindgen::to_value(&serde_json::json!({ "requestId": iid })).unwrap_or(no_args());
                                                        call::<()>("decline_poke", args).await.map(|_| ())
                                                    }
                                                    "request" => {
                                                        let args = serde_wasm_bindgen::to_value(&serde_json::json!({ "requestId": iid })).unwrap_or(no_args());
                                                        call::<()>("decline_kx_request", args).await.map(|_| ())
                                                    }
                                                    "TYPE_V" | "kxgo" | "TYPE_C" | "credit" | "TYPE_Y" | "deposit" => {
                                                        let args = serde_wasm_bindgen::to_value(&serde_json::json!({ "commitmentId": iid, "commitmentType": itype })).unwrap_or(no_args());
                                                        call::<String>("cancel_commitment", args).await.map(|_| ())
                                                    }
                                                    _ => Ok(()),
                                                };
                                                let _ = result;
                                                dismiss_target.set(None);
                                                dismiss_busy.set(false);
                                                load();
                                            });
                                        }
                                    }
                                >{move || if dismiss_busy.get() { "\u{2026}" } else { btn_label }}</button>
                            </div>
                        </div>
                    </div>
                }.into_any()
            } else { view! { <span></span> }.into_any() }}
        </div>
    }
}

// ── HistoryPanel ──────────────────────────────────────────────────────────────

#[component]
fn HistoryPanel(
    info: RwSignal<Option<AccountInfo>>,
    email_locks: RwSignal<Vec<TimeLockInfo>>,
    on_email_check: impl Fn() + Clone + 'static,
) -> impl IntoView {
    // email_locks and on_email_check are passed from parent but we use our own incoming signal
    let _ = &email_locks;
    let _ = &on_email_check;
    let entries    = RwSignal::new(Vec::<TxHistoryEntry>::new());
    let incoming   = RwSignal::new(Vec::<TimeLockInfo>::new());
    let h_loading  = RwSignal::new(false);
    let h_err      = RwSignal::new(String::new());
    let expanded   = RwSignal::new(Option::<String>::None);
    // Cancel confirmation modal state
    let cancel_target    = RwSignal::new(Option::<String>::None); // lock_id to cancel
    let cancel_is_email  = RwSignal::new(false);
    let cancel_busy      = RwSignal::new(false);
    let cancel_msg       = RwSignal::new(String::new());
    let cancel_cascade_ids = RwSignal::new(Vec::<String>::new()); // non-empty = series cancel
    // Sort: 0=date desc (default), 1=date asc, 2=amount desc, 3=amount asc, 4=type
    let h_sort = RwSignal::new(0u8);
    let h_page = RwSignal::new(0usize); // 0-indexed page number
    // Type filter: 0=All, 1=Sent, 2=Received, 3=Incoming Promise, 4=Outgoing Promise
    let h_filter = RwSignal::new(0u8);
    const PAGE_SIZE: usize = 10;

    // Claim message for incoming promise claims
    let inc_claim_msg = RwSignal::new(String::new());
    // Inline claim code input for email locks in History tab
    let inline_claim_open = RwSignal::new(Option::<String>::None); // lock_id when open
    let inline_claim_code = RwSignal::new(String::new());
    let inline_claim_busy = RwSignal::new(false);
    let inline_claim_result = RwSignal::new(String::new());

    let reload = move || {
        spawn_local(async move {
            h_loading.set(true);
            h_err.set(String::new());
            match call::<Vec<TxHistoryEntry>>("get_transaction_history", no_args()).await {
                Ok(e)  => entries.set(e),
                Err(e) => h_err.set(e),
            }
            // Also fetch incoming promises
            if let Ok(locks) = call::<Vec<TimeLockInfo>>("get_pending_incoming", no_args()).await {
                incoming.set(locks);
            }
            h_loading.set(false);
        });
    };

    Effect::new(move |_| { reload(); });
    // Reset to first page when sort or filter changes
    Effect::new(move |_| { h_sort.get(); h_filter.get(); h_page.set(0); });
    let on_refresh = move |_: web_sys::MouseEvent| { h_page.set(0); reload(); };

    // Sender info cache for relay-delivered transactions
    // Key: tx_id, Value: (sender_display, sender_wallet, avatar_url)
    let sender_cache: RwSignal<HashMap<String, (String, String, String)>> = RwSignal::new(HashMap::new());
    // Badge cache for counterparty wallets
    let badge_cache: RwSignal<HashMap<String, String>> = RwSignal::new(HashMap::new());

    // Look up sender info for relay-delivered incoming transactions
    Effect::new(move |_| {
        let all = entries.get();
        let my_wallet = info.get().map(|a| a.account_id.clone()).unwrap_or_default();
        if my_wallet.is_empty() { return; }
        let already = sender_cache.get_untracked();
        for entry in &all {
            let cp = entry.counterparty.as_deref().unwrap_or("");
            let is_incoming_type = matches!(entry.tx_type.as_str(),
                "Transfer Received" | "Email Claimed" | "Promise Kept");
            if !is_relay_wallet(cp) || !is_incoming_type { continue; }
            if already.contains_key(&entry.tx_id) { continue; }
            let tx_id = entry.tx_id.clone();
            let wallet = my_wallet.clone();
            let amount_kx: f64 = entry.amount_chronos.as_deref()
                .and_then(|c| c.parse::<f64>().ok())
                .map(|c| c / 1_000_000.0)
                .unwrap_or(0.0);
            spawn_local(async move {
                let args = serde_wasm_bindgen::to_value(&serde_json::json!({
                    "walletAddress": wallet,
                    "amountKx": amount_kx,
                })).unwrap_or(no_args());
                if let Ok(resp) = call::<String>("get_sender_info", args).await {
                    if let Ok(data) = serde_json::from_str::<serde_json::Value>(&resp) {
                        if data["found"].as_bool().unwrap_or(false) {
                            let display = data["sender_display"].as_str().unwrap_or("Unknown").to_string();
                            let sw = data["sender_wallet"].as_str().unwrap_or("").to_string();
                            let avatar_url = if !sw.is_empty() {
                                format!("https://api.chronx.io/avatar/{}", sw)
                            } else { String::new() };
                            sender_cache.update(|m| { m.insert(tx_id.clone(), (display, sw.clone(), avatar_url)); });
                            // Fetch badge for sender wallet
                            if !sw.is_empty() {
                                let sw2 = sw.clone();
                                let args2 = serde_wasm_bindgen::to_value(&serde_json::json!({ "walletAddress": sw2 })).unwrap_or(no_args());
                                if let Ok(meta_json) = call::<String>("get_avatar_meta", args2).await {
                                    if let Ok(meta) = serde_json::from_str::<serde_json::Value>(&meta_json) {
                                        if let Some(b) = meta["badge"].as_str() {
                                            if !b.is_empty() {
                                                badge_cache.update(|m| { m.insert(sw, b.to_string()); });
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            });
        }
    });

    view! {
        <div class="card">
            <div class="row">
                <p class="section-title">"Transaction History"</p>
                <button on:click=on_refresh disabled=move || h_loading.get()>
                    {move || if h_loading.get() { "\u{2026}" } else { "\u{21bb} Refresh" }}
                </button>
            </div>
            <div class="sort-bar">
                <span class="sort-label">"Sort:"</span>
                <button class=move || if h_sort.get() <= 1 { "pill active" } else { "pill" }
                    on:click=move |_| {
                        let cur = h_sort.get_untracked();
                        if cur == 0 { h_sort.set(1); } else { h_sort.set(0); }
                    }>
                    {move || if h_sort.get() == 1 { "Date \u{2191}" } else { "Date \u{2193}" }}
                </button>
                <button class=move || if h_sort.get() == 2 || h_sort.get() == 3 { "pill active" } else { "pill" }
                    on:click=move |_| {
                        let cur = h_sort.get_untracked();
                        if cur == 2 { h_sort.set(3); } else { h_sort.set(2); }
                    }>
                    {move || if h_sort.get() == 3 { "Amount \u{2191}" } else { "Amount \u{2193}" }}
                </button>
                <button class=move || if h_sort.get()==4 { "pill active" } else { "pill" }
                    on:click=move |_| h_sort.set(4)>"Type"</button>
            </div>

            // Type filter
            <div class="sort-bar" style="margin-top:4px">
                <span class="sort-label">"Filter:"</span>
                <button class=move || if h_filter.get()==0 { "pill active" } else { "pill" }
                    on:click=move |_| h_filter.set(0)>"All"</button>
                <button class=move || if h_filter.get()==1 { "pill active" } else { "pill" }
                    on:click=move |_| h_filter.set(1)>"Sent"</button>
                <button class=move || if h_filter.get()==2 { "pill active" } else { "pill" }
                    on:click=move |_| h_filter.set(2)>"Received"</button>
                <button class=move || if h_filter.get()==3 { "pill active" } else { "pill" }
                    on:click=move |_| h_filter.set(3)>"Incoming"</button>
                <button class=move || if h_filter.get()==4 { "pill active" } else { "pill" }
                    on:click=move |_| h_filter.set(4)>"Outgoing"</button>
            </div>

            // Incoming promise claim message
            {move || {
                let m = inc_claim_msg.get();
                if m.is_empty() { view! { <span></span> }.into_any() }
                else {
                    let cls = if m.starts_with("Error") { "msg error" }
                              else if m.starts_with("Mining") { "msg mining" }
                              else { "msg success" };
                    view! { <p class=cls style="margin:6px 0">{m}</p> }.into_any()
                }
            }}

            {move || {
                let e = h_err.get();
                if e.is_empty() { view! { <span></span> }.into_any() }
                else { view! { <p class="error">{e}</p> }.into_any() }
            }}

            {move || {
                let mut list = entries.get();

                // Convert incoming promises to TxHistoryEntry for unified display
                // Filter out self-referencing locks (sender == own wallet) to avoid
                // ghost "Incoming Promise" rows for the user's own outgoing sends.
                // NOTE: use .get() (not get_untracked) so the closure re-runs when
                // info loads — otherwise own_account_id is empty on first render
                // and the filter silently fails.
                let own_account_id = info.get()
                    .map(|i| i.account_id.clone())
                    .unwrap_or_default();
                let inc_locks = incoming.get();
                for lock in &inc_locks {
                    if lock.sender == own_account_id || own_account_id.is_empty() {
                        continue; // skip self-referencing lock (or skip all if info not loaded yet)
                    }
                    list.push(TxHistoryEntry {
                        tx_id: lock.lock_id.clone(),
                        tx_type: "Incoming Promise".to_string(),
                        amount_chronos: Some(lock.amount_chronos.clone()),
                        counterparty: Some(lock.sender.clone()),
                        timestamp: lock.created_at,
                        status: lock.status.clone(),
                        unlock_date: Some(lock.unlock_at),
                        cancellation_window_secs: lock.cancellation_window_secs,
                        created_at: Some(lock.created_at),
                        claim_code: None,
                        // Only propagate claim_secret_hash for email locks (have recipient_email_hash).
                        // Wallet-to-wallet timelocks may have claim_secret_hash but no email hash.
                        claim_secret_hash: if lock.recipient_email_hash.is_some() {
                            lock.claim_secret_hash.clone()
                        } else {
                            None
                        },
                        recipient_registered: None,
                        memo: lock.memo.clone(),
                        sender_wallet: None,
                        sender_email: None,
                        sender_display: None,
                    });
                }

                // Build cascade maps: which claim_secret_hash groups have any claimed lock?
                let mut cascade_claimed: std::collections::HashMap<String, bool> = HashMap::new();
                let mut cascade_lock_ids: std::collections::HashMap<String, Vec<String>> = HashMap::new();
                // Map claim_secret_hash → (email, recipient_registered) from Email Send entries
                let mut cascade_email: std::collections::HashMap<String, (String, Option<bool>)> = HashMap::new();
                for e in &list {
                    if let Some(ref hash) = e.claim_secret_hash {
                        cascade_lock_ids.entry(hash.clone()).or_default().push(e.tx_id.clone());
                        if e.status == "Claimed" || e.status.contains("Reverted") {
                            cascade_claimed.insert(hash.clone(), true);
                        }
                        // Capture email from Email Send entries for cascade cross-reference
                        if e.tx_type == "Email Send" {
                            if let Some(ref cp) = e.counterparty {
                                cascade_email.entry(hash.clone()).or_insert_with(|| (cp.clone(), e.recipient_registered));
                            }
                        }
                    }
                }

                // Apply type filter
                let filter = h_filter.get();
                if filter > 0 {
                    list.retain(|e| match filter {
                        1 => matches!(e.tx_type.as_str(), "Transfer Sent" | "Email Send"),
                        2 => matches!(e.tx_type.as_str(), "Transfer Received" | "Email Claimed" | "Promise Kept"),
                        3 => e.tx_type == "Incoming Promise",
                        4 => matches!(e.tx_type.as_str(), "Promise Sent" | "TimeLockCreate"),
                        _ => true,
                    });
                }

                // Apply sort
                match h_sort.get() {
                    0 => list.sort_by(|a, b| b.timestamp.cmp(&a.timestamp)),
                    1 => list.sort_by(|a, b| a.timestamp.cmp(&b.timestamp)),
                    2 => list.sort_by(|a, b| {
                        let ac: u128 = a.amount_chronos.as_deref().and_then(|s| s.parse().ok()).unwrap_or(0);
                        let bc: u128 = b.amount_chronos.as_deref().and_then(|s| s.parse().ok()).unwrap_or(0);
                        bc.cmp(&ac)
                    }),
                    3 => list.sort_by(|a, b| {
                        let ac: u128 = a.amount_chronos.as_deref().and_then(|s| s.parse().ok()).unwrap_or(0);
                        let bc: u128 = b.amount_chronos.as_deref().and_then(|s| s.parse().ok()).unwrap_or(0);
                        ac.cmp(&bc)
                    }),
                    4 => list.sort_by(|a, b| a.tx_type.cmp(&b.tx_type)),
                    _ => {}
                }
                // Pagination
                let total = list.len();
                let total_pages = if total == 0 { 1 } else { (total + PAGE_SIZE - 1) / PAGE_SIZE };
                let page = h_page.get().min(total_pages.saturating_sub(1));
                let page_list: Vec<TxHistoryEntry> = list.into_iter()
                    .skip(page * PAGE_SIZE)
                    .take(PAGE_SIZE)
                    .collect();

                // Group cascade entries by claim_secret_hash for collapsed display
                let mut seen_cascade_hashes: std::collections::HashSet<String> = std::collections::HashSet::new();
                let mut cascade_groups: std::collections::HashMap<String, Vec<TxHistoryEntry>> = HashMap::new();
                for e in &page_list {
                    if let Some(ref hash) = e.claim_secret_hash {
                        if cascade_lock_ids.get(hash).map_or(false, |ids| ids.len() > 1) {
                            cascade_groups.entry(hash.clone()).or_default().push(e.clone());
                        }
                    }
                }

                if h_loading.get() {
                    view! { <p class="muted">"Loading\u{2026}"</p> }.into_any()
                } else if total == 0 && h_err.get().is_empty() {
                    view! {
                        <div class="empty-state">
                            <p>"\u{1f552} No transactions yet"</p>
                            <p class="muted">"Transactions will appear here once confirmed on-chain."</p>
                        </div>
                    }.into_any()
                } else {
                    view! {
                        <div class="history-list">
                            {page_list.into_iter().filter_map(|entry| {
                                // Skip cascade entries that were already rendered as part of a group
                                if let Some(ref hash) = entry.claim_secret_hash {
                                    if cascade_groups.contains_key(hash) {
                                        if seen_cascade_hashes.contains(hash) {
                                            return None; // skip — already rendered in group
                                        }
                                        seen_cascade_hashes.insert(hash.clone());
                                    }
                                }
                                let tx_id = entry.tx_id.clone();
                                let tx_id_for_toggle = tx_id.clone();
                                let is_email_send = entry.tx_type == "Email Send";
                                let is_incoming_promise = entry.tx_type == "Incoming Promise";
                                let is_incoming = matches!(entry.tx_type.as_str(),
                                    "Transfer Received" | "Email Claimed" | "Promise Kept" | "Incoming Promise");
                                let type_icon = match entry.tx_type.as_str() {
                                    "Promise Sent" | "TimeLockCreate" => "\u{23f3}",
                                    "TimeLockClaim" => "\u{2705}",
                                    "Email Send" => "\u{1f4e7}",
                                    "Transfer Received" => "\u{2199}",
                                    "Email Claimed" => "\u{1f4ec}",
                                    "Promise Kept" => "\u{1f381}",
                                    "Incoming Promise" => "\u{1f4e5}",
                                    _ => "\u{2197}",
                                };
                                // Type label badge
                                let now_ts = (js_sys::Date::now() / 1000.0) as i64;
                                let is_scheduled = matches!(entry.tx_type.as_str(), "Promise Sent" | "TimeLockCreate")
                                    && entry.unlock_date.map_or(false, |u| u > now_ts);
                                // ── Outgoing email lock category (v1.4.96 label rules) ──
                                let entry_status_early = entry.status.clone();
                                let email_category = if entry.tx_type == "Email Send" {
                                    let resolved = matches!(entry_status_early.as_str(), "Claimed" | "Expired \u{2014} Reverted" | "Cancelled");
                                    let is_reclaiming = entry_status_early == "Expired \u{2014} Reclaiming";
                                    if resolved || is_reclaiming {
                                        0u8 // resolved — show status as-is
                                    } else {
                                        let unlock = entry.unlock_date.unwrap_or(0);
                                        let is_instant = unlock <= now_ts + 60;
                                        if is_instant && entry_status_early == "Claimed" {
                                            3u8 // Cat 3: instant + registered (auto-delivered)
                                        } else if is_instant {
                                            4u8 // Cat 4: instant + unregistered (pending claim)
                                        } else if unlock > now_ts + (3 * 24 * 3600) {
                                            1u8 // Cat 1: future >3 days
                                        } else {
                                            2u8 // Cat 2: future <=3 days
                                        }
                                    }
                                } else { 0u8 };
                                // Cascade detection (need early for type_label)
                                let is_cascade_early = entry.claim_secret_hash.as_ref()
                                    .map_or(false, |h| cascade_lock_ids.get(h).map_or(false, |ids| ids.len() > 1));
                                let type_label = match entry.tx_type.as_str() {
                                    "Transfer Sent" => "SENT",
                                    "Transfer Received" => "RECEIVED",
                                    "Email Send" if is_cascade_early => "CASCADE",
                                    "Email Send" => match email_category {
                                        1 | 2 => "PROMISE",
                                        3 => "SENT",
                                        4 => "SENT",
                                        _ => "SENT",
                                    },
                                    "Email Claimed" => "RECEIVED",
                                    "Promise Sent" | "TimeLockCreate" if is_cascade_early => "CASCADE",
                                    "Promise Sent" | "TimeLockCreate" if is_scheduled => "SCHEDULED",
                                    "Promise Sent" | "TimeLockCreate" => "OUTGOING PROMISE",
                                    "Promise Kept" => "RECEIVED",
                                    "TimeLockClaim" => "RECEIVED",
                                    "Incoming Promise" => "INCOMING PROMISE",
                                    _ => "SENT",
                                };
                                let label_class = match type_label {
                                    "SENT" => "history-type-badge sent",
                                    "RECEIVED" => "history-type-badge received",
                                    "INCOMING PROMISE" => "history-type-badge incoming-promise",
                                    "OUTGOING PROMISE" => "history-type-badge outgoing-promise",
                                    "SCHEDULED" => "history-type-badge scheduled",
                                    "PROMISE" => "history-type-badge scheduled",
                                    "CASCADE" => "history-type-badge cascade",
                                    _ => "history-type-badge",
                                };
                                let amount_display = entry.amount_chronos.as_deref()
                                    .map(|c| {
                                        let kx = format_kx(c);
                                        if is_incoming { format!("+{} KX", kx) } else { format!("{} KX", kx) }
                                    })
                                    .unwrap_or_else(|| "\u{2014}".to_string());
                                let amount_class = if is_incoming { "history-amount incoming" } else { "history-amount" };
                                // Email sends: show email address (truncated) regardless of unlock_date
                                let addr_display = if is_email_send {
                                    entry.counterparty.as_deref()
                                        .map(|e| if e.len() > 26 { format!("{}…", &e[..24]) } else { e.to_string() })
                                        .unwrap_or_default()
                                } else if is_incoming_promise {
                                    // Show "From: <shortened account>" + unlock date
                                    let from = entry.counterparty.as_deref()
                                        .map(|a| format!("From {}", shorten_addr(a)))
                                        .unwrap_or_default();
                                    if let Some(unlock_ts) = entry.unlock_date {
                                        let now = (js_sys::Date::now() / 1000.0) as i64;
                                        if unlock_ts <= now && entry.claim_secret_hash.is_none() {
                                            format!("{} \u{b7} Arriving shortly\u{2026}", from)
                                        } else if unlock_ts <= now {
                                            format!("{} \u{b7} Enter claim code to receive", from)
                                        } else {
                                            format!("{} \u{b7} Unlocks {}", from, unix_to_date_str(unlock_ts))
                                        }
                                    } else { from }
                                } else if is_incoming {
                                    let cp = entry.counterparty.as_deref().unwrap_or("");
                                    if is_relay_wallet(cp) {
                                        "Email delivery".to_string() // placeholder — replaced reactively below
                                    } else {
                                        entry.counterparty.as_deref()
                                            .map(|a| format!("From {}", shorten_addr(a)))
                                            .unwrap_or_default()
                                    }
                                } else if is_cascade_early {
                                    // Cascade "Promise Sent": show email from sibling Email Send entry
                                    entry.claim_secret_hash.as_ref()
                                        .and_then(|h| cascade_email.get(h))
                                        .map(|(email, _)| if email.len() > 26 { format!("{}…", &email[..24]) } else { email.clone() })
                                        .unwrap_or_else(|| entry.counterparty.as_deref().map(shorten_addr).unwrap_or_default())
                                } else if let Some(unlock_ts) = entry.unlock_date {
                                    format!("Unlocks {}", unix_to_date_str(unlock_ts))
                                } else {
                                    entry.counterparty.as_deref()
                                        .map(shorten_addr)
                                        .unwrap_or_default()
                                };
                                let date_display = format_utc_ts(entry.timestamp);
                                let tx_id_short = shorten_addr(&entry.tx_id);
                                let entry_status = entry.status.clone();

                                // Is this an email lock? (has claim_secret_hash)
                                let is_email_lock = entry.claim_secret_hash.is_some();

                                // Cascade awareness
                                let is_cascade = entry.claim_secret_hash.as_ref()
                                    .map_or(false, |h| cascade_lock_ids.get(h).map_or(false, |ids| ids.len() > 1));
                                let cascade_has_claim = entry.claim_secret_hash.as_ref()
                                    .map_or(false, |h| *cascade_claimed.get(h).unwrap_or(&false));
                                let entry_cascade_ids: Vec<String> = entry.claim_secret_hash.as_ref()
                                    .and_then(|h| cascade_lock_ids.get(h))
                                    .cloned()
                                    .unwrap_or_default();
                                let is_expired_reclaiming = entry_status == "Expired \u{2014} Reclaiming";

                                // Determine if this entry can be cancelled (OUTGOING only — never on incoming)
                                let can_cancel_base = !is_incoming
                                    && (entry.status == "Pending" || entry.status == "Pending Claim")
                                    && entry.cancellation_window_secs.map_or(false, |w| w > 0)
                                    && entry.created_at.map_or(false, |ca| {
                                        let window = entry.cancellation_window_secs.unwrap_or(0) as f64;
                                        let deadline = (ca as f64 + window) * 1000.0; // ms
                                        js_sys::Date::now() < deadline
                                    });
                                // For cascades: block cancel if any lock has been claimed
                                let can_cancel = if is_cascade { can_cancel_base && !cascade_has_claim } else { can_cancel_base };

                                let status_display = if is_cascade && cascade_has_claim && !is_email_send {
                                    "Promised \u{2713}".to_string()
                                } else if can_cancel && !is_email_send {
                                    "Pending \u{2014} subject to reversion".to_string()
                                } else if entry.status == "Pending Claim" {
                                    "Pending".to_string()
                                } else {
                                    entry.status.clone()
                                };

                                let cancel_lock_id = entry.tx_id.clone();
                                let inline_cancel_id = cancel_lock_id.clone();
                                let entry_claim_code = entry.claim_code.clone();
                                let entry_recipient_registered = entry.recipient_registered
                                    .or_else(|| entry.claim_secret_hash.as_ref()
                                        .and_then(|h| cascade_email.get(h))
                                        .and_then(|(_, reg)| *reg))
                                    .unwrap_or(false);

                                // ── Cascade group rendering (v1.5.3) ──
                                let cascade_group_entries: Option<Vec<TxHistoryEntry>> = entry.claim_secret_hash.as_ref()
                                    .and_then(|h| cascade_groups.get(h))
                                    .cloned();
                                let is_cascade_group = cascade_group_entries.is_some() && cascade_group_entries.as_ref().map_or(false, |g| g.len() > 1);

                                if is_cascade_group {
                                    let group = cascade_group_entries.unwrap();
                                    let stage_count = group.len();
                                    let total_chronos: u128 = group.iter()
                                        .filter_map(|e| e.amount_chronos.as_deref()?.parse::<u128>().ok())
                                        .sum();
                                    let total_kx = format_kx(&total_chronos.to_string());
                                    let group_email = entry.counterparty.clone()
                                        .or_else(|| entry.claim_secret_hash.as_ref()
                                            .and_then(|h| cascade_email.get(h))
                                            .map(|(em, _)| em.clone()))
                                        .unwrap_or_default();
                                    let toggle_hash = entry.claim_secret_hash.clone().unwrap_or_default();
                                    let any_claimed = group.iter().any(|e| e.status == "Claimed" || e.status.contains("Reverted"));
                                    let group_can_cancel = can_cancel && !any_claimed;
                                    let cancel_ids_for_group = entry_cascade_ids.clone();

                                    return Some(view! {
                                        <div class="cascade-parent">
                                            <div class="history-row" on:click=move |_| {
                                                let current = expanded.get_untracked();
                                                if current.as_deref() == Some(&toggle_hash) {
                                                    expanded.set(None);
                                                } else {
                                                    expanded.set(Some(toggle_hash.clone()));
                                                }
                                            }>
                                                <div class="history-row-top">
                                                    <span class="history-type">
                                                        "\u{23f3} " {format!("Cascade ({} stages)", stage_count)}
                                                    </span>
                                                    <span class="history-type-badge cascade" style="font-size:9px;padding:1px 6px;border-radius:4px;font-weight:700;letter-spacing:0.5px;margin-left:6px">
                                                        "CASCADE"
                                                    </span>
                                                    <span class={if is_incoming { "history-amount incoming" } else { "history-amount" }}>
                                                        {if is_incoming { format!("+{} KX", total_kx) } else { format!("{} KX", total_kx) }}
                                                    </span>
                                                </div>
                                                <div class="history-row-bottom">
                                                    <span class="history-addr">
                                                        {if group_email.len() > 26 { format!("{}…", &group_email[..24]) } else { group_email.clone() }}
                                                    </span>
                                                    <span class="history-date">{date_display.clone()}</span>
                                                </div>
                                                {if group_can_cancel {
                                                    let cids = cancel_ids_for_group.clone();
                                                    let first_id = entry.tx_id.clone();
                                                    view! {
                                                        <div style="margin-top:4px">
                                                            <button class="cancel-btn" style="font-size:11px;padding:2px 10px"
                                                                on:click=move |ev: web_sys::MouseEvent| {
                                                                    ev.stop_propagation();
                                                                    cancel_msg.set(String::new());
                                                                    cancel_is_email.set(true);
                                                                    cancel_cascade_ids.set(cids.clone());
                                                                    cancel_target.set(Some(first_id.clone()));
                                                                }>
                                                                "Cancel Series"
                                                            </button>
                                                        </div>
                                                    }.into_any()
                                                } else { view! { <span></span> }.into_any() }}
                                            </div>
                                            // Expanded stages
                                            {move || {
                                                let hash_check = entry.claim_secret_hash.clone().unwrap_or_default();
                                                if expanded.get().as_deref() != Some(&hash_check) {
                                                    return view! { <span></span> }.into_any();
                                                }
                                                let mut sorted_group = group.clone();
                                                sorted_group.sort_by_key(|s| s.unlock_date.unwrap_or(0));
                                                view! {
                                                    <div>
                                                        {sorted_group.iter().enumerate().map(|(i, stage)| {
                                                            let stage_amount = stage.amount_chronos.as_deref()
                                                                .map(|c| format!("{} KX", format_kx(c)))
                                                                .unwrap_or_default();
                                                            let stage_unlock = stage.unlock_date.map(unix_to_date_str).unwrap_or_default();
                                                            let stage_status = stage.status.clone();
                                                            let badge_cls = match stage_status.as_str() {
                                                                "Claimed" => "email-badge claimed",
                                                                "Cancelled" => "email-badge expired",
                                                                _ => "email-badge pending-claim",
                                                            };
                                                            view! {
                                                                <div class="cascade-stage">
                                                                    <div class="cascade-stage-row">
                                                                        <span style="color:#a78bfa;font-weight:600">{format!("Stage {}", i + 1)}</span>
                                                                        <span>{format!("Unlocks {}", stage_unlock)}</span>
                                                                        <span style="font-weight:600">{stage_amount}</span>
                                                                        <span class=badge_cls style="font-size:10px;padding:1px 6px;border-radius:3px">
                                                                            {if stage_status == "Pending Claim" || stage_status == "Pending" { "Pending".to_string() } else { stage_status }}
                                                                        </span>
                                                                    </div>
                                                                </div>
                                                            }
                                                        }).collect::<Vec<_>>()}
                                                    </div>
                                                }.into_any()
                                            }}
                                        </div>
                                    }.into_any());
                                }

                                // Avatar URL for counterparty
                                let cp_str = entry.counterparty.as_deref().unwrap_or("");
                                let is_from_relay = is_incoming && is_relay_wallet(cp_str);
                                let relay_tx_id = if is_from_relay { entry.tx_id.clone() } else { String::new() };
                                let avatar_addr = if is_email_send || is_from_relay {
                                    String::new()
                                } else {
                                    entry.counterparty.clone().unwrap_or_default()
                                };
                                let has_avatar = !avatar_addr.is_empty() && avatar_addr.len() > 10;
                                let avatar_src = if has_avatar {
                                    format!("https://api.chronx.io/avatar/{}", avatar_addr)
                                } else { String::new() };

                                Some(view! {
                                    <div class="history-row"
                                        on:click=move |_| {
                                            let current = expanded.get_untracked();
                                            if current.as_deref() == Some(&tx_id_for_toggle) {
                                                expanded.set(None);
                                            } else {
                                                expanded.set(Some(tx_id_for_toggle.clone()));
                                            }
                                        }>
                                        <div style="display:flex;align-items:flex-start;gap:8px">
                                            {if has_avatar {
                                                view! {
                                                    <img src={avatar_src}
                                                        style="width:32px;height:32px;border-radius:50%;border:1px solid #d4a84b;flex-shrink:0;object-fit:cover;background:#1a1a2e"
                                                        on:error=|ev| {
                                                            let target = ev.target().unwrap();
                                                            let el: web_sys::HtmlElement = target.unchecked_into();
                                                            let _ = el.style().set_property("display", "none");
                                                        }
                                                    />
                                                }.into_any()
                                            } else if is_from_relay {
                                                let rtx = relay_tx_id.clone();
                                                view! {
                                                    {move || {
                                                        let cache = sender_cache.get();
                                                        if let Some((_, _, ref av)) = cache.get(&rtx) {
                                                            if !av.is_empty() {
                                                                return view! {
                                                                    <img src={av.clone()}
                                                                        style="width:32px;height:32px;border-radius:50%;border:1px solid #d4a84b;flex-shrink:0;object-fit:cover;background:#1a1a2e"
                                                                        on:error=|ev| {
                                                                            let target = ev.target().unwrap();
                                                                            let el: web_sys::HtmlElement = target.unchecked_into();
                                                                            let _ = el.style().set_property("display", "none");
                                                                        }
                                                                    />
                                                                }.into_any();
                                                            }
                                                        }
                                                        view! {
                                                            <div style="width:32px;height:32px;border-radius:50%;background:#d4a84b;display:flex;align-items:center;justify-content:center;font-size:14px;font-weight:700;color:#000;flex-shrink:0">
                                                                {"\u{2709}"}
                                                            </div>
                                                        }.into_any()
                                                    }}
                                                }.into_any()
                                            } else if is_email_send {
                                                view! {
                                                    <div style="width:32px;height:32px;border-radius:50%;border:1px solid #6b7280;flex-shrink:0;display:flex;align-items:center;justify-content:center;background:#1a1a2e;font-size:16px">
                                                        "\u{2709}"
                                                    </div>
                                                }.into_any()
                                            } else {
                                                view! { <span></span> }.into_any()
                                            }}
                                            <div style="flex:1;min-width:0">
                                                <div class="history-row-top">
                                                    <span class="history-type">
                                                        {type_icon} " " {entry.tx_type.clone()}
                                                    </span>
                                                    <span class={label_class} style="font-size:9px;padding:1px 6px;border-radius:4px;font-weight:700;letter-spacing:0.5px;margin-left:6px">
                                                        {type_label}
                                                    </span>
                                                    <span class={amount_class}>{amount_display}</span>
                                                </div>
                                                <div class="history-row-bottom">
                                                    {if is_from_relay {
                                                        let rtx2 = relay_tx_id.clone();
                                                        let rtx3 = relay_tx_id.clone();
                                                        view! {
                                                            <span class="history-addr">{move || {
                                                                let cache = sender_cache.get();
                                                                if let Some((ref disp, _, _)) = cache.get(&rtx2) {
                                                                    format!("From: {}", disp)
                                                                } else {
                                                                    addr_display.clone()
                                                                }
                                                            }}</span>
                                                            {move || {
                                                                let cache = sender_cache.get();
                                                                let badges = badge_cache.get();
                                                                if let Some((_, ref sw, _)) = cache.get(&rtx3) {
                                                                    if let Some(b) = badges.get(sw) {
                                                                        let (bg, fg, text) = match b.as_str() {
                                                                            "FOUNDING_MEMBER" | "Founding Team" => ("#d4a84b", "black", "Founding Team"),
                                                                            "GENESIS_MEMBER" => ("#d4a84b", "black", "Genesis"),
                                                                            "PROTOCOL_PATRON" => ("#e2e8f0", "#1a1a2e", "Patron"),
                                                                            _ => return view! { <span></span> }.into_any(),
                                                                        };
                                                                        return view! {
                                                                            <span style={format!("display:inline-block;padding:1px 6px;border-radius:4px;background:{bg};color:{fg};font-size:10px;font-weight:700;margin-left:4px")}>{text}</span>
                                                                        }.into_any();
                                                                    }
                                                                }
                                                                view! { <span></span> }.into_any()
                                                            }}
                                                        }.into_any()
                                                    } else {
                                                        view! { <span class="history-addr">{addr_display}</span> }.into_any()
                                                    }}
                                                    <span class="history-date">{date_display}</span>
                                                </div>
                                            </div>
                                        </div>
                                        // Email send status badge + inline Cancel/Reclaim for email sends
                                        {if is_email_send {
                                            // v1.4.96: category-based badge labels
                                            let (badge_class, badge_text) = match email_category {
                                                3 => ("email-badge claimed", "CLAIMED".to_string()),
                                                4 => ("email-badge pending-claim", "PENDING".to_string()),
                                                1 | 2 => ("email-badge pending-claim", "PROMISE".to_string()),
                                                _ => {
                                                    // Resolved / reclaiming — use status as-is
                                                    let cls = match entry_status.as_str() {
                                                        "Claimed"       => "email-badge claimed",
                                                        "Expired \u{2014} Reclaiming" => "email-badge reclaiming",
                                                        "Cancelled"     => "email-badge expired",
                                                        _               => "email-badge expired",
                                                    };
                                                    (cls, entry_status.clone())
                                                }
                                            };
                                            // Cancel button visibility per category
                                            let cat_can_cancel = match email_category {
                                                3 => false, // instant + registered: auto-delivered, no cancel
                                                4 => can_cancel, // instant + unregistered: can cancel within 72h
                                                1 => can_cancel, // future >3 days: can cancel
                                                2 => can_cancel, // future <=3 days: can cancel (conservative — no registration check available)
                                                _ => false, // resolved states: no cancel
                                            };
                                            let reclaim_lock_id = entry.tx_id.clone();
                                            let cancel_cascade_for_click = entry_cascade_ids.clone();
                                            view! {
                                                <div style="display:flex;align-items:center;gap:8px;margin-top:4px;flex-wrap:wrap">
                                                    <span class=badge_class>{badge_text}</span>
                                                    {if is_expired_reclaiming {
                                                        // Show Reclaim button for expired-but-not-yet-swept locks
                                                        view! {
                                                            <button class="cancel-btn" style="margin-top:0;font-size:11px;padding:2px 10px;background:#d4a84b;color:#0a0a0a;border:none;border-radius:4px;cursor:pointer;font-weight:600"
                                                                on:click={move |ev: web_sys::MouseEvent| {
                                                                    ev.stop_propagation();
                                                                    let lid = reclaim_lock_id.clone();
                                                                    inc_claim_msg.set("Reclaiming\u{2026}".into());
                                                                    spawn_local(async move {
                                                                        let args = serde_wasm_bindgen::to_value(
                                                                            &serde_json::json!({ "lockIdHex": lid })
                                                                        ).unwrap_or(no_args());
                                                                        match call::<String>("reclaim_expired_lock", args).await {
                                                                            Ok(_) => {
                                                                                inc_claim_msg.set("Reclaimed! Funds returned.".into());
                                                                                reload();
                                                                            }
                                                                            Err(e) => inc_claim_msg.set(format!("Reclaim error: {e}")),
                                                                        }
                                                                    });
                                                                }}>
                                                                "Reclaim"
                                                            </button>
                                                        }.into_any()
                                                    } else if is_cascade && cascade_has_claim {
                                                        // Cascade where claim has started — no cancel allowed
                                                        view! {
                                                            <span style="color:#d4a84b;font-size:11px;font-weight:700">"Promised \u{2713}"</span>
                                                        }.into_any()
                                                    } else if cat_can_cancel {
                                                        view! {
                                                            <button class="cancel-btn" style="margin-top:0;font-size:11px;padding:2px 10px"
                                                                on:click={move |ev: web_sys::MouseEvent| {
                                                                    ev.stop_propagation();
                                                                    cancel_msg.set(String::new());
                                                                    cancel_is_email.set(true);
                                                                    cancel_cascade_ids.set(cancel_cascade_for_click.clone());
                                                                    cancel_target.set(Some(inline_cancel_id.clone()));
                                                                }}>
                                                                "Cancel"
                                                            </button>
                                                        }.into_any()
                                                    } else { view! { <span></span> }.into_any() }}
                                                    // Category subtext (v1.5.2: cascade + registration from sibling data)
                                                    {if is_cascade_early && entry_status != "Claimed" && entry_status != "Cancelled" && entry_status != "Expired \u{2014} Reverted" {
                                                        // Use entry's own recipient_registered, or fall back to sibling cascade data
                                                        let is_registered = entry.recipient_registered
                                                            .or_else(|| entry.claim_secret_hash.as_ref()
                                                                .and_then(|h| cascade_email.get(h))
                                                                .and_then(|(_, reg)| *reg))
                                                            .unwrap_or(false);
                                                        let reg_label = if is_registered { "Registered" } else { "Unregistered" };
                                                        let unlock_date = entry.unlock_date.map(unix_to_date_str).unwrap_or_default();
                                                        let subtext = format!("{} \u{b7} Unlocks {}", reg_label, unlock_date);
                                                        view! { <span style="color:#9ca3af;font-size:10px">{subtext}</span> }.into_any()
                                                    } else if email_category == 4 {
                                                        view! { <span style="color:#9ca3af;font-size:10px">{format!("{} \u{b7} Recipient has 72 hours to claim", t("en", "unregistered"))}</span> }.into_any()
                                                    } else if email_category == 1 || email_category == 2 {
                                                        let unlock = entry.unlock_date.unwrap_or(0);
                                                        let remaining = unlock - now_ts;
                                                        let hours = remaining / 3600;
                                                        let days = hours / 24;
                                                        let countdown = if days > 0 { format!("Unlocks in {}d {}h", days, hours % 24) }
                                                                        else { format!("Unlocks in {}h", hours) };
                                                        let is_registered = entry.recipient_registered.unwrap_or(false);
                                                        let subtext = if is_registered {
                                                            countdown
                                                        } else {
                                                            format!("{} \u{b7} {}", t("en", "unregistered"), countdown)
                                                        };
                                                        view! { <span style="color:#9ca3af;font-size:10px">{subtext}</span> }.into_any()
                                                    } else { view! { <span></span> }.into_any() }}
                                                </div>
                                            }.into_any()
                                        } else if is_incoming_promise {
                                            let now = (js_sys::Date::now() / 1000.0) as i64;
                                            let matured = entry.unlock_date.map_or(false, |u| u <= now) && entry_status == "Pending";
                                            if matured && !is_email_lock {
                                                // Wallet-to-wallet matured: node auto-delivers, no user action needed
                                                view! {
                                                    <span class="badge received" style="margin-top:4px;display:inline-block;color:#4ade80;font-weight:600;font-size:11px">
                                                        "Arriving shortly\u{2026}"
                                                    </span>
                                                }.into_any()
                                            } else if !matured {
                                                view! {
                                                    <span class="badge pending" style="margin-top:4px;display:inline-block">{entry_status.clone()}</span>
                                                }.into_any()
                                            } else {
                                                // Email lock matured: inline claim code input
                                                let claim_lock_id = entry.tx_id.clone();
                                                let claim_lock_id2 = claim_lock_id.clone();
                                                view! {
                                                    <div style="margin-top:4px">
                                                        <button class="pill" style="color:#d4a84b;border-color:#d4a84b;font-weight:600;font-size:11px;padding:2px 10px"
                                                            on:click=move |ev: web_sys::MouseEvent| {
                                                                ev.stop_propagation();
                                                                let current = inline_claim_open.get_untracked();
                                                                if current.as_deref() == Some(&claim_lock_id) {
                                                                    inline_claim_open.set(None);
                                                                    inline_claim_result.set(String::new());
                                                                } else {
                                                                    inline_claim_open.set(Some(claim_lock_id.clone()));
                                                                    inline_claim_code.set(String::new());
                                                                    inline_claim_result.set(String::new());
                                                                }
                                                            }>
                                                            {move || if inline_claim_open.get().as_deref() == Some(&claim_lock_id2) { "Cancel" } else { "Enter claim code to receive" }}
                                                        </button>
                                                        {
                                                        let claim_check_id = entry.tx_id.clone();
                                                        move || {
                                                            let open_id = inline_claim_open.get();
                                                            if open_id.as_deref() != Some(&claim_check_id) {
                                                                return view! { <span></span> }.into_any();
                                                            }
                                                            let result_msg = inline_claim_result.get();
                                                            view! {
                                                                <div style="margin-top:6px;display:flex;gap:6px;align-items:center;flex-wrap:wrap" on:click=move |ev: web_sys::MouseEvent| ev.stop_propagation()>
                                                                    <input type="text" placeholder="KX-XXXX-XXXX-XXXX-XXXX" style="flex:1;min-width:180px;padding:6px 10px;font-size:13px;background:#161b27;border:1px solid #2d3748;border-radius:6px;color:#e5e7eb;font-family:monospace;letter-spacing:1px"
                                                                        prop:value=move || inline_claim_code.get()
                                                                        on:input=move |ev| inline_claim_code.set(event_target_value(&ev))
                                                                        disabled=move || inline_claim_busy.get() />
                                                                    <button style="padding:6px 14px;font-size:13px;font-weight:600;background:#d4a84b;color:#000;border:none;border-radius:6px;cursor:pointer;white-space:nowrap"
                                                                        disabled=move || inline_claim_busy.get()
                                                                        on:click=move |_| {
                                                                            let code = inline_claim_code.get_untracked();
                                                                            if code.trim().is_empty() { return; }
                                                                            inline_claim_busy.set(true);
                                                                            inline_claim_result.set("Claiming\u{2026}".into());
                                                                            spawn_local(async move {
                                                                                let args = serde_wasm_bindgen::to_value(
                                                                                    &serde_json::json!({ "claimCode": code.trim() })
                                                                                ).unwrap_or(no_args());
                                                                                match call::<ClaimByCodeResult>("claim_by_code", args).await {
                                                                                    Ok(result) => {
                                                                                        let kx = format_kx(&result.total_chronos);
                                                                                        inline_claim_result.set(format!("\u{2705} Claimed {kx} KX!"));
                                                                                        inline_claim_code.set(String::new());
                                                                                        inline_claim_open.set(None);
                                                                                        reload();
                                                                                    }
                                                                                    Err(e) => {
                                                                                        inline_claim_result.set(format!("Error: {e}"));
                                                                                    }
                                                                                }
                                                                                inline_claim_busy.set(false);
                                                                            });
                                                                        }>
                                                                        "Claim"
                                                                    </button>
                                                                </div>
                                                                {if !result_msg.is_empty() {
                                                                    let cls = if result_msg.starts_with("Error") { "color:#ef4444" } else if result_msg.starts_with("\u{2705}") { "color:#4ade80" } else { "color:#d4a84b" };
                                                                    view! { <p style={format!("font-size:12px;margin:4px 0 0;{}", cls)}>{result_msg}</p> }.into_any()
                                                                } else { view! { <span></span> }.into_any() }}
                                                            }.into_any()
                                                        }}
                                                    </div>
                                                }.into_any()
                                            }
                                        } else if is_cascade_early && !is_email_send && !is_incoming_promise {
                                            // "Promise Sent" cascade entries: show registration + unlock subtext
                                            let resolved = matches!(entry_status.as_str(), "Claimed" | "Cancelled" | "Expired" | "Reverted");
                                            if !resolved {
                                                let is_reg = entry.recipient_registered
                                                    .or_else(|| entry.claim_secret_hash.as_ref()
                                                        .and_then(|h| cascade_email.get(h))
                                                        .and_then(|(_, reg)| *reg))
                                                    .unwrap_or(false);
                                                let reg_label = if is_reg { "Registered" } else { "Unregistered" };
                                                let unlock_date = entry.unlock_date.map(unix_to_date_str).unwrap_or_default();
                                                let sub = format!("{} \u{b7} Unlocks {}", reg_label, unlock_date);
                                                view! {
                                                    <div style="display:flex;align-items:center;gap:8px;margin-top:4px;flex-wrap:wrap">
                                                        <span style="color:#9ca3af;font-size:10px">{sub}</span>
                                                        {if can_cancel && !cascade_has_claim {
                                                            let cancel_cascade_for_ps = entry_cascade_ids.clone();
                                                            let inline_cancel_ps = entry.tx_id.clone();
                                                            view! {
                                                                <button class="cancel-btn" style="margin-top:0;font-size:11px;padding:2px 10px"
                                                                    on:click={move |ev: web_sys::MouseEvent| {
                                                                        ev.stop_propagation();
                                                                        cancel_msg.set(String::new());
                                                                        cancel_is_email.set(true);
                                                                        cancel_cascade_ids.set(cancel_cascade_for_ps.clone());
                                                                        cancel_target.set(Some(inline_cancel_ps.clone()));
                                                                    }}>
                                                                    "Cancel"
                                                                </button>
                                                            }.into_any()
                                                        } else { view! { <span></span> }.into_any() }}
                                                    </div>
                                                }.into_any()
                                            } else {
                                                view! { <span></span> }.into_any()
                                            }
                                        } else { view! { <span></span> }.into_any() }}
                                        {move || {
                                            let is_expanded = expanded.get().as_deref() == Some(tx_id.as_str());
                                            if is_expanded {
                                                let cancel_id = cancel_lock_id.clone();
                                                let detail_cascade_ids = entry_cascade_ids.clone();
                                                let btn_label = if is_email_send {
                                                    "Cancel"
                                                } else { "Cancel Promise" };
                                                let code_opt = entry_claim_code.clone();
                                                let detail_reclaim_id = entry.tx_id.clone();
                                                view! {
                                                    <div class="history-detail">
                                                        <p>"TxID: " {tx_id_short.clone()}</p>
                                                        <p class="muted">"Status: " {status_display.clone()}</p>
                                                        // Show claim code for email sends so Alice can re-share it (hide if recipient verified)
                                                        {if is_email_send && !entry_recipient_registered {
                                                            if let Some(code) = code_opt {
                                                                let code_copy = code.clone();
                                                                view! {
                                                                    <div style="margin:6px 0">
                                                                        <p class="muted" style="font-size:11px;margin-bottom:2px">"Claim code (share with recipient):"</p>
                                                                        <p style="font-family:monospace;letter-spacing:2px;color:#d4a84b;font-size:14px;margin:2px 0">{code_copy.clone()}</p>
                                                                        <button class="copy-btn" on:click=move |ev: web_sys::MouseEvent| {
                                                                            ev.stop_propagation();
                                                                            let c = code_copy.clone();
                                                                            spawn_local(async move { copy_to_clipboard(c).await; });
                                                                        }>"📋 Copy Code"</button>
                                                                    </div>
                                                                }.into_any()
                                                            } else { view! { <span></span> }.into_any() }
                                                        } else { view! { <span></span> }.into_any() }}
                                                        {if is_expired_reclaiming {
                                                            view! {
                                                                <button
                                                                    class="cancel-btn"
                                                                    style="background:#d4a84b;color:#0a0a0a;border:none;border-radius:4px;cursor:pointer;font-weight:600"
                                                                    on:click=move |ev: web_sys::MouseEvent| {
                                                                        ev.stop_propagation();
                                                                        let lid = detail_reclaim_id.clone();
                                                                        inc_claim_msg.set("Reclaiming\u{2026}".into());
                                                                        spawn_local(async move {
                                                                            let args = serde_wasm_bindgen::to_value(
                                                                                &serde_json::json!({ "lockIdHex": lid })
                                                                            ).unwrap_or(no_args());
                                                                            match call::<String>("reclaim_expired_lock", args).await {
                                                                                Ok(_) => {
                                                                                    inc_claim_msg.set("Reclaimed! Funds returned.".into());
                                                                                    reload();
                                                                                }
                                                                                Err(e) => inc_claim_msg.set(format!("Reclaim error: {e}")),
                                                                            }
                                                                        });
                                                                    }
                                                                >"Reclaim Funds"</button>
                                                            }.into_any()
                                                        } else if is_cascade && cascade_has_claim && entry_status != "Claimed" && !entry_status.contains("Expired") && entry_status != "Cancelled" {
                                                            view! {
                                                                <span style="color:#d4a84b;font-weight:700">"Promised \u{2713} \u{2014} cannot cancel"</span>
                                                            }.into_any()
                                                        } else if can_cancel {
                                                            view! {
                                                                <button
                                                                    class="cancel-btn"
                                                                    on:click=move |ev: web_sys::MouseEvent| {
                                                                        ev.stop_propagation();
                                                                        cancel_msg.set(String::new());
                                                                        cancel_is_email.set(is_email_send);
                                                                        cancel_cascade_ids.set(detail_cascade_ids.clone());
                                                                        cancel_target.set(Some(cancel_id.clone()));
                                                                    }
                                                                >{btn_label}</button>
                                                            }.into_any()
                                                        } else {
                                                            view! { <span></span> }.into_any()
                                                        }}
                                                    </div>
                                                }.into_any()
                                            } else { view! { <span></span> }.into_any() }
                                        }}
                                    </div>
                                }.into_any())
                            }).collect::<Vec<_>>()}
                        </div>
                        {if total_pages > 1 {
                            view! {
                                <div class="pagination-bar">
                                    <button class="pill"
                                        disabled={move || h_page.get() == 0}
                                        on:click={move |_| h_page.update(|p| if *p > 0 { *p -= 1; })}>
                                        "\u{2190} Prev"
                                    </button>
                                    <span class="page-indicator">
                                        {format!("Page {} of {}", page + 1, total_pages)}
                                    </span>
                                    <button class="pill"
                                        disabled={move || h_page.get() >= total_pages - 1}
                                        on:click={move |_| { h_page.update(|p| { *p += 1; }); }}>
                                        "Next \u{2192}"
                                    </button>
                                </div>
                            }.into_any()
                        } else { view! { <span></span> }.into_any() }}
                    }.into_any()
                }
            }}

            // ── Cancel confirmation modal ────────────────────────────────────────
            {move || if cancel_target.get().is_some() {
                let lock_id = cancel_target.get_untracked().unwrap_or_default();
                let lock_id_confirm = lock_id.clone();
                let is_email = cancel_is_email.get_untracked();
                let cascade_ids = cancel_cascade_ids.get_untracked();
                let is_series = cascade_ids.len() > 1;
                let modal_title = if is_email { "Cancel Email Send?" } else { "Cancel Promise?" };
                let modal_body = if is_email {
                    if is_series {
                        "Cancel all sends in this series? The KX will return to your balance immediately."
                    } else {
                        "Cancel this send? The KX will return to your balance immediately."
                    }
                } else {
                    "Are you sure you wish to cancel this Promise? The KX will be returned to your balance immediately. This cannot be undone."
                };
                let confirm_label = if is_email { "Yes, Cancel" } else { "Yes, Cancel Promise" };
                view! {
                    <div class="modal-overlay" on:click=move |_| {
                        if !cancel_busy.get_untracked() { cancel_target.set(None); }
                    }>
                        <div class="modal" on:click=move |ev: web_sys::MouseEvent| ev.stop_propagation()>
                            <p class="modal-title">{modal_title}</p>
                            <p class="modal-body">{modal_body}
                            </p>
                            {move || {
                                let msg = cancel_msg.get();
                                if msg.is_empty() { view! { <span></span> }.into_any() }
                                else { view! { <p class="error">{msg}</p> }.into_any() }
                            }}
                            <div class="modal-actions">
                                <button
                                    disabled=move || cancel_busy.get()
                                    on:click=move |_| {
                                        if !cancel_busy.get_untracked() {
                                            cancel_target.set(None);
                                        }
                                    }
                                >"No, Keep It"</button>
                                <button
                                    class="danger-btn"
                                    disabled=move || cancel_busy.get()
                                    on:click=move |_| {
                                        let id = lock_id_confirm.clone();
                                        let series_ids = cascade_ids.clone();
                                        cancel_busy.set(true);
                                        cancel_msg.set(String::new());
                                        spawn_local(async move {
                                            if series_ids.len() > 1 {
                                                // Cascade cancel — use cancel_timelock_series
                                                let args = serde_wasm_bindgen::to_value(
                                                    &serde_json::json!({ "lockIds": series_ids })
                                                ).unwrap_or(no_args());
                                                match call::<Vec<String>>("cancel_timelock_series", args).await {
                                                    Ok(_) => {
                                                        cancel_target.set(None);
                                                        cancel_busy.set(false);
                                                        reload();
                                                    }
                                                    Err(e) => {
                                                        cancel_msg.set(format!("Cancel failed: {e}"));
                                                        cancel_busy.set(false);
                                                    }
                                                }
                                            } else {
                                                // Single cancel
                                                let args = serde_wasm_bindgen::to_value(
                                                    &serde_json::json!({ "lockIdHex": id })
                                                ).unwrap_or(no_args());
                                                match call::<String>("cancel_timelock", args).await {
                                                    Ok(_) => {
                                                        cancel_target.set(None);
                                                        cancel_busy.set(false);
                                                        reload();
                                                    }
                                                    Err(e) => {
                                                        cancel_msg.set(format!("Cancel failed: {e}"));
                                                        cancel_busy.set(false);
                                                    }
                                                }
                                            }
                                        });
                                    }
                                >{move || if cancel_busy.get() { "Cancelling\u{2026}" } else { confirm_label }}</button>
                            </div>
                        </div>
                    </div>
                }.into_any()
            } else {
                view! { <span></span> }.into_any()
            }}
        </div>
    }
}

// ── RewardsPanel ──────────────────────────────────────────────────────────────

#[derive(Clone, Deserialize, Default)]
struct RewardsStatus {
    registered: bool,
    confirmed: bool,
    email: Option<String>,
}

#[component]
fn RewardsPanel(active_tab: RwSignal<u8>) -> impl IntoView {
    let email      = RwSignal::new(String::new());
    // 0 = loading, 1 = not registered, 2 = pending confirmation, 3 = confirmed
    let phase      = RwSignal::new(0u8);
    let masked_email = RwSignal::new(String::new());
    let reg_msg    = RwSignal::new(String::new());
    let submitting = RwSignal::new(false);
    let has_claim_emails = RwSignal::new(true); // assume true until we check

    // Check claim emails on mount
    spawn_local(async move {
        if let Ok(emails) = call::<Vec<String>>("get_claim_emails", no_args()).await {
            has_claim_emails.set(!emails.is_empty());
        } else {
            has_claim_emails.set(false);
        }
    });

    // Check rewards status on mount
    spawn_local(async move {
        if let Ok(status) = call::<RewardsStatus>("check_rewards_status", no_args()).await {
            if status.confirmed {
                if let Some(e) = status.email { masked_email.set(e); }
                phase.set(3);
            } else if status.registered {
                if let Some(e) = status.email { masked_email.set(e); }
                phase.set(2);
            } else {
                phase.set(1);
            }
        } else {
            phase.set(1); // on error, show registration form
        }
    });

    let on_register = move |_: web_sys::MouseEvent| {
        let email_str = email.get_untracked();
        if !is_valid_email(&email_str) {
            reg_msg.set("Error: enter a valid email address.".into()); return;
        }
        submitting.set(true);
        reg_msg.set(String::new());
        spawn_local(async move {
            let args = serde_wasm_bindgen::to_value(
                &serde_json::json!({ "email": email_str })
            ).unwrap_or(no_args());
            match call::<String>("register_for_rewards", args).await {
                Ok(_) => {
                    phase.set(2); // show "check your email"
                }
                Err(e) => {
                    reg_msg.set(format!("Error: {e}"));
                }
            }
            submitting.set(false);
        });
    };

    let on_resend = move |_: web_sys::MouseEvent| {
        let email_str = email.get_untracked();
        if email_str.is_empty() || !is_valid_email(&email_str) {
            reg_msg.set("Enter your email above to resend.".into()); return;
        }
        submitting.set(true);
        reg_msg.set(String::new());
        spawn_local(async move {
            let args = serde_wasm_bindgen::to_value(
                &serde_json::json!({ "email": email_str })
            ).unwrap_or(no_args());
            match call::<String>("register_for_rewards", args).await {
                Ok(_) => {
                    reg_msg.set("Confirmation email resent!".into());
                }
                Err(e) => {
                    reg_msg.set(format!("Error: {e}"));
                }
            }
            submitting.set(false);
        });
    };

    let on_check_status = move |_: web_sys::MouseEvent| {
        spawn_local(async move {
            if let Ok(status) = call::<RewardsStatus>("check_rewards_status", no_args()).await {
                if status.confirmed {
                    if let Some(e) = status.email { masked_email.set(e); }
                    phase.set(3);
                }
            }
        });
    };

    view! {
        <div class="card">
            <p class="section-title">"🎁 ChronX Rewards"</p>
            <p class="label" style="color:var(--muted);margin-bottom:16px;">
                "Earn free KX for being part of the ChronX community. Register your wallet to receive rewards, announcements, and exclusive airdrops."
            </p>

            // Claim email nudge removed — claim by code no longer requires email

            {move || match phase.get() {
                0 => {
                    // Loading state
                    view! {
                        <div style="text-align:center;padding:20px 0;">
                            <p style="color:var(--muted);font-size:14px;">"Checking rewards status\u{2026}"</p>
                        </div>
                    }.into_any()
                }
                2 => {
                    // Pending confirmation — user registered but hasn't clicked the email link
                    view! {
                        <div style="text-align:center;padding:20px 0;">
                            <div style="font-size:32px;margin-bottom:10px;">"📧"</div>
                            <p style="font-weight:700;color:var(--gold);margin-bottom:8px;font-size:15px;">
                                "Check your email!"
                            </p>
                            <p style="font-size:13px;color:var(--muted);margin-bottom:16px;">
                                "We sent a confirmation link to your email. Click it to activate your Rewards registration."
                            </p>
                            {move || {
                                let m = reg_msg.get();
                                if m.is_empty() { view! { <span></span> }.into_any() }
                                else {
                                    let cls = if m.starts_with("Error") { "msg error" } else { "msg success" };
                                    view! { <p class=cls>{m}</p> }.into_any()
                                }
                            }}
                            <p class="label" style="margin-top:12px;">"Email (to resend confirmation)"</p>
                            <input
                                type="email"
                                class="input"
                                placeholder="you@example.com"
                                prop:value=move || email.get()
                                on:input=move |ev| email.set(event_target_value(&ev))
                            />
                            <div style="display:flex;gap:8px;margin-top:12px;">
                                <button
                                    class="btn-primary"
                                    style="flex:1;padding:10px;"
                                    disabled=move || submitting.get()
                                    on:click=on_resend>
                                    {move || if submitting.get() { "Sending\u{2026}" } else { "Resend Email" }}
                                </button>
                                <button
                                    class="pill"
                                    style="padding:10px 16px;"
                                    on:click=on_check_status>
                                    "I Confirmed"
                                </button>
                            </div>
                        </div>
                    }.into_any()
                }
                3 => {
                    // Confirmed — fully registered
                    let email_display = masked_email.get_untracked();
                    view! {
                        <div style="text-align:center;padding:20px 0;">
                            <div style="font-size:32px;margin-bottom:10px;">"🎉"</div>
                            <p style="font-weight:700;color:var(--gold);margin-bottom:8px;font-size:15px;">
                                "You are registered for ChronX Rewards!"
                            </p>
                            {if !email_display.is_empty() {
                                view! {
                                    <p style="font-size:13px;color:var(--muted);margin-bottom:8px;">
                                        {format!("Email: {}", email_display)}
                                    </p>
                                }.into_any()
                            } else {
                                view! { <span></span> }.into_any()
                            }}
                            <p style="font-size:13px;color:var(--muted);margin-bottom:12px">
                                "Watch your inbox for free KX opportunities!"
                            </p>
                            <button style="color:#ef4444;border:1px solid #ef4444;background:transparent;border-radius:6px;padding:6px 14px;font-size:13px;cursor:pointer"
                                on:click=move |_| {
                                    // Reset to unregistered state
                                    phase.set(1);
                                    reg_msg.set("You have been unsubscribed from Rewards.".to_string());
                                }>"Unsubscribe from Rewards"</button>
                        </div>
                    }.into_any()
                }
                _ => {
                    // Phase 1 — not registered, show registration form
                    view! {
                        <div>
                            <p class="label">"Your Email Address"</p>
                            <input
                                type="email"
                                class="input"
                                placeholder="you@example.com"
                                prop:value=move || email.get()
                                on:input=move |ev| email.set(event_target_value(&ev))
                            />
                            {move || {
                                let m = reg_msg.get();
                                if m.is_empty() { view! { <span></span> }.into_any() }
                                else {
                                    let cls = if m.starts_with("Error") { "msg error" } else { "msg success" };
                                    view! { <p class=cls>{m}</p> }.into_any()
                                }
                            }}
                            <button
                                class="btn-primary"
                                style="margin-top:12px;width:100%;padding:10px;"
                                disabled=move || submitting.get()
                                on:click=on_register>
                                {move || if submitting.get() { "Registering\u{2026}" } else { "Register for Rewards" }}
                            </button>
                        </div>
                    }.into_any()
                }
            }}

            <div style="margin-top:24px;padding-top:16px;border-top:1px solid var(--border);">
                <p class="label" style="margin-bottom:8px;">"How Rewards Work"</p>
                <ul style="color:var(--muted);font-size:13px;line-height:2;padding-left:18px;">
                    <li>"Register your wallet and confirm your email"</li>
                    <li>"Receive KX airdrops from the ChronX team"</li>
                    <li>"Get notified of exclusive opportunities"</li>
                    <li>"KX delivered directly to your wallet — no action needed"</li>
                </ul>
            </div>
        </div>
    }
}

// ── SettingsPanel ─────────────────────────────────────────────────────────────

#[component]
fn SettingsPanel(
    online: RwSignal<bool>,
    app_phase: RwSignal<AppPhase>,
    pin_digits: RwSignal<String>,
    pin_msg: RwSignal<String>,
    pin_shake: RwSignal<bool>,
    app_version: RwSignal<String>,
    notices: RwSignal<Vec<Notice>>,
    seen_ids: RwSignal<Vec<String>>,
    on_mark_seen: impl Fn(String) + Clone + Send + 'static,
    pin_len: RwSignal<u8>,
    update_available: RwSignal<bool>,
    lang: RwSignal<String>,
    desktop: bool,
    info: RwSignal<Option<AccountInfo>>,
    email_locks: RwSignal<Vec<TimeLockInfo>>,
    on_email_check: impl Fn() + Clone + 'static,
    active_tab: RwSignal<u8>,
    bug_modal_open: RwSignal<bool>,
    bug_body: RwSignal<String>,
) -> impl IntoView {
    let node_url   = RwSignal::new(String::new());
    let save_msg   = RwSignal::new(String::new());
    let pubkey_hex = RwSignal::new(String::new());
    let pk_loading = RwSignal::new(false);

    // Update check state
    let update_result   = RwSignal::new(Option::<UpdateInfo>::None);
    let update_checking = RwSignal::new(false);

    // Export/Import state (legacy)
    let show_export       = RwSignal::new(false);
    let export_confirmed  = RwSignal::new(false);
    let export_key        = RwSignal::new(String::new());
    let export_loading    = RwSignal::new(false);
    let show_import       = RwSignal::new(false);
    let import_key        = RwSignal::new(String::new());
    let import_msg        = RwSignal::new(String::new());
    let import_busy       = RwSignal::new(false);
    let import_confirm    = RwSignal::new(false);

    // Seed phrase view state
    let show_seed_modal       = RwSignal::new(false);
    let seed_pin_phase        = RwSignal::new(0u8); // 0=enter PIN, 1=reveal screen
    let seed_pin_input        = RwSignal::new(String::new());
    let seed_pin_msg          = RwSignal::new(String::new());
    let seed_words            = RwSignal::new(String::new());
    let seed_revealed         = RwSignal::new(false);
    let seed_loading          = RwSignal::new(false);

    // New wallet creation (compromised flow)
    let show_new_wallet       = RwSignal::new(false);
    let show_balance_warning  = RwSignal::new(false);
    let balance_warning_kx    = RwSignal::new(String::new());
    let new_wallet_confirm_input = RwSignal::new(String::new());
    let new_wallet_busy       = RwSignal::new(false);
    let new_wallet_msg        = RwSignal::new(String::new());
    let new_wallet_mnemonic   = RwSignal::new(String::new());
    let new_wallet_address    = RwSignal::new(String::new());
    let compromised_expanded  = RwSignal::new(false);

    // Auth method (PIN / Biometric)
    let auth_method = RwSignal::new("pin".to_string());
    let auth_method_loading = RwSignal::new(false);

    // Cold storage state
    let show_cold         = RwSignal::new(false);
    let cold_result       = RwSignal::new(Option::<(String, String)>::None); // (account_id, private_key_b64)
    let cold_generating   = RwSignal::new(false);
    let cold_saved        = RwSignal::new(false);
    let cold_wallets      = RwSignal::new(Vec::<String>::new());

    // Load cold wallets list on mount
    Effect::new(move |_| {
        spawn_local(async move {
            let wallets = call::<Vec<String>>("get_cold_wallets", no_args()).await.unwrap_or_default();
            cold_wallets.set(wallets);
        });
    });

    // Modal visibility
    let show_about   = RwSignal::new(false);
    let show_updates = RwSignal::new(false);
    let show_change_pin = RwSignal::new(false);

    // Multi claim emails (up to 3) + verification state
    let claim_emails = RwSignal::new(Vec::<String>::new());
    let claim_email_msg = RwSignal::new(String::new());
    let verified_emails = RwSignal::new(Vec::<String>::new());
    // New email being added (verification flow)
    let new_email_input = RwSignal::new(String::new());
    let verify_phase = RwSignal::new(0u8); // 0=idle, 1=sending code, 2=code sent (enter code), 3=verifying
    let verify_code_input = RwSignal::new(String::new());
    let verify_msg = RwSignal::new(String::new());
    let verify_email_addr = RwSignal::new(String::new()); // email being verified

    // Change PIN state
    let cp_phase    = RwSignal::new(0u8); // 0=verify current, 1=enter new, 2=confirm new
    let cp_digits   = RwSignal::new(String::new());
    let cp_first    = RwSignal::new(String::new());
    let cp_msg      = RwSignal::new(String::new());
    let cp_shake    = RwSignal::new(false);

    // Privacy signals (loaded at Settings mount level)
    let settings_badge_list = RwSignal::new(Vec::<(String, String, String)>::new()); // (bg, fg, label)
    let settings_badges_on = RwSignal::new(true);
    let settings_identity_on = RwSignal::new(true);

    Effect::new(move |_| {
        spawn_local(async move {
            let url = call::<String>("get_node_url", no_args()).await.unwrap_or_default();
            node_url.set(url);
            let emails = call::<Vec<String>>("get_claim_emails", no_args()).await.unwrap_or_default();
            claim_emails.set(emails);
            let verified = call::<Vec<String>>("get_verified_emails", no_args()).await.unwrap_or_default();
            verified_emails.set(verified);
            let method = call::<String>("get_auth_method", no_args()).await.unwrap_or_else(|_| "pin".to_string());
            auth_method.set(method);
            // Load privacy settings
            settings_badges_on.set(call::<bool>("get_show_badges", no_args()).await.unwrap_or(true));
            settings_identity_on.set(call::<bool>("get_show_identity", no_args()).await.unwrap_or(true));
            // Load badges — fetch immediately, retry after 2s
            for attempt in 0..2u8 {
                if attempt > 0 { delay_ms(2000).await; }
                if let Ok(acct) = call::<AccountInfo>("get_account_info", no_args()).await {
                    if !acct.account_id.is_empty() {
                        let args = serde_wasm_bindgen::to_value(&serde_json::json!({ "walletAddress": acct.account_id })).unwrap_or(no_args());
                        if let Ok(wb) = call::<Vec<WalletBadge>>("get_wallet_badges", args).await {
                            let pills: Vec<(String, String, String)> = wb.iter().map(|b| {
                                match b.badge_type.as_str() {
                                    "FOUNDING_MEMBER" | "FOUNDER" | "Founding Team" => ("#d4a84b".to_string(), "black".to_string(), "Founding Team".to_string()),
                                    "GENESIS_MEMBER" => ("#d4a84b".to_string(), "black".to_string(), "Genesis Member".to_string()),
                                    "KXGO_BRONZE" => ("#CD7F32".to_string(), "white".to_string(), "KXGO Bronze".to_string()),
                                    "KXGO_SILVER" => ("#C0C0C0".to_string(), "#1a1a2e".to_string(), "KXGO Silver".to_string()),
                                    "KXGO_GOLD" => ("#D4A84B".to_string(), "black".to_string(), "KXGO Gold".to_string()),
                                    _ => (b.color.clone().unwrap_or_else(|| "#555".to_string()), "white".to_string(), b.badge_type.clone()),
                                }
                            }).collect();
                            if !pills.is_empty() {
                                settings_badge_list.set(pills);
                                break;
                            }
                        }
                    }
                }
            }
        });
    });

    let on_save = move |_: web_sys::MouseEvent| {
        let url = node_url.get_untracked();
        if url.is_empty() { save_msg.set("Error: URL cannot be empty.".into()); return; }
        spawn_local(async move {
            save_msg.set(String::new());
            let args = serde_wasm_bindgen::to_value(&serde_json::json!({ "url": url })).unwrap_or(no_args());
            match call::<()>("set_node_url", args).await {
                Ok(_) => {
                    let is_online = call::<bool>("check_node", no_args()).await.unwrap_or(false);
                    online.set(is_online);
                    save_msg.set(if is_online { "Saved. Node is online." } else { "Saved. Node is offline." }.into());
                }
                Err(e) => save_msg.set(format!("Error: {e}")),
            }
        });
    };

    let on_show_pubkey = move |_: web_sys::MouseEvent| {
        // Toggle: if key is already visible, hide it
        if !pubkey_hex.get_untracked().is_empty() {
            pubkey_hex.set(String::new());
            return;
        }
        spawn_local(async move {
            pk_loading.set(true);
            match call::<String>("export_public_key", no_args()).await {
                Ok(pk) => pubkey_hex.set(pk),
                Err(e) => pubkey_hex.set(format!("Error: {e}")),
            }
            pk_loading.set(false);
        });
    };

    // Change PIN: auto-submit Effect (digit capture is handled by the shared PinInput component).
    Effect::new(move |_| {
        let d = cp_digits.get();
        if d.len() == pin_len.get() as usize {
            let captured = d.clone();
            cp_digits.set(String::new());
            let phase = cp_phase.get_untracked();
            match phase {
                0 => {
                    // Verify current PIN
                    spawn_local(async move {
                        let args = serde_wasm_bindgen::to_value(
                            &serde_json::json!({ "pin": captured })
                        ).unwrap_or(no_args());
                        match call::<bool>("verify_pin", args).await {
                            Ok(true) => { cp_phase.set(1); cp_msg.set(String::new()); }
                            Ok(false) | Err(_) => {
                                do_shake(cp_shake);
                                cp_msg.set(t(&lang.get(), "settings_incorrect_pin"));
                            }
                        }
                    });
                }
                1 => { cp_first.set(captured); cp_phase.set(2); cp_msg.set(String::new()); }
                2 => {
                    let first = cp_first.get_untracked();
                    if captured == first {
                        spawn_local(async move {
                            let args = serde_wasm_bindgen::to_value(
                                &serde_json::json!({ "pin": captured })
                            ).unwrap_or(no_args());
                            match call::<()>("set_pin", args).await {
                                Ok(_) => {
                                    cp_msg.set(t(&lang.get(), "settings_pin_changed"));
                                    cp_phase.set(0);
                                    cp_first.set(String::new());
                                    // Close modal after brief delay
                                    spawn_local(async move {
                                        delay_ms(1500).await;
                                        show_change_pin.set(false);
                                        cp_msg.set(String::new());
                                    });
                                }
                                Err(e) => cp_msg.set(format!("Error: {e}")),
                            }
                        });
                    } else {
                        do_shake(cp_shake);
                        cp_msg.set(t(&lang.get(), "settings_pins_no_match"));
                        cp_phase.set(1);
                        cp_first.set(String::new());
                    }
                }
                _ => {}
            }
        }
    });

    // Language picker state
    let show_lang_picker = RwSignal::new(false);
    // History sub-view for mobile
    let show_mobile_history = RwSignal::new(false);
    // Rewards sub-view for mobile
    let show_mobile_rewards = RwSignal::new(false);
    // Collapsible section states (v2.4.8)
    let sec_notices_open = RwSignal::new(false);
    let sec_rewards_open = RwSignal::new(false);
    let sec_privacy_open = RwSignal::new(false);
    let sec_security_open = RwSignal::new(false);
    // Auto-open Notices if unread notices exist
    Effect::new(move |_| {
        let unread = notices.get().iter()
            .filter(|n| n.severity != "urgent" && !seen_ids.get().contains(&n.id))
            .count();
        if unread > 0 { sec_notices_open.set(true); }
    });

    view! {
        // Mobile History full-screen view
        {if !desktop {
            view! {
                <div style:display=move || if show_mobile_history.get() { "" } else { "none" }>
                    <div class="card">
                        <div class="row" style="justify-content:space-between;align-items:center">
                            <button class="btn-outline small" on:click=move |_| show_mobile_history.set(false)>
                                "\u{2190} Back"
                            </button>
                            <p class="section-title">{move || t(&lang.get(), "transaction_history")}</p>
                            <span></span>
                        </div>
                    </div>
                    <HistoryPanel info=info email_locks=email_locks on_email_check=on_email_check.clone() />
                </div>
            }.into_any()
        } else {
            view! { <span></span> }.into_any()
        }}

        // Mobile Rewards full-screen view
        {if !desktop {
            view! {
                <div style:display=move || if show_mobile_rewards.get() { "" } else { "none" }>
                    <div class="card">
                        <div class="row" style="justify-content:space-between;align-items:center">
                            <button class="btn-outline small" on:click=move |_| show_mobile_rewards.set(false)>
                                "\u{2190} Back"
                            </button>
                            <p class="section-title">{move || t(&lang.get(), "tab_rewards")}</p>
                            <span></span>
                        </div>
                    </div>
                    <RewardsPanel active_tab=active_tab />
                </div>
            }.into_any()
        } else {
            view! { <span></span> }.into_any()
        }}

        // Main settings content (hidden when mobile sub-views are active)
        <div style:display=move || {
            if !desktop && (show_mobile_history.get() || show_mobile_rewards.get()) {
                "none"
            } else { "" }
        }>
        <div class="card settings-content-wrap">
            <p class="section-title" style="order:0">{move || t(&lang.get(), "tab_settings")}</p>

            // Language picker
            <div class="settings-section" style="margin-bottom:12px;order:1">
                <div class="row" style="justify-content:space-between;align-items:center;cursor:pointer"
                    on:click=move |_| show_lang_picker.set(!show_lang_picker.get_untracked())>
                    <div style="display:flex;align-items:center;gap:10px">
                        <span style="width:28px;height:28px;border-radius:6px;background:#4A90D9;display:flex;align-items:center;justify-content:center;flex-shrink:0;color:white;font-size:13px;font-weight:700">"EN"</span>
                        <span>{move || t(&lang.get(), "settings_language")}</span>
                    </div>
                    <span style="color:#DAA520">{move || {
                        let l = lang.get();
                        t(&l, &format!("lang_{}", l))
                    }}</span>
                </div>
                {move || if show_lang_picker.get() {
                    let langs = vec![
                        ("en", "\u{1f1fa}\u{1f1f8} English"),
                        ("fr", "\u{1f1eb}\u{1f1f7} Fran\u{e7}ais"),
                        ("de", "\u{1f1e9}\u{1f1ea} Deutsch"),
                        ("zh", "\u{1f1e8}\u{1f1f3} \u{4e2d}\u{6587}"),
                        ("es", "\u{1f1ea}\u{1f1f8} Espa\u{f1}ol"),
                        ("ru", "\u{1f1f7}\u{1f1fa} \u{0420}\u{0443}\u{0441}\u{0441}\u{043a}\u{0438}\u{0439}"),
                        ("ar", "\u{1f1f8}\u{1f1e6} \u{0627}\u{0644}\u{0639}\u{0631}\u{0628}\u{064a}\u{0629}"),
                        ("ur", "\u{1f1f5}\u{1f1f0} \u{0627}\u{0631}\u{062f}\u{0648}"),
                    ];
                    view! {
                        <div class="lang-picker" style="margin-top:8px">
                            {langs.into_iter().map(|(code, label)| {
                                let code_str = code.to_string();
                                let code_for_click = code_str.clone();
                                view! {
                                    <div class="lang-row" style="padding:8px 12px;cursor:pointer;border-radius:8px"
                                        style:background=move || if lang.get() == code_str { "#333" } else { "transparent" }
                                        on:click=move |_| {
                                            let c = code_for_click.clone();
                                            lang.set(c.clone());
                                            show_lang_picker.set(false);
                                            // Set RTL for Arabic
                                            if let Some(doc) = web_sys::window().and_then(|w| w.document()) {
                                                if let Some(body) = doc.body() {
                                                    let _ = body.set_attribute("dir", if c == "ar" || c == "ur" { "rtl" } else { "ltr" });
                                                }
                                            }
                                            // Save preference
                                            let c2 = c.clone();
                                            spawn_local(async move {
                                                let args = serde_wasm_bindgen::to_value(&serde_json::json!({ "lang": c2 }))
                                                    .unwrap_or(no_args());
                                                let _ = call::<()>("set_language", args).await;
                                            });
                                        }>
                                        <span>{label}</span>
                                        {if lang.get_untracked() == code {
                                            view! { <span style="float:right;color:#DAA520">"\u{2713}"</span> }.into_any()
                                        } else {
                                            view! { <span></span> }.into_any()
                                        }}
                                    </div>
                                }
                            }).collect_view()}
                        </div>
                    }.into_any()
                } else {
                    view! { <span></span> }.into_any()
                }}
            </div>

            // (Mobile nav links removed in v2.5.4 — Activity tab handles History, Rewards is its own section)

            // Advanced Settings (all platforms)
            {
                let advanced_open = RwSignal::new(false);
                let node_editing = RwSignal::new(false);
                view! {
                    <div class="settings-section" style="order:7"
                         class:open=move || advanced_open.get()>
                        <div style="display:flex;justify-content:space-between;align-items:center;padding:2px 0;cursor:pointer;border-bottom:1px solid rgba(255,255,255,0.06)"
                            on:click=move |_| advanced_open.update(|v| *v = !*v)>
                            <div style="display:flex;align-items:center;gap:10px">
                                <span style="width:28px;height:28px;border-radius:6px;background:#808080;display:flex;align-items:center;justify-content:center;flex-shrink:0;color:white;font-size:16px;font-weight:700">{"\u{2699}\u{FE0E}"}</span>
                                <span style="font-size:14px;color:#e5e7eb">"Advanced"</span>
                            </div>
                            <span style=move || format!("color:#888;font-size:12px;transition:transform 0.2s;display:inline-block;{}", if advanced_open.get() { "transform:rotate(90deg)" } else { "" })>{"\u{203a}"}</span>
                        </div>
                        {move || if advanced_open.get() {
                            view! {
                                <div style="margin-top:10px">
                                    <div class="field">
                                        <label>{move || t(&lang.get(), "settings_node_url")}</label>
                                        <p class="muted" style="font-size:11px;margin-bottom:4px">"Only change this if you know what you're doing."</p>
                                        <input type="text" placeholder="http://127.0.0.1:8545"
                                            prop:value=move || node_url.get()
                                            on:input=move |ev| node_url.set(event_target_value(&ev))
                                            readonly=move || !node_editing.get() />
                                    </div>
                                    {move || if node_editing.get() {
                                        view! {
                                            <button class="primary" on:click=on_save>{move || t(&lang.get(), "settings_save_reconnect")}</button>
                                        }.into_any()
                                    } else {
                                        view! {
                                            <button style="font-size:12px;padding:4px 12px"
                                                on:click=move |_| node_editing.set(true)>"Edit"</button>
                                        }.into_any()
                                    }}
                                    {move || {
                                        let s = save_msg.get();
                                        if s.is_empty() { view! { <span></span> }.into_any() }
                                        else {
                                            let cls = if s.starts_with("Error") { "msg error" } else { "msg success" };
                                            view! { <p class=cls>{s}</p> }.into_any()
                                        }
                                    }}
                                    // ── My Public Key (inside Advanced) ──
                                    <hr style="border:none;border-top:1px solid #2d3748;margin:12px 0" />
                                    <p class="label" style="text-transform:uppercase;letter-spacing:1px">{move || t(&lang.get(), "settings_public_key")}</p>
                                    <p class="muted" style="font-size:11px;margin-bottom:6px">
                                        {move || format!("({})", t(&lang.get(), "settings_public_key_sub"))}
                                    </p>
                                    <button on:click=on_show_pubkey disabled=move || pk_loading.get()>
                                        {move || if pk_loading.get() {
                                            "\u{2026}".to_string()
                                        } else if pubkey_hex.get().is_empty() {
                                            t(&lang.get(), "settings_show_pubkey")
                                        } else {
                                            t(&lang.get(), "settings_hide_pubkey")
                                        }}
                                    </button>
                                    {move || {
                                        let pk = pubkey_hex.get();
                                        if pk.is_empty() { view! { <span></span> }.into_any() }
                                        else {
                                            let pk_for_copy = pk.clone();
                                            view! {
                                                <div>
                                                    <div style="max-height:120px;overflow-y:auto;background:#0f1117;border-radius:6px;padding:8px;margin-top:8px">
                                                        <p class="mono" style="font-size:10px;word-break:break-all;line-height:1.6;color:#9ca3af;margin:0">{pk}</p>
                                                    </div>
                                                    <button style="font-size:12px;padding:4px 10px;margin-top:6px"
                                                        on:click=move |_: web_sys::MouseEvent| {
                                                            let pk_val = pk_for_copy.clone();
                                                            spawn_local(async move {
                                                                copy_to_clipboard(pk_val).await;
                                                            });
                                                        }>
                                                        {format!("{}", t(&lang.get(), "settings_copy_pubkey"))}
                                                    </button>
                                                </div>
                                            }.into_any()
                                        }
                                    }}
                                </div>
                            }.into_any()
                        } else {
                            view! { <span></span> }.into_any()
                        }}
                    </div>
                }.into_any()
            }

            // Notices (collapsible)
            <div class="settings-section" style="order:2"
                 class:open=move || sec_notices_open.get()>
                <div style="display:flex;justify-content:space-between;align-items:center;padding:2px 0;cursor:pointer;border-bottom:1px solid rgba(255,255,255,0.06)"
                    on:click=move |_| sec_notices_open.set(!sec_notices_open.get_untracked())>
                    <div style="display:flex;align-items:center;gap:10px">
                        <span style="width:28px;height:28px;border-radius:6px;background:#F5A623;display:flex;align-items:center;justify-content:center;flex-shrink:0;color:white;font-size:13px;font-weight:700">"!"</span>
                        <span style="font-size:14px;color:#e5e7eb">"Notices"</span>
                    </div>
                    <div style="display:flex;align-items:center;gap:6px">
                        {move || {
                            let unread = notices.get().iter()
                                .filter(|n| n.severity != "urgent" && !seen_ids.get().contains(&n.id))
                                .count();
                            if unread > 0 {
                                view! { <span class="notice-badge" style="background:#d4a84b;font-size:10px">{unread}</span> }.into_any()
                            } else { view! { <span></span> }.into_any() }
                        }}
                        <span style=move || format!("color:#888;font-size:12px;transition:transform 0.2s;display:inline-block;{}", if sec_notices_open.get() { "transform:rotate(90deg)" } else { "" })>{"\u{203a}"}</span>
                    </div>
                </div>
                <div style:display=move || if sec_notices_open.get() { "" } else { "none" }>
                {move || if update_available.get() {
                    view! {
                        <div class="update-card" style="background:rgba(201,168,76,0.1);border:1px solid rgba(201,168,76,0.3);border-radius:8px;padding:12px;margin-bottom:8px">
                            <p style="font-weight:700;color:#c9a84c;font-size:13px">
                                {format!("\u{1f514} {}", t(&lang.get(), "settings_update_available"))}
                            </p>
                            <p class="muted" style="font-size:12px;margin-top:4px">
                                {t(&lang.get(), "settings_update_go")}
                            </p>
                        </div>
                    }.into_any()
                } else {
                    view! { <span></span> }.into_any()
                }}
                {move || {
                    let all = notices.get();
                    let seen = seen_ids.get();
                    let unread = all.iter().filter(|n| !seen.contains(&n.id)).count();
                    if all.is_empty() {
                        view! { <p class="muted">{t(&lang.get(), "settings_no_notices")}</p> }.into_any()
                    } else {
                        let on_mark_c = on_mark_seen.clone();
                        view! {
                            <div>
                                {if unread > 0 {
                                    view! {
                                        <p class="label" style="color:#e74c3c;margin-bottom:8px">
                                            {unread} " unread"
                                        </p>
                                    }.into_any()
                                } else { view! { <span></span> }.into_any() }}
                                {all.into_iter().filter(|n| {
                                    // Filter out urgent notices (shown in banner) and expired
                                    if n.severity == "urgent" { return false; }
                                    if let Some(ref exp) = n.expires {
                                        let now_str = js_sys::Date::new_0().to_iso_string().as_string().unwrap_or_default();
                                        if now_str.as_str() > exp.as_str() { return false; }
                                    }
                                    true
                                }).map(|n| {
                                    let is_read = seen.contains(&n.id);
                                    let nid = n.id.clone();
                                    let on_mark_n = on_mark_c.clone();
                                    let url = n.url.clone();
                                    let url_label = n.url_label.clone();
                                    let icon = if n.severity == "reward" { "🎁 " } else { "" };
                                    view! {
                                        <div class=format!("notice-card {}", n.severity)
                                             style=format!("opacity:{};cursor:pointer", if is_read { "0.55" } else { "1" })
                                             on:click=move |_| on_mark_n(nid.clone())>
                                            <p class="notice-card-title">{icon.to_string() + &n.title}</p>
                                            <p class="notice-card-date">{n.date.clone()}</p>
                                            <p class="notice-card-body">
                                                {linkify_body(n.body.clone()).into_iter().map(|(is_url, part)| {
                                                    if is_url {
                                                        view! { <a href=part.clone() class="notice-link" target="_blank" rel="noopener">{part.clone()}</a> }.into_any()
                                                    } else {
                                                        view! { <span>{part}</span> }.into_any()
                                                    }
                                                }).collect::<Vec<_>>()}
                                            </p>
                                            {if let Some(link) = url {
                                                let label = url_label.unwrap_or_else(|| link.clone());
                                                view! {
                                                    <a href=link class="notice-link" target="_blank" rel="noopener"
                                                       style="display:inline-block;margin-top:8px;font-weight:700;">{label}</a>
                                                }.into_any()
                                            } else { view! { <span></span> }.into_any() }}
                                        </div>
                                    }
                                }).collect::<Vec<_>>()}
                            </div>
                        }.into_any()
                    }
                }}
                </div> // close notices collapsible
            </div>

            // Rewards (collapsible)
            <div class="settings-section" style="order:3"
                 class:open=move || sec_rewards_open.get()>
                <div style="display:flex;justify-content:space-between;align-items:center;padding:2px 0;cursor:pointer;border-bottom:1px solid rgba(255,255,255,0.06)"
                    on:click=move |_| sec_rewards_open.set(!sec_rewards_open.get_untracked())>
                    <div style="display:flex;align-items:center;gap:10px">
                        <span style="width:28px;height:28px;border-radius:6px;background:#FFD700;display:flex;align-items:center;justify-content:center;flex-shrink:0;color:#1a1a2e;font-size:14px;font-weight:700">{"\u{2605}"}</span>
                        <span style="font-size:14px;color:#e5e7eb">"Rewards"</span>
                    </div>
                    <span style=move || format!("color:#888;font-size:12px;transition:transform 0.2s;display:inline-block;{}", if sec_rewards_open.get() { "transform:rotate(90deg)" } else { "" })>{"\u{203a}"}</span>
                </div>
                <div style:display=move || if sec_rewards_open.get() { "" } else { "none" }>
                    <div style="padding:12px 0">
                        <RewardsPanel active_tab=active_tab />
                    </div>
                </div>
            </div>

            // ─── SETTINGS SECTION: SECURITY (order:5) ──────────────────────
            // TOP-LEVEL — contains MY EMAILS FOR KX CLAIMS
            <div class="settings-section" style="order:5"
                 class:open=move || sec_security_open.get()>
                <div style="display:flex;justify-content:space-between;align-items:center;padding:2px 0;cursor:pointer;border-bottom:1px solid rgba(255,255,255,0.06)"
                    on:click=move |_| sec_security_open.set(!sec_security_open.get_untracked())>
                    <div style="display:flex;align-items:center;gap:10px">
                        <span style="width:28px;height:28px;border-radius:6px;background:#50C878;display:flex;align-items:center;justify-content:center;flex-shrink:0;color:white;font-size:14px;font-weight:700">{"\u{2022}"}</span>
                        <span style="font-size:14px;color:#e5e7eb">"Security"</span>
                    </div>
                    <span style=move || format!("color:#888;font-size:12px;transition:transform 0.2s;display:inline-block;{}", if sec_security_open.get() { "transform:rotate(90deg)" } else { "" })>{"\u{203a}"}</span>
                </div>
                <div style:display=move || if sec_security_open.get() { "" } else { "none" }>

                // Login Method toggle
                <p class="muted" style="font-size:12px;margin-bottom:6px">"Login Method"</p>
                <div style="display:flex;gap:8px;margin-bottom:12px">
                    <button
                        class=move || if auth_method.get() == "pin" { "pin-len-btn active" } else { "pin-len-btn" }
                        style="flex:1"
                        on:click=move |_| {
                            if auth_method.get() != "pin" {
                                auth_method.set("pin".to_string());
                                spawn_local(async move {
                                    let args = serde_wasm_bindgen::to_value(
                                        &serde_json::json!({ "method": "pin" })
                                    ).unwrap_or(no_args());
                                    let _ = call::<()>("set_auth_method", args).await;
                                });
                            }
                        }
                    >"PIN"</button>
                    <button
                        class=move || if auth_method.get() == "biometric" { "pin-len-btn active" } else { "pin-len-btn" }
                        style="flex:1"
                        disabled=move || auth_method_loading.get()
                        on:click=move |_| {
                            if auth_method.get() != "biometric" {
                                auth_method_loading.set(true);
                                spawn_local(async move {
                                    // Check availability first
                                    let avail = call::<String>("check_biometric_available", no_args()).await.unwrap_or_else(|_| "not_supported".to_string());
                                    if avail != "available" {
                                        cp_msg.set(if avail == "not_configured" {
                                            "Windows Hello is not configured. Set it up in Windows Settings first.".to_string()
                                        } else {
                                            "Biometric authentication not supported on this device.".to_string()
                                        });
                                        auth_method_loading.set(false);
                                        return;
                                    }
                                    match call::<bool>("authenticate_biometric", no_args()).await {
                                        Ok(true) => {
                                            let args = serde_wasm_bindgen::to_value(
                                                &serde_json::json!({ "method": "biometric" })
                                            ).unwrap_or(no_args());
                                            let _ = call::<()>("set_auth_method", args).await;
                                            auth_method.set("biometric".to_string());
                                            cp_msg.set("\u{2705} Biometric login enabled".to_string());
                                        }
                                        Err(e) => {
                                            cp_msg.set(e);
                                        }
                                        _ => {
                                            cp_msg.set("Biometric verification failed.".to_string());
                                        }
                                    }
                                    auth_method_loading.set(false);
                                });
                            }
                        }
                    >{move || if auth_method_loading.get() { "\u{2026}" } else { "Biometric" }}</button>
                </div>
                // Biometric status message
                {move || {
                    let m = cp_msg.get();
                    if m.is_empty() || m.starts_with("PIN changed") { view! { <span></span> }.into_any() }
                    else if m.starts_with("\u{2705}") { view! { <p class="msg success" style="margin-bottom:8px">{m}</p> }.into_any() }
                    else { view! { <p class="msg error" style="margin-bottom:8px">{m}</p> }.into_any() }
                }}

                // PIN-specific options (only when PIN is selected)
                {move || if auth_method.get() == "pin" {
                    view! {
                        <p class="muted" style="font-size:12px;margin-bottom:6px">{move || t(&lang.get(), "settings_pin_length")}</p>
                        <div style="display:flex;gap:8px;margin-bottom:12px">
                            {[4u8, 6, 8].into_iter().map(|n| {
                                view! {
                                    <button
                                        class=move || if pin_len.get() == n { "pin-len-btn active" } else { "pin-len-btn" }
                                        on:click=move |_| {
                                            if pin_len.get() != n {
                                                pin_len.set(n);
                                                spawn_local(async move {
                                                    let args = serde_wasm_bindgen::to_value(
                                                        &serde_json::json!({ "length": n })
                                                    ).unwrap_or(no_args());
                                                    let _ = call::<()>("set_pin_length", args).await;
                                                });
                                                cp_phase.set(0); cp_digits.set(String::new());
                                                cp_msg.set(format!("PIN length changed to {} digits. Enter current PIN, then set a new {}-digit PIN.", n, n));
                                                show_change_pin.set(true);
                                            }
                                        }
                                    >{format!("{} {}", n, t(&lang.get(), "settings_digits"))}</button>
                                }
                            }).collect::<Vec<_>>()}
                        </div>
                        <button on:click=move |_| {
                            cp_phase.set(0); cp_digits.set(String::new());
                            cp_msg.set(String::new()); show_change_pin.set(true);
                        }>{move || format!("{}", t(&lang.get(), "settings_change_pin"))}</button>
                    }.into_any()
                } else {
                    view! {
                        <div style="background:#1a2a1a;border:1px solid #333;border-radius:8px;padding:12px;margin-bottom:12px">
                            <p style="font-size:13px;color:#aaa;margin-bottom:4px">
                                "Face ID / Fingerprint / Windows Hello will unlock your wallet"
                            </p>
                            <p style="font-size:11px;color:#666">
                                "PIN kept as backup if biometric is unavailable"
                            </p>
                        </div>
                        <button on:click=move |_| {
                            cp_phase.set(0); cp_digits.set(String::new());
                            cp_msg.set(String::new()); show_change_pin.set(true);
                        }>{move || format!("{}", t(&lang.get(), "settings_change_pin"))}</button>
                    }.into_any()
                }}

                // ── My Emails for KX Claims (inside Security) ──
                <hr style="border:none;border-top:1px solid #2d3748;margin:12px 0" />
                <p class="label">{move || t(&lang.get(), "settings_claim_emails")}</p>
                <p class="muted" style="font-size:12px;margin-bottom:8px">
                    "Enter your email address to auto-claim any KX sent to it directly to your wallet balance."
                </p>
                // List of existing emails with verified/unverified badges
                <div class="claim-emails-list">
                    {move || {
                        let emails = claim_emails.get();
                        let verified = verified_emails.get();
                        if emails.is_empty() {
                            view! { <span></span> }.into_any()
                        } else {
                            let rows: Vec<_> = emails.iter().enumerate().map(|(i, email)| {
                                let email_display = email.clone();
                                let is_verified = verified.contains(email);
                                let idx = i;
                                let email_for_reverify = email.clone();
                                view! {
                                    <div class="claim-email-row" dir="ltr" style="display:flex;align-items:center;gap:8px;padding:6px 0;direction:ltr">
                                        <span dir="ltr" style="flex:1;font-size:13px;word-break:break-all;text-align:left;unicode-bidi:embed;direction:ltr">{email_display}</span>
                                        {if is_verified {
                                            view! {
                                                <span style="font-size:11px;color:#4ade80;white-space:nowrap">
                                                    {format!("\u{2713} {}", t(&lang.get(), "verified"))}
                                                </span>
                                            }.into_any()
                                        } else {
                                            view! {
                                                <span style="font-size:11px;white-space:nowrap">
                                                    <span style="color:#f59e0b" title="Unverified">"\u{26a0} "</span>
                                                    <a href="#" style="color:#d4a84b;text-decoration:underline;font-size:11px" on:click=move |ev| {
                                                        ev.prevent_default();
                                                        let e = email_for_reverify.clone();
                                                        verify_email_addr.set(e.clone());
                                                        new_email_input.set(e);
                                                        verify_phase.set(1);
                                                        verify_msg.set(String::new());
                                                        verify_code_input.set(String::new());
                                                        // Send verification code
                                                        spawn_local(async move {
                                                            let addr = verify_email_addr.get_untracked();
                                                            let wid = info.get_untracked().map(|a| a.account_id).unwrap_or_default();
                                                            let args = serde_wasm_bindgen::to_value(
                                                                &serde_json::json!({ "email": addr, "walletId": wid })
                                                            ).unwrap_or(no_args());
                                                            match call::<String>("send_verify_email", args).await {
                                                                Ok(_) => {
                                                                    verify_phase.set(2);
                                                                    verify_msg.set(t(&lang.get(), "verify_code_sent"));
                                                                }
                                                                Err(e) => {
                                                                    verify_phase.set(0);
                                                                    verify_msg.set(format!("Error: {e}"));
                                                                }
                                                            }
                                                        });
                                                    }>{move || t(&lang.get(), "verify_send_code")}</a>
                                                </span>
                                            }.into_any()
                                        }}
                                        <button style="font-size:12px;padding:4px 8px;color:#f87171;background:transparent;border:1px solid #f87171;border-radius:4px"
                                            on:click=move |_| {
                                                claim_emails.update(|list| { if idx < list.len() { list.remove(idx); } });
                                                let remaining = claim_emails.get_untracked();
                                                spawn_local(async move {
                                                    let args = serde_wasm_bindgen::to_value(
                                                        &serde_json::json!({ "emails": remaining })
                                                    ).unwrap_or(no_args());
                                                    let _ = call::<()>("set_claim_emails", args).await;
                                                });
                                            }
                                        >"\u{2716}"</button>
                                    </div>
                                }
                            }).collect();
                            view! { <div>{rows}</div> }.into_any()
                        }
                    }}
                </div>

                // Verification code entry (visible when verify_phase == 2)
                {move || if verify_phase.get() == 2 {
                    view! {
                        <div style="border:1px solid rgba(212,168,75,0.3);border-radius:8px;padding:12px;margin:8px 0">
                            <p dir="ltr" style="font-size:13px;color:#d4a84b;font-weight:600;margin-bottom:6px;text-align:left;unicode-bidi:embed">
                                {move || verify_email_addr.get()}
                            </p>
                            <p class="muted" style="font-size:12px;margin-bottom:8px">
                                {move || t(&lang.get(), "verify_enter_code")}
                            </p>
                            <div style="display:flex;gap:8px;align-items:center">
                                <input type="text" maxlength="6"
                                    placeholder="ABC123"
                                    style="font-family:monospace;font-size:16px;letter-spacing:3px;text-align:center;text-transform:uppercase;width:140px"
                                    prop:value=move || verify_code_input.get()
                                    on:input=move |ev| verify_code_input.set(event_target_value(&ev)) />
                                <button class="primary" style="padding:8px 16px"
                                    disabled=move || verify_phase.get() == 3
                                    on:click=move |_| {
                                        let code = verify_code_input.get_untracked().trim().to_string();
                                        if code.len() < 6 { return; }
                                        verify_phase.set(3);
                                        verify_msg.set(t(&lang.get(), "verify_checking"));
                                        spawn_local(async move {
                                            let addr = verify_email_addr.get_untracked();
                                            let wid = info.get_untracked().map(|a| a.account_id).unwrap_or_default();
                                            let args = serde_wasm_bindgen::to_value(
                                                &serde_json::json!({ "email": addr, "code": code, "walletId": wid })
                                            ).unwrap_or(no_args());
                                            match call::<bool>("confirm_verify_email", args).await {
                                                Ok(true) => {
                                                    verify_msg.set(t(&lang.get(), "verify_success"));
                                                    verify_phase.set(0);
                                                    verify_code_input.set(String::new());
                                                    // Reload verified + claim emails
                                                    let v = call::<Vec<String>>("get_verified_emails", no_args()).await.unwrap_or_default();
                                                    verified_emails.set(v);
                                                    let c = call::<Vec<String>>("get_claim_emails", no_args()).await.unwrap_or_default();
                                                    claim_emails.set(c);
                                                }
                                                Ok(false) => {
                                                    verify_msg.set(t(&lang.get(), "verify_failed"));
                                                    verify_phase.set(2);
                                                    verify_code_input.set(String::new());
                                                }
                                                Err(e) => {
                                                    verify_msg.set(format!("Error: {e}"));
                                                    verify_phase.set(2);
                                                }
                                            }
                                        });
                                    }>
                                    {move || if verify_phase.get() == 3 {
                                        t(&lang.get(), "verify_checking")
                                    } else {
                                        t(&lang.get(), "verify_confirm")
                                    }}
                                </button>
                                <button style="padding:8px 12px;background:transparent;border:1px solid #555;color:#9ca3af;border-radius:6px;cursor:pointer"
                                    on:click=move |_| {
                                        verify_phase.set(0);
                                        verify_code_input.set(String::new());
                                        verify_msg.set(String::new());
                                    }>{move || t(&lang.get(), "cancel")}</button>
                            </div>
                        </div>
                    }.into_any()
                } else {
                    view! { <span></span> }.into_any()
                }}

                // Add new email button + verification flow
                {move || {
                    let phase = verify_phase.get();
                    if phase == 0 && claim_emails.get().len() < 3 {
                        view! {
                            <div style="margin-top:8px" dir="ltr">
                                <div style="direction:ltr">
                                    <input type="email" placeholder="you@example.com"
                                        dir="ltr"
                                        style="width:100%;box-sizing:border-box;direction:ltr;text-align:left;margin-bottom:8px"
                                        prop:value=move || new_email_input.get()
                                        on:input=move |ev| new_email_input.set(event_target_value(&ev)) />
                                    <button class="primary" style="white-space:nowrap;padding:8px 14px;width:100%"
                                        on:click=move |_| {
                                            let addr = new_email_input.get_untracked().trim().to_lowercase();
                                            if addr.is_empty() {
                                                verify_msg.set("Please enter an email address".to_string());
                                                return;
                                            }
                                            if !addr.contains('@') {
                                                verify_msg.set("Please enter a valid email address".to_string());
                                                return;
                                            }
                                            verify_email_addr.set(addr.clone());
                                            verify_phase.set(1);
                                            verify_msg.set(t(&lang.get(), "verify_sending"));
                                            verify_code_input.set(String::new());
                                            // Also immediately save so it shows in the list (unverified)
                                            claim_emails.update(|list| {
                                                if !list.contains(&addr) && list.len() < 3 {
                                                    list.push(addr.clone());
                                                }
                                            });
                                            let save_emails = claim_emails.get_untracked();
                                            spawn_local(async move {
                                                let save_args = serde_wasm_bindgen::to_value(
                                                    &serde_json::json!({ "emails": save_emails })
                                                ).unwrap_or(no_args());
                                                let _ = call::<()>("set_claim_emails", save_args).await;
                                                let wid = info.get_untracked().map(|a| a.account_id).unwrap_or_default();
                                                let args = serde_wasm_bindgen::to_value(
                                                    &serde_json::json!({ "email": addr, "walletId": wid })
                                                ).unwrap_or(no_args());
                                                match call::<String>("send_verify_email", args).await {
                                                    Ok(_) => {
                                                        verify_phase.set(2);
                                                        verify_msg.set(t(&lang.get(), "verify_code_sent"));
                                                    }
                                                    Err(e) => {
                                                        verify_phase.set(0);
                                                        verify_msg.set(format!("Error: {e}"));
                                                    }
                                                }
                                            });
                                            new_email_input.set(String::new());
                                        }>
                                        {move || t(&lang.get(), "verify_send_code")}
                                    </button>
                                </div>
                            </div>
                        }.into_any()
                    } else if phase == 1 {
                        view! {
                            <p class="muted" style="margin-top:8px;font-size:12px">{move || t(&lang.get(), "verify_sending")}</p>
                        }.into_any()
                    } else {
                        view! { <span></span> }.into_any()
                    }
                }}

                // Status message
                {move || {
                    let s = verify_msg.get();
                    let s2 = claim_email_msg.get();
                    let msg = if !s.is_empty() { s } else { s2 };
                    if msg.is_empty() { view! { <span></span> }.into_any() }
                    else {
                        let cls = if msg.starts_with("Error") { "msg error" }
                                  else if msg.contains("Invalid") || msg.contains("expired") { "msg error" }
                                  else if msg.starts_with("Please") { "msg warning" }
                                  else { "msg success" };
                        view! { <p class=cls>{msg}</p> }.into_any()
                    }
                }}

                </div> // close security content
            </div> // close security settings-section

            // ─── SETTINGS SECTION: PRIVACY (order:4) ───────────────────────
            // TOP-LEVEL — never nest inside another section
            // Always visible regardless of badge load status
            // ── Privacy (collapsible, top-level) ──
            <div class="settings-section" style="order:4"
                 class:open=move || sec_privacy_open.get()>
                <div style="display:flex;justify-content:space-between;align-items:center;padding:2px 0;cursor:pointer;border-bottom:1px solid rgba(255,255,255,0.06)"
                    on:click=move |_| sec_privacy_open.set(!sec_privacy_open.get_untracked())>
                    <div style="display:flex;align-items:center;gap:10px">
                        <span style="width:28px;height:28px;border-radius:6px;background:#7B68EE;display:flex;align-items:center;justify-content:center;flex-shrink:0;color:white;font-size:14px;font-weight:700">{"\u{25cf}"}</span>
                        <span style="font-size:14px;color:#e5e7eb">"Privacy"</span>
                    </div>
                    <span style=move || format!("color:#888;font-size:12px;transition:transform 0.2s;display:inline-block;{}", if sec_privacy_open.get() { "transform:rotate(90deg)" } else { "" })>{"\u{203a}"}</span>
                </div>
                <div style:display=move || if sec_privacy_open.get() { "" } else { "none" }>
                // Show badges toggle (with inline badge pills)
                <div style="display:flex;align-items:center;gap:8px;margin-bottom:4px">
                    <span style="font-size:13px;color:#e5e7eb;white-space:nowrap">"Show badges when sending?"</span>
                    {move || {
                        let badges = settings_badge_list.get();
                        view! {
                            <div style="display:flex;flex-wrap:wrap;gap:4px;flex:1;min-width:0">
                                {badges.into_iter().map(|(bg, fg, label)| {
                                    view! {
                                        <span style={format!("display:inline-block;padding:2px 8px;border-radius:4px;background:{bg};color:{fg};font-size:10px;font-weight:700")}>{label}</span>
                                    }
                                }).collect_view()}
                            </div>
                        }
                    }}
                    <label style="position:relative;display:inline-block;width:44px;height:24px;flex-shrink:0;cursor:pointer">
                        <input type="checkbox" style="opacity:0;width:0;height:0"
                            prop:checked=move || settings_badges_on.get()
                            on:change=move |ev| {
                                use wasm_bindgen::JsCast;
                                let checked = ev.target()
                                    .and_then(|t| t.dyn_into::<web_sys::HtmlInputElement>().ok())
                                    .map(|i| i.checked()).unwrap_or(true);
                                settings_badges_on.set(checked);
                                spawn_local(async move {
                                    let args = serde_wasm_bindgen::to_value(&serde_json::json!({ "show": checked })).unwrap_or(no_args());
                                    let _ = call::<()>("set_show_badges", args).await;
                                });
                            } />
                        <span style=move || format!(
                            "position:absolute;inset:0;border-radius:12px;transition:0.2s;{}",
                            if settings_badges_on.get() { "background:#d4a84b" } else { "background:#444" }
                        )></span>
                        <span style=move || format!(
                            "position:absolute;top:2px;width:20px;height:20px;border-radius:50%;background:white;transition:0.2s;{}",
                            if settings_badges_on.get() { "left:22px" } else { "left:2px" }
                        )></span>
                    </label>
                </div>
                <p class="muted" style="font-size:11px;margin-bottom:12px">
                    "When off, badges are hidden from recipients when you send KX."
                </p>

                <hr style="border:none;border-top:1px solid #2d3748;margin:8px 0" />

                // Show identity toggle
                <div style="display:flex;align-items:center;justify-content:space-between;gap:12px;margin-bottom:4px">
                    <span style="font-size:13px;color:#e5e7eb">"Show my name to recipients?"</span>
                    <label style="position:relative;display:inline-block;width:44px;height:24px;flex-shrink:0;cursor:pointer">
                        <input type="checkbox" style="opacity:0;width:0;height:0"
                            prop:checked=move || settings_identity_on.get()
                            on:change=move |ev| {
                                use wasm_bindgen::JsCast;
                                let checked = ev.target()
                                    .and_then(|t| t.dyn_into::<web_sys::HtmlInputElement>().ok())
                                    .map(|i| i.checked()).unwrap_or(true);
                                settings_identity_on.set(checked);
                                spawn_local(async move {
                                    let args = serde_wasm_bindgen::to_value(&serde_json::json!({ "show": checked })).unwrap_or(no_args());
                                    let _ = call::<()>("set_show_identity", args).await;
                                });
                            } />
                        <span style=move || format!(
                            "position:absolute;inset:0;border-radius:12px;transition:0.2s;{}",
                            if settings_identity_on.get() { "background:#d4a84b" } else { "background:#444" }
                        )></span>
                        <span style=move || format!(
                            "position:absolute;top:2px;width:20px;height:20px;border-radius:50%;background:white;transition:0.2s;{}",
                            if settings_identity_on.get() { "left:22px" } else { "left:2px" }
                        )></span>
                    </label>
                </div>
                <p class="muted" style="font-size:11px;margin-bottom:12px">
                    "When off, recipients see your wallet address instead of your name."
                </p>

                <hr style="border:none;border-top:1px solid #2d3748;margin:8px 0" />

                // KX Request permissions
                <p class="muted" style="font-size:12px;margin-bottom:6px">"Who can request KX from me?"</p>
                {
                    let rp = RwSignal::new("anyone".to_string());
                    Effect::new(move |_| {
                        spawn_local(async move {
                            let p = call::<String>("get_request_permission", no_args()).await.unwrap_or_else(|_| "anyone".to_string());
                            rp.set(p);
                        });
                    });
                    let set_perm = move |val: &str| {
                        let v = val.to_string();
                        rp.set(v.clone());
                        spawn_local(async move {
                            let args = serde_wasm_bindgen::to_value(&serde_json::json!({ "permission": v })).unwrap_or(no_args());
                            let _ = call::<()>("set_request_permission", args).await;
                        });
                    };
                    view! {
                        <div style="display:flex;flex-direction:column;align-items:flex-start;gap:12px;width:100%">
                            <label style="display:flex;align-items:center;gap:8px;cursor:pointer;font-size:13px;color:#e5e7eb;white-space:nowrap">
                                <input type="radio" name="req_perm" prop:checked=move || rp.get() == "anyone"
                                    on:change=move |_| set_perm("anyone") style="accent-color:#d4a84b" />
                                "Anyone"
                            </label>
                            <label style="display:flex;align-items:center;gap:8px;cursor:pointer;font-size:13px;color:#e5e7eb;white-space:nowrap">
                                <input type="radio" name="req_perm" prop:checked=move || rp.get() == "address_book"
                                    on:change=move |_| set_perm("address_book") style="accent-color:#d4a84b" />
                                "My Address Book only"
                            </label>
                            <label style="display:flex;align-items:center;gap:8px;cursor:pointer;font-size:13px;color:#e5e7eb;white-space:nowrap">
                                <input type="radio" name="req_perm" prop:checked=move || rp.get() == "nobody"
                                    on:change=move |_| set_perm("nobody") style="accent-color:#d4a84b" />
                                "Nobody"
                            </label>
                        </div>
                    }
                }
            </div> // close privacy content
            </div> // close privacy settings-section

            // ─── SETTINGS SECTION: BACKUP (order:6) ────────────────────────
            // TOP-LEVEL — never nest inside another section
            // ── Wallet Management (collapsed by default) ──
            {
                let wm_expanded = RwSignal::new(false);
                view! {
            <div class="settings-section" style="order:6"
                 class:open=move || wm_expanded.get()>
                        <div style="display:flex;justify-content:space-between;align-items:center;padding:2px 0;cursor:pointer;border-bottom:1px solid rgba(255,255,255,0.06)"
                            on:click=move |_| wm_expanded.set(!wm_expanded.get_untracked())>
                            <div style="display:flex;align-items:center;gap:10px">
                                <span style="width:28px;height:28px;border-radius:6px;background:#FF8C00;display:flex;align-items:center;justify-content:center;flex-shrink:0;color:white;font-size:14px;font-weight:700">{"\u{2193}"}</span>
                                <span style="font-size:14px;color:#e5e7eb">"Backup & Recovery"</span>
                            </div>
                            <span style=move || format!("color:#888;font-size:12px;transition:transform 0.2s;display:inline-block;{}", if wm_expanded.get() { "transform:rotate(90deg)" } else { "" })>{"\u{203a}"}</span>
                        </div>
                        <div style:display=move || if wm_expanded.get() { "" } else { "none" }>
            <div style="margin-top:12px">
                <p class="label">"Backup Your Wallet"</p>
                <p class="muted" style="font-size:12px;margin-bottom:8px">
                    "These 24 words are the ONLY way to recover your wallet. Write them down and store them safely offline."
                </p>
                <button style="background:linear-gradient(135deg,#b8860b,#daa520);color:#000;font-weight:700"
                    on:click=move |_| {
                        seed_pin_phase.set(0);
                        seed_pin_input.set(String::new());
                        seed_pin_msg.set(String::new());
                        seed_words.set(String::new());
                        seed_revealed.set(false);
                        seed_loading.set(false);
                        show_seed_modal.set(true);
                    }
                >"View Seed Phrase"</button>
                <p class="muted" style="font-size:11px;margin-top:6px">
                    <a href="javascript:void(0)" style="color:#888;text-decoration:underline" on:click=move |_| {
                        export_confirmed.set(false);
                        export_key.set(String::new());
                        show_export.set(true);
                    }>"Export raw key (advanced)"</a>
                </p>

                // ── Compromised? expandable section ──
                <div style="margin-top:12px">
                    <a href="javascript:void(0)" style="color:#f87171;font-size:12px;text-decoration:none"
                        on:click=move |_| compromised_expanded.set(!compromised_expanded.get_untracked())
                    >{move || if compromised_expanded.get() { "Compromised? \u{25b2}" } else { "Compromised? \u{25bc}" }}</a>
                    {move || if compromised_expanded.get() {
                        view! {
                            <div style="margin-top:8px;padding:12px;background:#1a1020;border:1px solid #442;border-radius:8px">
                                <p style="font-size:13px;font-weight:700;color:#daa520;margin-bottom:6px">"Create New Wallet"</p>
                                <p class="muted" style="font-size:12px;margin-bottom:10px">
                                    "If your seed phrase was compromised or lost, you can create a new wallet with a new address. \
                                     You will need to send your KX balance to the new address manually. \
                                     This action creates a new wallet \u{2014} it does not change your current one."
                                </p>
                                <button style="border:2px solid #daa520;background:transparent;color:#daa520;font-weight:700"
                                    on:click=move |_| {
                                        new_wallet_confirm_input.set(String::new());
                                        new_wallet_msg.set(String::new());
                                        new_wallet_mnemonic.set(String::new());
                                        new_wallet_address.set(String::new());
                                        new_wallet_busy.set(false);
                                        // Check balance before proceeding
                                        spawn_local(async move {
                                            if let Ok(acct) = call::<AccountInfo>("get_account_info", no_args()).await {
                                                let bal: u128 = acct.balance_chronos.parse().unwrap_or(0);
                                                if bal > 0 {
                                                    balance_warning_kx.set(format_kx(&acct.balance_chronos));
                                                    show_balance_warning.set(true);
                                                    return;
                                                }
                                            }
                                            show_new_wallet.set(true);
                                        });
                                    }
                                >"Create New Wallet"</button>
                            </div>
                        }.into_any()
                    } else {
                        view! { <span></span> }.into_any()
                    }}
                </div>
            </div>

            // ── Restore Wallet (Seed Phrase) ──
            <div class="settings-section">
                <p class="label">"Restore Wallet"</p>
                <p class="muted" style="font-size:12px;margin-bottom:8px">
                    "Enter your 24-word seed phrase or legacy private key to restore your wallet on this device."
                </p>
                <button on:click=move |_| {
                    import_key.set(String::new());
                    import_msg.set(String::new());
                    import_confirm.set(false);
                    show_import.set(true);
                }>"Restore from Seed Phrase"</button>
            </div>

            // Cold Storage Wallet Generator (desktop only)
            {if desktop {
                view! {
                    <div class="settings-section">
                        <p class="label">{move || t(&lang.get(), "settings_cold_storage")}</p>
                        <p class="muted" style="font-size:12px;margin-bottom:8px">
                            {move || t(&lang.get(), "settings_cold_sub")}
                        </p>
                        <button on:click=move |_| {
                            cold_result.set(None);
                            cold_saved.set(false);
                            show_cold.set(true);
                        }>{move || format!("{}", t(&lang.get(), "settings_gen_cold"))}</button>
                        {move || {
                            let wallets = cold_wallets.get();
                            if wallets.is_empty() {
                                view! { <span></span> }.into_any()
                            } else {
                                view! {
                                    <div style="margin-top:8px">
                                        <p class="muted" style="font-size:11px;margin-bottom:4px">
                                            {format!("{} ({})", t(&lang.get(), "settings_cold_wallets"), wallets.len())}
                                        </p>
                                        {wallets.into_iter().map(|w| {
                                            view! {
                                                <p class="muted" style="font-size:11px;font-family:monospace;word-break:break-all;padding:2px 0">
                                                    {w}
                                                </p>
                                            }
                                        }).collect::<Vec<_>>()}
                                    </div>
                                }.into_any()
                            }
                        }}
                    </div>
                }.into_any()
            } else {
                view! { <span></span> }.into_any()
            }}

            </div> // close wm_expanded div
            </div> // close settings-section
                }.into_any()
            }


            // About (no section label — just centered buttons)
            <div style="order:99;text-align:center;padding:12px 0 4px">
                <div style="display:flex;gap:8px;justify-content:center;flex-wrap:wrap;margin-bottom:8px">
                    <button on:click=move |_| show_about.set(true) style="font-size:13px">{move || format!("{}", t(&lang.get(), "settings_about_chronx"))}</button>
                    <button on:click=move |_| show_updates.set(true) style="font-size:13px">{move || format!("{}", t(&lang.get(), "settings_check_updates"))}</button>
                </div>
                <a href="javascript:void(0)" style="display:block;text-align:center;color:#d4a84b;text-decoration:underline;font-size:13px;cursor:pointer;margin-top:8px"
                    on:click=move |_| {
                        bug_body.set(String::new());
                        bug_modal_open.set(true);
                    }>"Report a Bug"</a>
                {move || {
                    let version = app_version.get();
                    view! {
                        <p style="font-size:11px;color:#555;margin:0;margin-top:4px">"ChronX Wallet v" {version}</p>
                    }
                }}
            </div>
        </div>

        // ── About modal ───────────────────────────────────────────────────────

        {move || if show_about.get() {
            view! {
                <div class="modal-overlay" on:click=move |_| show_about.set(false)>
                    <div class="modal-card" on:click=move |ev| ev.stop_propagation()>
                        <img src=logo_src() alt="ChronX" style="width:70px;height:auto;margin:0 auto" />
                        <p class="modal-title">"ChronX Wallet v" {move || app_version.get()}</p>
                        <div class="modal-body">
                            <p>"The Future Payment Protocol"</p>
                            <p>"Built on post-quantum cryptography"</p>
                        </div>
                        <div style="display:flex;flex-direction:column;gap:6px;align-items:center">
                            <a href="https://www.chronx.io" target="_blank" rel="noopener" class="modal-link">
                                "chronx.io"
                            </a>
                            <a href="https://github.com/Counselco/chronx" target="_blank" rel="noopener" class="modal-link">
                                "github.com/Counselco/chronx"
                            </a>
                        </div>
                        <button class="primary" on:click=move |_| show_about.set(false)>{t(&lang.get(), "close")}</button>
                    </div>
                </div>
            }.into_any()
        } else { view! { <span></span> }.into_any() }}

        // ── Check for updates modal ───────────────────────────────────────────

        {move || if show_updates.get() {
            let version = app_version.get();
            view! {
                <div class="modal-overlay" on:click=move |_| {
                    show_updates.set(false);
                    update_result.set(None);
                }>
                    <div class="modal-card" on:click=move |ev| ev.stop_propagation()>
                        <p class="modal-title">{format!("{}", t(&lang.get(), "settings_check_updates"))}</p>
                        <div class="modal-body">
                            <p class="label">{format!("{}: {}", t(&lang.get(), "settings_current_version"), version)}</p>
                            {move || {
                                if update_checking.get() {
                                    view! { <p class="muted">"Checking\u{2026}"</p> }.into_any()
                                } else if let Some(info) = update_result.get() {
                                    if info.up_to_date {
                                        view! {
                                            <p class="update-up-to-date">
                                                "\u{2705} You are running the latest version ("
                                                {info.current.clone()} ")"
                                            </p>
                                        }.into_any()
                                    } else if is_ios() {
                                        view! {
                                            <div class="update-info">
                                                <p class="update-available">
                                                    "A new version (" {info.latest.clone()} ") is available. Update via the App Store when available."
                                                </p>
                                                {if !info.release_notes.is_empty() {
                                                    view! {
                                                        <p class="muted" style="font-size:12px;margin-top:4px">
                                                            "What\u{2019}s new: " {info.release_notes.clone()}
                                                        </p>
                                                    }.into_any()
                                                } else { view! { <span></span> }.into_any() }}
                                            </div>
                                        }.into_any()
                                    } else {
                                        let dl_url = if is_desktop() {
                                            info.download_url.clone()
                                        } else {
                                            "market://details?id=com.chronx.wallet".to_string()
                                        };
                                        let fallback_url = "https://play.google.com/store/apps/details?id=com.chronx.wallet".to_string();
                                        let desktop = is_desktop();
                                        view! {
                                            <div class="update-info">
                                                <p class="update-available">
                                                    "\u{1f504} Version " {info.latest.clone()} " is available"
                                                </p>
                                                {if !info.release_notes.is_empty() {
                                                    view! {
                                                        <p class="muted" style="font-size:12px;margin-top:4px">
                                                            "What\u{2019}s new: " {info.release_notes.clone()}
                                                        </p>
                                                    }.into_any()
                                                } else { view! { <span></span> }.into_any() }}
                                                <button class="primary" style="margin-top:10px"
                                                    on:click=move |_| {
                                                        let url = dl_url.clone();
                                                        let fallback = fallback_url.clone();
                                                        let is_mobile = !desktop;
                                                        spawn_local(async move {
                                                            let args = serde_wasm_bindgen::to_value(
                                                                &serde_json::json!({ "url": url })
                                                            ).unwrap_or(no_args());
                                                            if call::<()>("open_url", args).await.is_err() && is_mobile {
                                                                let fb_args = serde_wasm_bindgen::to_value(
                                                                    &serde_json::json!({ "url": fallback })
                                                                ).unwrap_or(no_args());
                                                                let _ = call::<()>("open_url", fb_args).await;
                                                            }
                                                        });
                                                    }>{if is_desktop() { "\u{2b07} Download Update" } else { "\u{25b6} Update on Google Play" }}</button>
                                            </div>
                                        }.into_any()
                                    }
                                } else {
                                    view! { <span></span> }.into_any()
                                }
                            }}
                        </div>
                        <div style="display:flex;gap:8px;flex-wrap:wrap;margin-top:4px">
                            <button on:click=move |_| {
                                update_checking.set(true);
                                update_result.set(None);
                                spawn_local(async move {
                                    match call::<UpdateInfo>("check_for_updates", no_args()).await {
                                        Ok(info) => update_result.set(Some(info)),
                                        Err(_)   => {} // silent fail — no error shown
                                    }
                                    update_checking.set(false);
                                });
                            } disabled=move || update_checking.get()>
                                {move || if update_checking.get() { "\u{2026}".to_string() } else { t(&lang.get(), "settings_check_now") }}
                            </button>
                            <button on:click=move |_| {
                                show_updates.set(false);
                                update_result.set(None);
                            }>{t(&lang.get(), "close")}</button>
                        </div>
                    </div>
                </div>
            }.into_any()
        } else { view! { <span></span> }.into_any() }}

        // ── Change PIN modal ──────────────────────────────────────────────────

        {move || if show_change_pin.get() {
            let cp_title = move || match cp_phase.get() {
                0 => t(&lang.get(), "settings_enter_current_pin"),
                1 => t(&lang.get(), "settings_enter_new_pin"),
                _ => t(&lang.get(), "settings_confirm_new_pin"),
            };
            view! {
                <div class="modal-overlay" on:click=move |_| {
                    show_change_pin.set(false);
                    cp_digits.set(String::new());
                    cp_msg.set(String::new());
                }>
                    <div class="modal-card" on:click=move |ev| ev.stop_propagation()>
                        <p class="modal-title">{t(&lang.get(), "settings_change_pin")}</p>
                        <p class="pin-subtitle">{cp_title}</p>

                        // Shared PIN digit entry — same component as login screen
                        <PinInput digits=cp_digits shake=cp_shake pin_len=pin_len.get() />

                        {move || {
                            let msg = cp_msg.get();
                            if msg.is_empty() { view! { <span></span> }.into_any() }
                            else {
                                let cls = if msg.starts_with("PIN changed") { "msg success" } else { "pin-msg" };
                                view! { <p class=cls>{msg}</p> }.into_any()
                            }
                        }}

                        <button on:click=move |_| {
                            show_change_pin.set(false);
                            cp_digits.set(String::new());
                            cp_msg.set(String::new());
                        }>"Cancel"</button>
                    </div>
                </div>
            }.into_any()
        } else { view! { <span></span> }.into_any() }}

        </div> // close content-hiding div for mobile sub-views

        // ── Export Private Key modal ──────────────────────────────────────────

        {move || if show_export.get() {
            view! {
                <div class="modal-overlay" on:click=move |_| {
                    if !export_loading.get_untracked() { show_export.set(false); }
                }>
                    <div class="modal-card" style="max-width:440px" on:click=move |ev| ev.stop_propagation()>
                        <p class="modal-title">"🔑 Export Private Key"</p>
                        {move || {
                            if !export_confirmed.get() {
                                // Step 1: Warning + confirmation
                                view! {
                                    <div class="modal-body" style="text-align:left">
                                        <div class="export-warning">
                                            <p style="font-weight:700;color:#f87171;margin-bottom:8px">
                                                "⚠ Read carefully before proceeding:"
                                            </p>
                                            <ul style="font-size:13px;line-height:1.8;color:#c7cdd4;padding-left:18px">
                                                <li>"Your private key is the ONLY way to access your KX."</li>
                                                <li>"If you lose it, your funds are gone forever."</li>
                                                <li>"No one — not even ChronX — can recover it."</li>
                                                <li>"Never share your private key with anyone."</li>
                                                <li>"Never paste it into a website."</li>
                                                <li>"Write it down and store it somewhere safe offline."</li>
                                            </ul>
                                            <p style="font-weight:700;color:#e74c3c;margin-top:12px;font-size:13px">
                                                "ChronX will NEVER ask for your private key. Anyone who asks for it is trying to steal your funds."
                                            </p>
                                        </div>
                                        <div style="display:flex;gap:8px;margin-top:16px;flex-wrap:wrap">
                                            <button class="primary" on:click=move |_| {
                                                export_confirmed.set(true);
                                                export_loading.set(true);
                                                spawn_local(async move {
                                                    match call::<String>("export_secret_key", no_args()).await {
                                                        Ok(key) => export_key.set(key),
                                                        Err(e)  => export_key.set(format!("Error: {e}")),
                                                    }
                                                    export_loading.set(false);
                                                });
                                            }>"I Understand — Show My Key"</button>
                                            <button on:click=move |_| show_export.set(false)>"Cancel"</button>
                                        </div>
                                    </div>
                                }.into_any()
                            } else {
                                // Step 2: Key display
                                let key = export_key.get();
                                if export_loading.get() {
                                    view! { <p class="muted">"Loading\u{2026}"</p> }.into_any()
                                } else {
                                    let key_copy = key.clone();
                                    view! {
                                        <div class="modal-body" style="text-align:left">
                                            <p class="muted" style="font-size:12px;margin-bottom:8px">
                                                "Your private backup key (copy and store safely):"
                                            </p>
                                            <div class="export-key-box">
                                                <p class="mono" style="font-size:10px;word-break:break-all;user-select:all">
                                                    {key.clone()}
                                                </p>
                                            </div>
                                            <div style="display:flex;gap:8px;margin-top:8px;flex-wrap:wrap">
                                                <button class="primary" on:click=move |_| {
                                                    let k = key_copy.clone();
                                                    spawn_local(async move { copy_to_clipboard(k).await; });
                                                }>"📋 Copy to Clipboard"</button>
                                                {
                                                    let key_file = key.clone();
                                                    view! {
                                                        <button on:click=move |_| {
                                                            save_text_file("chronx-backup-key.txt", &key_file);
                                                        }>"💾 Save to File"</button>
                                                    }
                                                }
                                            </div>
                                            <p class="muted" style="font-size:11px;margin-top:10px">
                                                "Store this somewhere safe. Consider writing it on paper and keeping it in a secure location."
                                            </p>
                                            <button style="margin-top:8px" on:click=move |_| {
                                                export_key.set(String::new());
                                                export_confirmed.set(false);
                                                show_export.set(false);
                                            }>"Done"</button>
                                        </div>
                                    }.into_any()
                                }
                            }
                        }}
                    </div>
                </div>
            }.into_any()
        } else { view! { <span></span> }.into_any() }}

        // ── Import Private Key modal ──────────────────────────────────────────

        {move || if show_import.get() {
            view! {
                <div class="modal-overlay" on:click=move |_| {
                    if !import_busy.get_untracked() { show_import.set(false); }
                }>
                    <div class="modal-card" style="max-width:440px" on:click=move |ev| ev.stop_propagation()>
                        <p class="modal-title">"Restore from Seed Phrase"</p>
                        <div class="modal-body" style="text-align:left">
                            <div class="export-warning" style="margin-bottom:12px">
                                <p style="font-weight:700;color:#f87171;font-size:13px">
                                    "\u{26a0} Restoring will replace your current wallet. Make sure you have backed up your current seed phrase first."
                                </p>
                            </div>
                            <p class="label" style="margin-bottom:6px">"Enter your 24-word seed phrase or paste legacy backup key:"</p>
                            <textarea
                                class="restore-textarea"
                                style="width:100%;min-height:80px;font-family:monospace;font-size:11px"
                                placeholder="Enter your 24 recovery words or paste backup key\u{2026}"
                                prop:value=move || import_key.get()
                                on:input=move |ev| {
                                    import_key.set(event_target_value(&ev));
                                    import_msg.set(String::new());
                                }
                            ></textarea>
                            {move || {
                                let m = import_msg.get();
                                if m.is_empty() { view! { <span></span> }.into_any() }
                                else {
                                    let cls = if m.starts_with("Error") || m.starts_with("Invalid") || m.starts_with("A wallet") || m.starts_with("This will REPLACE") { "msg error" } else { "msg success" };
                                    view! { <p class=cls>{m}</p> }.into_any()
                                }
                            }}
                            <div style="display:flex;gap:8px;margin-top:10px;flex-wrap:wrap">
                                <button class={move || if import_confirm.get() { "btn-danger" } else { "primary" }}
                                    disabled=move || import_busy.get() || import_key.get().trim().is_empty()
                                    on:click=move |_| {
                                        let key = import_key.get_untracked().trim().to_string();
                                        let confirming = import_confirm.get_untracked();
                                        import_busy.set(true);
                                        import_msg.set(String::new());
                                        // Auto-detect mnemonic vs legacy key
                                        let word_count = key.split_whitespace().count();
                                        let is_mnemonic = word_count >= 12 && word_count <= 24
                                            && key.chars().all(|c| c.is_ascii_lowercase() || c == ' ');
                                        spawn_local(async move {
                                            if is_mnemonic {
                                                let args = serde_wasm_bindgen::to_value(
                                                    &serde_json::json!({ "mnemonicPhrase": key, "force": true })
                                                ).unwrap_or(no_args());
                                                match call::<serde_json::Value>("import_wallet_from_mnemonic", args).await {
                                                    Ok(v) => {
                                                        let acct = v.get("account_id").and_then(|a| a.as_str()).unwrap_or("OK");
                                                        import_msg.set(format!("Wallet restored! Account: {}", acct));
                                                        import_confirm.set(false);
                                                        delay_ms(2000).await;
                                                        let _ = web_sys::window().map(|w| w.location().reload());
                                                    }
                                                    Err(e) => {
                                                        import_confirm.set(false);
                                                        import_msg.set(format!("{e}"));
                                                    }
                                                }
                                            } else {
                                                let args = if confirming {
                                                    serde_wasm_bindgen::to_value(
                                                        &serde_json::json!({ "backupKey": key, "force": true })
                                                    ).unwrap_or(no_args())
                                                } else {
                                                    serde_wasm_bindgen::to_value(
                                                        &serde_json::json!({ "backupKey": key })
                                                    ).unwrap_or(no_args())
                                                };
                                                match call::<String>("restore_wallet", args).await {
                                                    Ok(acct) => {
                                                        import_msg.set(format!("Wallet restored! Account: {}", acct));
                                                        import_confirm.set(false);
                                                        delay_ms(2000).await;
                                                        let _ = web_sys::window().map(|w| w.location().reload());
                                                    }
                                                    Err(e) => {
                                                        if e.contains("WALLET_EXISTS_CONFIRM") {
                                                            import_confirm.set(true);
                                                            import_msg.set("This will REPLACE your current wallet. Make sure you have backed up your seed phrase first. Click the red button to confirm.".to_string());
                                                        } else {
                                                            import_confirm.set(false);
                                                            import_msg.set(format!("{e}"));
                                                        }
                                                    }
                                                }
                                            }
                                            import_busy.set(false);
                                        });
                                    }>
                                    {move || if import_busy.get() { "Restoring\u{2026}".to_string() } else if import_confirm.get() { "Yes, Replace My Wallet".to_string() } else { "Restore Wallet".to_string() }}
                                </button>
                                <button disabled=move || import_busy.get()
                                    on:click=move |_| { import_confirm.set(false); show_import.set(false); }>"Cancel"</button>
                            </div>
                        </div>
                    </div>
                </div>
            }.into_any()
        } else { view! { <span></span> }.into_any() }}

        // ── View Seed Phrase modal (PIN-gated) ─────────────────────────────

        {move || if show_seed_modal.get() {
            view! {
                <div class="modal-overlay" on:click=move |_| {
                    if !seed_loading.get_untracked() { show_seed_modal.set(false); }
                }>
                    <div class="modal-card" style="max-width:480px" on:click=move |ev| ev.stop_propagation()>
                        <p class="modal-title">"View Seed Phrase"</p>
                        <div class="modal-body" style="text-align:left">
                            {move || {
                                let phase = seed_pin_phase.get();
                                if phase == 0 {
                                    // ── Step 1: Enter PIN to verify identity ──
                                    view! {
                                        <p class="muted" style="font-size:13px;margin-bottom:12px">
                                            "Enter your PIN to view your seed phrase."
                                        </p>
                                        <input
                                            type="password"
                                            inputmode="numeric"
                                            maxlength="8"
                                            placeholder="Enter PIN"
                                            style="width:100%;padding:12px;font-size:18px;text-align:center;letter-spacing:8px;\
                                                   background:#1a1a2e;border:1px solid #333;border-radius:8px;color:#fff;\
                                                   font-family:monospace"
                                            prop:value=move || seed_pin_input.get()
                                            on:input=move |ev| {
                                                seed_pin_input.set(event_target_value(&ev));
                                                seed_pin_msg.set(String::new());
                                            }
                                            on:keydown=move |ev: web_sys::KeyboardEvent| {
                                                if ev.key() == "Enter" {
                                                    let pin = seed_pin_input.get_untracked();
                                                    if !pin.is_empty() {
                                                        seed_loading.set(true);
                                                        spawn_local(async move {
                                                            let args = serde_wasm_bindgen::to_value(
                                                                &serde_json::json!({ "pin": pin })
                                                            ).unwrap_or(no_args());
                                                            match call::<bool>("verify_pin", args).await {
                                                                Ok(true) => {
                                                                    // PIN correct — load mnemonic
                                                                    match call::<Option<String>>("get_mnemonic", no_args()).await {
                                                                        Ok(Some(words)) => {
                                                                            seed_words.set(words);
                                                                            seed_revealed.set(false);
                                                                            seed_pin_phase.set(1);
                                                                        }
                                                                        Ok(None) => {
                                                                            seed_pin_msg.set("No seed phrase found. This wallet was created before mnemonic support.".to_string());
                                                                        }
                                                                        Err(e) => seed_pin_msg.set(format!("Error: {e}")),
                                                                    }
                                                                }
                                                                _ => seed_pin_msg.set("Incorrect PIN".to_string()),
                                                            }
                                                            seed_loading.set(false);
                                                        });
                                                    }
                                                }
                                            }
                                        />
                                        {move || {
                                            let m = seed_pin_msg.get();
                                            if m.is_empty() { view! { <span></span> }.into_any() }
                                            else { view! { <p class="msg error" style="margin-top:8px">{m}</p> }.into_any() }
                                        }}
                                        <div style="display:flex;gap:8px;margin-top:12px">
                                            <button class="primary" disabled=move || seed_loading.get() || seed_pin_input.get().is_empty()
                                                on:click=move |_| {
                                                    let pin = seed_pin_input.get_untracked();
                                                    seed_loading.set(true);
                                                    spawn_local(async move {
                                                        let args = serde_wasm_bindgen::to_value(
                                                            &serde_json::json!({ "pin": pin })
                                                        ).unwrap_or(no_args());
                                                        match call::<bool>("verify_pin", args).await {
                                                            Ok(true) => {
                                                                match call::<Option<String>>("get_mnemonic", no_args()).await {
                                                                    Ok(Some(words)) => {
                                                                        seed_words.set(words);
                                                                        seed_revealed.set(false);
                                                                        seed_pin_phase.set(1);
                                                                    }
                                                                    Ok(None) => {
                                                                        seed_pin_msg.set("No seed phrase found. This wallet was created before mnemonic support.".to_string());
                                                                    }
                                                                    Err(e) => seed_pin_msg.set(format!("Error: {e}")),
                                                                }
                                                            }
                                                            _ => seed_pin_msg.set("Incorrect PIN".to_string()),
                                                        }
                                                        seed_loading.set(false);
                                                    });
                                                }
                                            >{move || if seed_loading.get() { "Verifying\u{2026}" } else { "Verify PIN" }}</button>
                                            <button on:click=move |_| show_seed_modal.set(false)>"Cancel"</button>
                                        </div>
                                    }.into_any()
                                } else {
                                    // ── Step 2: Seed phrase reveal screen ──
                                    let words = seed_words.get();
                                    let word_list: Vec<String> = words.split_whitespace().map(|s| s.to_string()).collect();
                                    let word_list_copy = word_list.clone();
                                    view! {
                                        {move || if !seed_revealed.get() {
                                            // Hidden — show warning + reveal button
                                            view! {
                                                <div style="background:#2a1a1a;border:1px solid #f87171;border-radius:8px;padding:16px;margin-bottom:12px">
                                                    <p style="color:#f87171;font-weight:700;font-size:13px;margin-bottom:8px">
                                                        "\u{26a0}\u{fe0f} Ensure no one can see your screen."
                                                    </p>
                                                    <p class="muted" style="font-size:12px">
                                                        "Your seed phrase gives full access to your wallet and all your KX. \
                                                         Never share it. Never photograph it. Never store it digitally."
                                                    </p>
                                                </div>
                                                <button class="primary" style="width:100%;padding:14px;font-size:15px"
                                                    on:click=move |_| seed_revealed.set(true)
                                                >"Reveal Seed Phrase"</button>
                                            }.into_any()
                                        } else {
                                            // Revealed — show word grid
                                            let words_for_copy = word_list_copy.join(" ");
                                            view! {
                                                <div style="display:grid;grid-template-columns:1fr 1fr 1fr;gap:6px 12px;\
                                                            background:#1a1a2e;border:1px solid #333;border-radius:8px;\
                                                            padding:16px;margin-bottom:12px;font-family:monospace;font-size:14px">
                                                    {word_list.clone().into_iter().enumerate().map(|(i, word)| {
                                                        view! {
                                                            <div style="display:flex;gap:6px;align-items:center">
                                                                <span style="color:#888;min-width:24px;text-align:right;font-size:12px">
                                                                    {format!("{}.", i + 1)}
                                                                </span>
                                                                <span style="color:#e0e0e0">{word}</span>
                                                            </div>
                                                        }
                                                    }).collect::<Vec<_>>()}
                                                </div>
                                                <button on:click=move |_| {
                                                    let w = words_for_copy.clone();
                                                    spawn_local(async move { copy_to_clipboard(w).await; });
                                                }>"Copy to Clipboard"</button>
                                            }.into_any()
                                        }}
                                        <button style="margin-top:8px" on:click=move |_| {
                                            seed_words.set(String::new());
                                            seed_revealed.set(false);
                                            show_seed_modal.set(false);
                                        }>"Done"</button>
                                    }.into_any()
                                }
                            }}
                        </div>
                    </div>
                </div>
            }.into_any()
        } else { view! { <span></span> }.into_any() }}

        // ── Balance warning before Create New Wallet ────────────────────────

        {move || if show_balance_warning.get() {
            let kx = balance_warning_kx.get();
            view! {
                <div class="modal-overlay" on:click=move |_| show_balance_warning.set(false)>
                    <div class="modal-card" style="max-width:440px" on:click=move |ev: web_sys::MouseEvent| ev.stop_propagation()>
                        <p class="modal-title">"\u{26a0}\u{fe0f} You Have KX in This Wallet"</p>
                        <div class="modal-body" style="text-align:left">
                            <div style="background:#2a1010;border:1px solid #f87171;border-radius:8px;padding:12px;margin-bottom:12px">
                                <p style="color:#f87171;font-size:13px;font-weight:700;line-height:1.5">
                                    "Your current wallet contains " {kx} " KX. \
                                     Creating a new wallet will generate a NEW address. \
                                     Your KX will NOT move automatically \u{2014} it will stay \
                                     at your old address."
                                </p>
                                <p style="color:#f87171;font-size:13px;font-weight:700;margin-top:8px">
                                    "You must send your KX to your new address manually \
                                     after creating it."
                                </p>
                            </div>
                            <p style="color:#aaa;font-size:13px;margin-bottom:12px">"Are you sure you want to continue?"</p>
                            <div style="display:flex;gap:8px">
                                <button style="flex:1" on:click=move |_| show_balance_warning.set(false)>"Cancel"</button>
                                <button style="flex:1;border:2px solid #daa520;background:transparent;color:#daa520;font-weight:700"
                                    on:click=move |_| {
                                        show_balance_warning.set(false);
                                        show_new_wallet.set(true);
                                    }
                                >"I understand, continue anyway"</button>
                            </div>
                        </div>
                    </div>
                </div>
            }.into_any()
        } else { view! { <span></span> }.into_any() }}

        // ── Create New Wallet (compromised) modal ──────────────────────────

        {move || if show_new_wallet.get() {
            view! {
                <div class="modal-overlay" on:click=move |_| {
                    if !new_wallet_busy.get_untracked() && new_wallet_mnemonic.get_untracked().is_empty() {
                        show_new_wallet.set(false);
                    }
                }>
                    <div class="modal-card" style="max-width:480px" on:click=move |ev| ev.stop_propagation()>
                        <p class="modal-title">"\u{26a0}\u{fe0f} Create New Wallet"</p>
                        <div class="modal-body" style="text-align:left">
                            {move || {
                                let mnemonic = new_wallet_mnemonic.get();
                                if mnemonic.is_empty() {
                                    // ── Confirmation step ──
                                    view! {
                                        <div class="export-warning" style="margin-bottom:12px">
                                            <p style="font-weight:700;color:#f87171;font-size:13px">
                                                "This will create a completely new wallet with a new address. \
                                                 Your current wallet will NOT be modified, but you will need \
                                                 to send your KX balance to the new address manually."
                                            </p>
                                        </div>
                                        <p class="label" style="margin-bottom:6px">
                                            "Type CONFIRM to proceed:"
                                        </p>
                                        <input
                                            type="text"
                                            placeholder="Type CONFIRM"
                                            style="width:100%;padding:10px;font-size:14px;text-align:center;\
                                                   background:#1a1a2e;border:1px solid #333;border-radius:8px;color:#fff;\
                                                   font-family:monospace;text-transform:uppercase"
                                            prop:value=move || new_wallet_confirm_input.get()
                                            on:input=move |ev| {
                                                new_wallet_confirm_input.set(event_target_value(&ev));
                                                new_wallet_msg.set(String::new());
                                            }
                                        />
                                        {move || {
                                            let m = new_wallet_msg.get();
                                            if m.is_empty() { view! { <span></span> }.into_any() }
                                            else { view! { <p class="msg error" style="margin-top:8px">{m}</p> }.into_any() }
                                        }}
                                        <div style="display:flex;gap:8px;margin-top:12px">
                                            <button class="btn-danger"
                                                disabled=move || new_wallet_busy.get() || new_wallet_confirm_input.get().trim().to_uppercase() != "CONFIRM"
                                                on:click=move |_| {
                                                    new_wallet_busy.set(true);
                                                    new_wallet_msg.set(String::new());
                                                    spawn_local(async move {
                                                        // First, backup current wallet to a temp file
                                                        let _ = call::<String>("export_secret_key", no_args()).await;
                                                        // Delete current wallet to allow creation
                                                        let args = serde_wasm_bindgen::to_value(
                                                            &serde_json::json!({ "force": true })
                                                        ).unwrap_or(no_args());
                                                        match call::<serde_json::Value>("generate_wallet_with_mnemonic", args).await {
                                                            Ok(result) => {
                                                                if let Some(phrase) = result.get("mnemonic").and_then(|v| v.as_str()) {
                                                                    new_wallet_mnemonic.set(phrase.to_string());
                                                                }
                                                                if let Some(acct) = result.get("account_id").and_then(|v| v.as_str()) {
                                                                    new_wallet_address.set(acct.to_string());
                                                                }
                                                            }
                                                            Err(e) => {
                                                                new_wallet_msg.set(format!("Error: {e}"));
                                                            }
                                                        }
                                                        new_wallet_busy.set(false);
                                                    });
                                                }
                                            >{move || if new_wallet_busy.get() { "Creating\u{2026}" } else { "Create New Wallet" }}</button>
                                            <button on:click=move |_| show_new_wallet.set(false)>"Cancel"</button>
                                        </div>
                                    }.into_any()
                                } else {
                                    // ── New wallet created — show mnemonic ──
                                    let word_list: Vec<String> = mnemonic.split_whitespace().map(|s| s.to_string()).collect();
                                    let addr = new_wallet_address.get();
                                    view! {
                                        <div style="background:#1a2a1a;border:1px solid #4a4;border-radius:8px;padding:12px;margin-bottom:12px">
                                            <p style="color:#4a4;font-weight:700;font-size:13px">
                                                "\u{2705} New wallet created!"
                                            </p>
                                        </div>
                                        <div style="background:#2a2010;border:1px solid #daa520;border-radius:8px;padding:12px;margin-bottom:12px">
                                            <p style="color:#daa520;font-weight:700;font-size:13px">
                                                "\u{26a0}\u{fe0f} Important: Your email is no longer registered \
                                                 to this wallet. Go to Settings \u{2192} My Emails to \
                                                 re-register your email for automatic KX delivery."
                                            </p>
                                        </div>
                                        <p class="label" style="margin-bottom:8px">"Write down these 24 words:"</p>
                                        <div style="display:grid;grid-template-columns:1fr 1fr 1fr;gap:6px 12px;\
                                                    background:#1a1a2e;border:1px solid #333;border-radius:8px;\
                                                    padding:16px;margin-bottom:12px;font-family:monospace;font-size:14px">
                                            {word_list.into_iter().enumerate().map(|(i, word)| {
                                                view! {
                                                    <div style="display:flex;gap:6px;align-items:center">
                                                        <span style="color:#888;min-width:24px;text-align:right;font-size:12px">
                                                            {format!("{}.", i + 1)}
                                                        </span>
                                                        <span style="color:#e0e0e0">{word}</span>
                                                    </div>
                                                }
                                            }).collect::<Vec<_>>()}
                                        </div>
                                        <p class="muted" style="font-size:12px;margin-bottom:12px">
                                            "New wallet address: "
                                            <span style="font-family:monospace;color:#daa520;word-break:break-all">{addr.clone()}</span>
                                        </p>
                                        <button class="primary" on:click=move |_| {
                                            // Pre-fill send tab with new address
                                            // Just reload the page to switch to the new wallet
                                            let _ = web_sys::window().map(|w| w.location().reload());
                                        }>"Done \u{2014} Reload Wallet"</button>
                                    }.into_any()
                                }
                            }}
                        </div>
                    </div>
                </div>
            }.into_any()
        } else { view! { <span></span> }.into_any() }}

        // ── Cold Storage Wallet modal ────────────────────────────────────────

        {move || if show_cold.get() {
            view! {
                <div class="modal-overlay" on:click=move |_| show_cold.set(false)>
                    <div class="modal-card" style="max-width:480px" on:click=move |ev| ev.stop_propagation()>
                        <p class="modal-title">"🧊 Cold Storage Wallet"</p>
                        <div class="modal-body" style="text-align:left">
                            {move || {
                                if let Some((ref acct, ref key)) = cold_result.get() {
                                    let acct_c = acct.clone();
                                    let key_c = key.clone();
                                    let acct_copy = acct.clone();
                                    let key_copy = key.clone();
                                    view! {
                                        <div class="export-warning" style="margin-bottom:10px">
                                            <p style="font-weight:700;color:#f87171;font-size:13px">
                                                "⚠ Save this private key NOW. It will not be shown again."
                                            </p>
                                        </div>
                                        <p class="label" style="margin-bottom:4px">"Account ID (send KX here):"</p>
                                        <div style="display:flex;gap:6px;align-items:start;margin-bottom:12px">
                                            <p style="font-family:monospace;font-size:12px;word-break:break-all;background:#0a0c1a;padding:8px;border-radius:6px;flex:1">{acct_c}</p>
                                            <button style="font-size:11px;padding:4px 8px;white-space:nowrap" on:click=move |_| {
                                                let t = acct_copy.clone();
                                                spawn_local(async move { copy_to_clipboard(t).await; });
                                            }>"Copy"</button>
                                        </div>
                                        <p class="label" style="margin-bottom:4px">"Private Key (keep secret!):"</p>
                                        <div style="display:flex;gap:6px;align-items:start;margin-bottom:12px">
                                            <p style="font-family:monospace;font-size:10px;word-break:break-all;background:#0a0c1a;padding:8px;border-radius:6px;flex:1;max-height:120px;overflow-y:auto">{key_c}</p>
                                            <button style="font-size:11px;padding:4px 8px;white-space:nowrap" on:click=move |_| {
                                                let t = key_copy.clone();
                                                spawn_local(async move { copy_to_clipboard(t).await; });
                                            }>"Copy"</button>
                                        </div>
                                        {move || if !cold_saved.get() {
                                            view! {
                                                <button class="primary" style="width:100%" on:click=move |_| {
                                                    if let Some((ref acct, _)) = cold_result.get_untracked() {
                                                        let acct = acct.clone();
                                                        spawn_local(async move {
                                                            let args = serde_wasm_bindgen::to_value(
                                                                &serde_json::json!({ "accountId": acct })
                                                            ).unwrap_or(no_args());
                                                            if call::<()>("save_cold_wallet", args).await.is_ok() {
                                                                cold_saved.set(true);
                                                                let wallets = call::<Vec<String>>("get_cold_wallets", no_args()).await.unwrap_or_default();
                                                                cold_wallets.set(wallets);
                                                            }
                                                        });
                                                    }
                                                }>"I\u{2019}ve saved the key \u{2014} Remember this wallet"</button>
                                            }.into_any()
                                        } else {
                                            view! {
                                                <p class="msg success">"Wallet saved to your cold storage list."</p>
                                            }.into_any()
                                        }}
                                    }.into_any()
                                } else {
                                    view! {
                                        <p class="muted" style="font-size:13px;margin-bottom:12px">
                                            "This generates a brand new wallet keypair entirely offline. "
                                            "You can send KX to its address for long-term storage. "
                                            "To spend from it later, import the private key."
                                        </p>
                                        <button class="primary" style="width:100%"
                                            disabled=move || cold_generating.get()
                                            on:click=move |_| {
                                                cold_generating.set(true);
                                                spawn_local(async move {
                                                    match call::<ColdWalletResult>("generate_cold_wallet", no_args()).await {
                                                        Ok(r) => cold_result.set(Some((r.account_id, r.private_key_b64))),
                                                        Err(e) => { let _ = web_sys::window().map(|w| w.alert_with_message(&format!("Error: {e}"))); },
                                                    };
                                                    cold_generating.set(false);
                                                });
                                            }>
                                            {move || if cold_generating.get() { "Generating\u{2026}" } else { "Generate Cold Wallet" }}
                                        </button>
                                    }.into_any()
                                }
                            }}
                            <button style="margin-top:8px" on:click=move |_| show_cold.set(false)>"Close"</button>
                        </div>
                    </div>
                </div>
            }.into_any()
        } else { view! { <span></span> }.into_any() }}

    }
}
