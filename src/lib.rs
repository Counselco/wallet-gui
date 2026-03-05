use base64::Engine as _;
use js_sys::Promise;
use leptos::prelude::*;
use serde::Deserialize;
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::{spawn_local, JsFuture};
use std::collections::HashMap;

const LOGO_PNG: &[u8] = include_bytes!("../assets/chronx-logo.png");

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

// ── Trusted contact type (frontend) ──────────────────────────────────────────

#[derive(Clone, Deserialize, Default)]
struct TrustedContact {
    email: String,
    wallet: Option<String>,
    display_name: Option<String>,
    added_at: u64,
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
}

/// Returned by `create_email_timelock` — carries the on-chain TxId and
/// the "KX-XXXX-XXXX-XXXX-XXXX" claim code to email/display to the recipient.
#[derive(Clone, Deserialize, Default)]
struct EmailLockResult {
    tx_id: String,
    claim_code: String,
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

// ── Display helpers ───────────────────────────────────────────────────────────

fn shorten_addr(addr: &str) -> String {
    if addr.len() > 20 {
        format!("{}…{}", &addr[..8], &addr[addr.len()-8..])
    } else {
        addr.to_string()
    }
}

fn format_utc_ts(ts: i64) -> String {
    let d = js_sys::Date::new(&wasm_bindgen::JsValue::from_f64(ts as f64 * 1000.0));
    format!(
        "{:04}-{:02}-{:02} {:02}:{:02} UTC",
        d.get_utc_full_year(),
        d.get_utc_month() + 1,
        d.get_utc_date(),
        d.get_utc_hours(),
        d.get_utc_minutes()
    )
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

/// Parse "YYYY-MM-DD" or "YYYY-MM-DDTHH:MM" as UTC Unix seconds.
fn date_str_to_unix(s: &str) -> Option<i64> {
    let utc_str = if s.len() == 10 {
        format!("{s}T00:00:00Z")
    } else if s.len() >= 16 {
        format!("{}:00Z", &s[..16])
    } else {
        return None;
    };
    let d = js_sys::Date::new(&wasm_bindgen::JsValue::from_str(&utc_str));
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

async fn delay_ms(ms: u32) {
    let promise = Promise::new(&mut |resolve, _| {
        if let Some(win) = web_sys::window() {
            let _ = win.set_timeout_with_callback_and_timeout_and_arguments_0(&resolve, ms as i32);
        }
    });
    let _ = JsFuture::from(promise).await;
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
    // Mobile: 0=Receive 1=Send 2=Promises 3=Settings
    // Desktop: 0=Receive 1=Send 2=Promises 3=Request 4=History 5=Settings
    let active_tab  = RwSignal::new(0u8);
    let app_version = RwSignal::new("1.0.0".to_string());
    let desktop     = is_desktop();

    // Language signal
    let lang = RwSignal::new("en".to_string());

    // Cascade send mode (desktop only): 0=Simple, 1=Cascade
    let send_cascade_mode = RwSignal::new(0u8);

    // Welcome / backup / restore state
    let welcome_busy  = RwSignal::new(false);
    let welcome_msg   = RwSignal::new(String::new());
    let backup_key_str = RwSignal::new(String::new());
    let backup_copied  = RwSignal::new(false);
    let restore_input  = RwSignal::new(String::new());
    let restore_msg    = RwSignal::new(String::new());
    let restore_busy   = RwSignal::new(false);

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

    // ── Auto-refresh balance every 10 seconds (silent, no spinner) ──────────
    {
        let cb = wasm_bindgen::closure::Closure::<dyn Fn()>::new(move || {
            spawn_local(async move {
                // Silent refresh — don't touch loading signal
                if let Ok(a) = call::<AccountInfo>("get_account_info", no_args()).await {
                    info.set(Some(a));
                }
            });
        });
        let _ = web_sys::window().unwrap().set_interval_with_callback_and_timeout_and_arguments_0(
            cb.as_ref().unchecked_ref(), 10_000
        );
        cb.forget(); // leak closure — lives for app lifetime
    }

    // ── Wallet creation (first run) ───────────────────────────────────────────

    let on_generate = move |_: web_sys::MouseEvent| {
        spawn_local(async move {
            welcome_busy.set(true);
            welcome_msg.set(String::new());
            match call::<String>("generate_wallet", no_args()).await {
                Ok(_account_id) => {
                    // Fetch backup key and show it before PIN setup
                    match call::<String>("export_secret_key", no_args()).await {
                        Ok(key) => {
                            backup_key_str.set(key);
                            backup_copied.set(false);
                            app_phase.set(AppPhase::BackupKey);
                        }
                        Err(e) => {
                            // Backup key fetch failed — still proceed to PIN setup
                            welcome_msg.set(format!("Warning: could not export backup key: {e}"));
                            app_phase.set(AppPhase::PinSetup);
                        }
                    }
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
                                // Check for pending deep link (chronx://claim?code=...)
                                if let Ok(Some(code)) = call::<Option<String>>("get_pending_deep_link", no_args()).await {
                                    deep_link_code.set(code);
                                    active_tab.set(0); // Navigate to Account/Receive tab (claim code is there)
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
                            // Check for pending deep link (chronx://claim?code=...)
                            if let Ok(Some(code)) = call::<Option<String>>("get_pending_deep_link", no_args()).await {
                                deep_link_code.set(code);
                                active_tab.set(1); // Navigate to Promises Made tab
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
                    copied=backup_copied
                    on_copy=move |_: web_sys::MouseEvent| {
                        let key = backup_key_str.get_untracked();
                        spawn_local(async move {
                            copy_to_clipboard(key).await;
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
                            restore_msg.set("Please paste your backup key.".to_string());
                            return;
                        }
                        spawn_local(async move {
                            restore_busy.set(true);
                            restore_msg.set(String::new());
                            let args = serde_wasm_bindgen::to_value(
                                &serde_json::json!({ "backupKey": key.trim() })
                            ).unwrap_or(no_args());
                            match call::<String>("restore_wallet", args).await {
                                Ok(_account_id) => {
                                    pin_digits.set(String::new());
                                    pin_msg.set(String::new());
                                    app_phase.set(AppPhase::PinSetup);
                                }
                                Err(e) => restore_msg.set(format!("Error: {e}")),
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
                />
            }.into_any(),

            AppPhase::Wallet => view! {
                <div class="app">
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

                    // Header
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
                    // Tab bar — mobile: 3 tabs + settings; desktop: 5 tabs + settings
                    <nav class="tab-bar">
                        <button class=move || if active_tab.get()==0 {"tab active"} else {"tab"}
                            on:click=move |_| active_tab.set(0)>
                            {move || t(&lang.get(), "tab_receive")}
                        </button>
                        <button class=move || if active_tab.get()==1 {"tab active"} else {"tab"}
                            on:click=move |_| active_tab.set(1)>
                            {move || t(&lang.get(), "tab_send")}
                        </button>
                        <button class=move || if active_tab.get()==2 {"tab active"} else {"tab"}
                            on:click=move |_| active_tab.set(2)>
                            {move || t(&lang.get(), "tab_promises")}
                        </button>
                        {if desktop {
                            view! {
                                <button class=move || if active_tab.get()==3 {"tab active"} else {"tab"}
                                    on:click=move |_| active_tab.set(3)>
                                    {move || t(&lang.get(), "tab_request")}
                                </button>
                                <button class=move || if active_tab.get()==4 {"tab active"} else {"tab"}
                                    on:click=move |_| active_tab.set(4)>
                                    {move || t(&lang.get(), "tab_history")}
                                </button>
                            }.into_any()
                        } else {
                            view! { <span></span> }.into_any()
                        }}
                        <button class=move || {
                            let settings_tab = if desktop { 5 } else { 3 };
                            if active_tab.get()==settings_tab {"tab active"} else {"tab"}
                        }
                            on:click=move |_| {
                                let settings_tab = if desktop { 5 } else { 3 };
                                active_tab.set(settings_tab);
                            }>
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

                    // Main content routing
                    <div>
                        {move || {
                            let tab = active_tab.get();
                            let settings_tab: u8 = if desktop { 5 } else { 3 };
                            match tab {
                                // Tab 0: Receive (was part of AccountPanel)
                                0 => view! {
                                    <AccountPanel info=info loading=loading err_msg=err_msg on_refresh=on_refresh pending_email_chronos=pending_email_chronos active_tab=active_tab deep_link_code=deep_link_code lang=lang />
                                }.into_any(),
                                // Tab 1: Send (Simple or Cascade on desktop)
                                1 => view! {
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
                                    {move || if send_cascade_mode.get() == 0 {
                                        view! { <SendPanel info=info pending_email_chronos=pending_email_chronos lang=lang /> }.into_any()
                                    } else {
                                        view! { <CascadeSendPanel info=info pending_email_chronos=pending_email_chronos lang=lang /> }.into_any()
                                    }}
                                }.into_any(),
                                // Tab 2: Promises (incoming only — node auto-delivers)
                                2 => view! {
                                    <PromisesPanel info=info lang=lang />
                                }.into_any(),
                                // Tab 3: Request (desktop only) OR Settings (mobile)
                                3 if desktop => view! {
                                    <RequestPanel info=info lang=lang />
                                }.into_any(),
                                // Tab 4: History (desktop only)
                                4 if desktop => view! {
                                    <HistoryPanel info=info email_locks=email_locks on_email_check=check_email />
                                }.into_any(),
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

                    // Version footer — always visible
                    <p class="version-footer">
                        "ChronX Wallet v"
                        {move || app_version.get()}
                    </p>
                    <div class="bug-footer">
                        <button class="bug-report-btn" on:click=move |_| {
                            bug_body.set(String::new());
                            bug_modal_open.set(true);
                        }>"🐞 Report a Bug"</button>
                    </div>

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
                                    <p class="modal-title">"🐞 Report a Bug"</p>
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

    view! {
        <div class="app">
            <div style="text-align:center;padding:20px 0 8px">
                <img src=logo_src() alt="ChronX" style="height:44px;width:auto;display:inline-block" />
            </div>

            <div class="pin-screen">
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
                    } else if !msg.is_empty() {
                        view! { <p class="pin-msg">{msg}</p> }.into_any()
                    } else {
                        view! { <span></span> }.into_any()
                    }
                }}

                <p class="version-footer" style="margin-top:auto;padding-top:12px;opacity:0.4;font-size:11px">
                    "ChronX Wallet v1.4.18"
                </p>
            </div>
        </div>
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
                <div style="display:flex;gap:8px">
                    <button on:click=on_copy style="flex:1">
                        {move || if copied.get() { "\u{2713} Copied!" } else { "\u{1f4cb} Copy to Clipboard" }}
                    </button>
                </div>
                <button class="primary" on:click=on_confirm style="margin-top:8px">
                    "I've saved my backup key \u{2192}"
                </button>
                <p class="muted" style="font-size:11px;text-align:center">
                    "Store it in a password manager or secure offline location."
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
                <p class="label">"Paste your ChronX wallet backup key below:"</p>
                <textarea
                    class="restore-textarea"
                    rows="5"
                    placeholder="Paste your backup key here\u{2026}"
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
    deep_link_code: RwSignal<String>,
    lang: RwSignal<String>,
) -> impl IntoView {
    let copy_success = RwSignal::new(false);
    let incoming     = RwSignal::new(Vec::<TimeLockInfo>::new());
    let inc_loading  = RwSignal::new(false);
    let qr_svg       = RwSignal::new(String::new());
    let qr_visible   = RwSignal::new(false);

    // Claim code on Receive tab (pre-fill from deep link)
    let dl_code = deep_link_code.get_untracked();
    let home_claim_code = RwSignal::new(dl_code.clone());
    if !dl_code.is_empty() {
        deep_link_code.set(String::new()); // consume it
    }
    let home_claim_msg  = RwSignal::new(String::new());
    let home_claim_busy = RwSignal::new(false);

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
                // ── Balance + Refresh ────────────────────────────────────────
                <div class="row">
                    <div>
                        <p class="label">"Balance"</p>
                        <p class="balance">
                            {move || {
                                if loading.get() { "\u{2026}".into() }
                                else {
                                    info.get()
                                        .map(|a| format!("{} KX", format_kx(&a.balance_chronos)))
                                        .unwrap_or_else(|| "\u{2014}".into())
                                }
                            }}
                        </p>
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
                            <a class="exchange-link" href="#" on:click=move |ev| {
                                ev.prevent_default();
                                spawn_local(async move {
                                    let args = serde_wasm_bindgen::to_value(
                                        &serde_json::json!({ "url": "https://chronx.io/exchange.html" })
                                    ).unwrap_or(no_args());
                                    let _ = call::<()>("open_url", args).await;
                                });
                            }>"Convert KX \u{2194} USDC"</a>
                        </p>
                    </div>
                    <button on:click=on_refresh disabled=move || loading.get()>
                        {move || if loading.get() { "\u{2026}" } else { "\u{21bb} Refresh" }}
                    </button>
                </div>

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

                // ── Section B: QR Code (inline toggle) ──────────────────────
                <div>
                    <button style="font-size:13px;padding:8px 16px;border:1px solid #d4a84b;background:transparent;color:#d4a84b;border-radius:6px;cursor:pointer;width:100%"
                        on:click=on_toggle_qr>
                        {move || if qr_visible.get() { "Hide QR Code" } else { "Show QR Code" }}
                    </button>
                    {move || if qr_visible.get() {
                        let svg = qr_svg.get();
                        view! {
                            <div class="modal-overlay" style="position:fixed;top:0;left:0;right:0;bottom:0;z-index:1000;display:flex;align-items:center;justify-content:center"
                                on:click=move |_| qr_visible.set(false)>
                                <div style="background:#fff;border-radius:12px;padding:24px;text-align:center;max-width:320px" on:click=move |ev| ev.stop_propagation()>
                                    <div inner_html=svg style="display:inline-block"></div>
                                    <p style="color:#555;font-size:11px;margin-top:8px">
                                        "Others scan this to send KX to you"
                                    </p>
                                    <button style="margin-top:12px;padding:8px 24px;background:#d4a84b;color:#0a0a0a;border:none;border-radius:6px;cursor:pointer;font-weight:700"
                                        on:click=move |_| qr_visible.set(false)>{t(&lang.get(), "close")}</button>
                                </div>
                            </div>
                        }.into_any()
                    } else {
                        view! { <span></span> }.into_any()
                    }}
                </div>

                <hr style="border:none;border-top:1px solid #1e2130;margin:14px 0" />

                // ── Claim Code ───────────────────────────────────────────────
                <div style="border:1px solid rgba(212,168,75,0.3);border-radius:8px;padding:12px;margin-top:0">
                    <p style="font-size:15px;font-weight:700;color:#d4a84b;margin:0 0 6px">"Got a claim code?"</p>
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
                            spawn_local(async move {
                                home_claim_msg.set("Searching for matching locks\u{2026}".into());
                                let args = serde_wasm_bindgen::to_value(
                                    &serde_json::json!({ "claimCode": code })
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
                </div>

                // ── Incoming promise link at bottom ──────────────────────────
                {move || {
                    let count = incoming.get().len();
                    if count > 0 {
                        view! {
                            <p style="margin-top:14px;font-size:13px;color:#9ca3af;text-align:center">
                                {format!("You have {} incoming promise{}.", count, if count == 1 { "" } else { "s" })}
                                " "
                                <a href="#" style="color:#d4a84b;text-decoration:underline;cursor:pointer" on:click=move |ev| {
                                    ev.prevent_default();
                                    active_tab.set(2); // navigate to Promises
                                }>{t(&lang.get(), "view_promises")}</a>
                            </p>
                        }.into_any()
                    } else {
                        view! { <span></span> }.into_any()
                    }
                }}
            </div>

        </div>

        // Send is now a standalone tab — removed from AccountPanel
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
fn SendPanel(info: RwSignal<Option<AccountInfo>>, pending_email_chronos: RwSignal<u64>, lang: RwSignal<String>) -> impl IntoView {
    let send_sub  = RwSignal::new(0u8); // 0=KX Address, 1=Email Address
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
    let sending   = RwSignal::new(false);
    let msg       = RwSignal::new(String::new());
    let scan_msg  = RwSignal::new(String::new());
    let spam_warn = RwSignal::new(false);

    // Series entries for Email + Send Later (additional payments beyond the first)
    let series_entries: RwSignal<Vec<(RwSignal<String>, RwSignal<String>, RwSignal<String>)>> = RwSignal::new(Vec::new()); // (amount, date, memo)

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

    // Clear messages on tab/mode switch
    Effect::new(move |_| {
        send_sub.get(); send_mode.get();
        msg.set(String::new()); scan_msg.set(String::new()); spam_warn.set(false);
    });

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
                    Ok(txid) => {
                        msg.set(format!("Sent! TxId: {}", &txid[..16.min(txid.len())]));
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
            // KX + Send Later
            let date_str = lock_date.get_untracked();
            if date_str.is_empty() { msg.set("Error: choose an unlock date.".into()); return; }
            let unlock_unix = match date_str_to_unix(&date_str) {
                Some(t) => t,
                None => { msg.set("Error: invalid date.".into()); return; }
            };
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
                })).unwrap_or(no_args());
                match call::<String>("create_timelock", args).await {
                    Ok(txid) => {
                        msg.set(format!("Promise made! ID: {}", &txid[..16.min(txid.len())]));
                        amount.set(String::new());
                        lock_date.set(String::new());
                        memo.set(String::new());
                        to_pubkey.set(String::new());
                        let prev_nonce = info.get_untracked().as_ref().map(|a| a.nonce).unwrap_or(0);
                        for _ in 0..15u8 {
                            delay_ms(1000).await;
                            if let Ok(a) = call::<AccountInfo>("get_account_info", no_args()).await {
                                if a.nonce > prev_nonce { info.set(Some(a)); break; }
                            }
                        }
                        // Always force a final refresh so balance is correct even if nonce poll timed out
                        if let Ok(a) = call::<AccountInfo>("get_account_info", no_args()).await {
                            info.set(Some(a));
                        }
                    }
                    Err(e) => msg.set(format!("Error: {e}")),
                }
                sending.set(false);
            });
        } else if sub == 1 && mode == 0 {
            // Email + Send Now (unlock = 0 → backend uses now → immediately claimable)
            let email_str = email.get_untracked();
            if !is_valid_email(&email_str) {
                msg.set("Error: Please enter a valid email address.".into()); return;
            }
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
                        email.set(String::new());
                        amount.set(String::new());
                        memo.set(String::new());
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
            let unlock_unix = match date_str_to_unix(&date_str) {
                Some(t) => t,
                None => { msg.set("Error: invalid date.".into()); return; }
            };

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
                            email.set(String::new());
                            amount.set(String::new());
                            lock_date.set(String::new());
                            memo.set(String::new());
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
                                Ok(_) => { msg.set(format!("Series sent! {count} promises created. Claim code: {claim_code}")); spam_warn.set(true); }
                                Err(_) => { msg.set(format!("Series on-chain! Email failed \u{2014} claim code: {claim_code}")); }
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
                spawn_local(async move {
                    sending.set(true);
                    pending_email_chronos.set((amt * 1_000_000.0) as u64);
                    msg.set("Mining PoW\u{2026} (~10s)".into());
                    let args = serde_wasm_bindgen::to_value(&serde_json::json!({
                        "email": email_str.clone(),
                        "amountKx": amt,
                        "unlockAtUnix": unlock_unix,
                        "memo": memo_opt.clone(),
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
                            email.set(String::new());
                            amount.set(String::new());
                            lock_date.set(String::new());
                            memo.set(String::new());
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

    view! {
        <div class="card">

            // Sub-tabs: KX Address | Email Address
            <div class="send-subtabs">
                <button type="button"
                    class=move || if send_sub.get()==0 { "send-subtab active" } else { "send-subtab" }
                    on:click=move |_| { send_sub.set(0); lock_date.set(String::new()); }
                    disabled=move || sending.get()>"KX Address"</button>
                <button type="button"
                    class=move || if send_sub.get()==1 { "send-subtab active" } else { "send-subtab" }
                    on:click=move |_| { send_sub.set(1); lock_date.set(String::new()); }
                    disabled=move || sending.get()>"Email Address"</button>
            </div>

            // Mode: Send Now | Send Later BETA
            <div class="send-mode-row">
                <button type="button"
                    class=move || if send_mode.get()==0 { "send-mode-btn active" } else { "send-mode-btn" }
                    on:click=move |_| { send_mode.set(0); lock_date.set(String::new()); }
                    disabled=move || sending.get()>"Send Now"</button>
                <button type="button"
                    class=move || if send_mode.get()==1 { "send-mode-btn active" } else { "send-mode-btn" }
                    on:click=move |_| send_mode.set(1)
                    disabled=move || sending.get()>"\u{23f3} Send Later BETA"</button>
            </div>

            // KX or Email address field
            {move || if send_sub.get() == 0 {
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
            } else {
                view! {
                    <div class="field">
                        <label>"Recipient Email Address"</label>
                        <input type="email" placeholder="recipient@example.com"
                            prop:value=move || email.get()
                            on:input=move |ev| email.set(event_target_value(&ev))
                            disabled=move || sending.get() />
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
                    </div>
                }.into_any()
            }}

            // Amount
            <div class="field">
                <label>"Amount (KX)"</label>
                <input type="text" inputmode="decimal" placeholder="0.000000"
                    prop:value=move || amount.get()
                    on:input=move |ev| {
                        let raw = event_target_value(&ev);
                        // Allow digits, at most one decimal point, max 6 decimal places
                        let filtered: String = {
                            let mut has_dot = false;
                            let mut decimals = 0u8;
                            raw.chars().filter(|&c| {
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

            // Datetime picker — Send Later only
            {move || if send_mode.get() == 1 {
                view! {
                    <div class="field">
                        <div class="utc-clock">
                            "\u{1f550} Current UTC time: " {move || utc_clock.get()}
                        </div>
                        <label>"Unlock Date \u{0026} Time (UTC)"</label>
                        <input type="datetime-local"
                            prop:min=move || min_datetime_str(86400)
                            prop:value=move || lock_date.get()
                            on:input=move |ev| lock_date.set(event_target_value(&ev))
                            disabled=move || sending.get() />
                        {move || {
                            let dt_str = lock_date.get();
                            if dt_str.is_empty() { return view! { <span></span> }.into_any(); }
                            let unix = match date_str_to_unix(&dt_str) {
                                Some(t) => t,
                                None => return view! { <span></span> }.into_any(),
                            };
                            let now_secs = (js_sys::Date::now() / 1000.0) as i64;
                            let diff = unix - now_secs;
                            if diff <= 0 {
                                return view! { <p class="msg error" style="margin-top:4px">"Date must be in the future"</p> }.into_any();
                            }
                            let days  = diff / 86400;
                            let hours = (diff % 86400) / 3600;
                            let text  = if days > 0 {
                                format!("Unlocks in {days} days, {hours} hours from now (UTC)")
                            } else {
                                let mins = (diff % 3600) / 60;
                                format!("Unlocks in {hours} hours, {mins} minutes from now (UTC)")
                            };
                            view! { <p class="unlock-countdown">{text}</p> }.into_any()
                        }}
                        <div class="quick-dates">
                            <button type="button" class="pill"
                                on:click=move |_| { let d=datetime_plus_months(1); set_date(d); }
                                disabled=move || sending.get()>"1 mo"</button>
                            <button type="button" class="pill"
                                on:click=move |_| { let d=datetime_plus_years(1); set_date(d); }
                                disabled=move || sending.get()>"1 yr"</button>
                            <button type="button" class="pill"
                                on:click=move |_| { let d=datetime_plus_years(5); set_date(d); }
                                disabled=move || sending.get()>"5 yr"</button>
                            <button type="button" class="pill"
                                on:click=move |_| { let d=datetime_plus_years(10); set_date(d); }
                                disabled=move || sending.get()>"10 yr"</button>
                            <button type="button" class="pill"
                                on:click=move |_| { let d=datetime_plus_years(25); set_date(d); }
                                disabled=move || sending.get()>"25 yr"</button>
                            <button type="button" class="pill"
                                on:click=move |_| { let d=datetime_plus_years(100); set_date(d); }
                                disabled=move || sending.get()>"100 yr"</button>
                        </div>
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
                    </div>
                }.into_any()
            } else { view! { <span></span> }.into_any() }}

            // Email info box
            {move || if send_sub.get() == 1 {
                let txt = if send_mode.get() == 0 {
                    "The recipient has 72 hours to accept. If not accepted, your KX is automatically returned."
                } else {
                    "The recipient will receive an email and has 72 hours to accept. \
                     If not accepted, your KX is automatically returned. \
                     You can cancel from History at any time."
                };
                view! {
                    <div style="background:#1a1d27;border:1px solid #2a2d37;border-radius:8px;padding:10px 12px;margin-bottom:8px">
                        <p style="font-size:12px;color:#9ca3af;line-height:1.5;margin:0">{txt}</p>
                    </div>
                }.into_any()
            } else { view! { <span></span> }.into_any() }}

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
                        <button class="pill" style="width:100%;margin:8px 0;color:#d4a84b;border-color:#d4a84b"
                            disabled=move || sending.get() || (series_entries.get().len() >= 9)
                            on:click=move |_| {
                                series_entries.update(|v| {
                                    v.push((RwSignal::new(String::new()), RwSignal::new(String::new()), RwSignal::new(String::new())));
                                });
                            }>
                            "+ Add Another Payment"
                        </button>
                    </div>
                }.into_any()
            }}

            // Send Later warning
            {move || if send_mode.get() == 1 {
                view! {
                    <p class="lock-warning">
                        "\u{26a0} Promised funds cannot be recovered before the unlock date. "
                        "This action is permanent and cannot be undone."
                    </p>
                }.into_any()
            } else { view! { <span></span> }.into_any() }}

            // Submit button
            <button class=move || if send_mode.get()==1 { "primary danger" } else { "primary" }
                on:click=on_send disabled=move || sending.get()>
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
        </div>
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
    match s.unlock_mode.get_untracked() {
        0 => "Immediately".to_string(),
        1 => {
            let v = s.dur_value.get_untracked();
            let u = s.dur_unit.get_untracked();
            if v.is_empty() { return "—".to_string(); }
            format!("{v} {u}")
        }
        2 => {
            let d = s.date.get_untracked();
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
    let stages: RwSignal<Vec<CascadeStage>> = RwSignal::new(vec![make_stage()]);
    let sending = RwSignal::new(false);
    let msg = RwSignal::new(String::new());
    let spam_warn = RwSignal::new(false);
    let confirm_open = RwSignal::new(false);

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
                        Ok(_) => { msg.set(format!("Cascade sent! {count} stages created.\nClaim code: {claim_code}")); spam_warn.set(true); }
                        Err(_) => { msg.set(format!("Cascade on-chain! Email failed.\nClaim code: {claim_code}")); }
                    }
                    email.set(String::new());
                    memo.set(String::new());
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
                    // Email
                    <div class="field">
                        <label>"Recipient Email"</label>
                        <input type="email" placeholder="recipient@example.com"
                            prop:value=move || email.get()
                            on:input=move |ev| email.set(event_target_value(&ev))
                            disabled=move || sending.get() />
                    </div>
                    // Memo
                    <div class="field">
                        <label>{move || t(&lang.get(), "memo_optional")}</label>
                        <input type="text" maxlength="256" placeholder="e.g. Welcome to ChronX"
                            prop:value=move || memo.get()
                            on:input=move |ev| memo.set(event_target_value(&ev))
                            disabled=move || sending.get() />
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
                                        <input type="number" step="0.01" min="0.01"
                                            placeholder="KX"
                                            prop:value=move || s.amount.get()
                                            on:input=move |ev| s.amount.set(event_target_value(&ev))
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
                    // Send button
                    <button class="primary" style="width:100%"
                        on:click=on_send disabled=move || sending.get()>
                        {move || if sending.get() { "Sending\u{2026}" } else { "Send Cascade" }}
                    </button>
                    <p class="fee-free-line">{"\u{2713} No transaction fees. The recipient receives exactly what you send."}</p>
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
                                "Ask your recipient to check their spam folder."
                            </p>
                        }.into_any()
                    } else { view! { <span></span> }.into_any() }}
                </div>
                // Live preview
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
                            format!("{sum:.6} KX")
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
                                let amt = s.amount.get();
                                let amt_display = if amt.is_empty() { "?".to_string() } else { format!("{amt} KX") };
                                let date_display = stage_display_date(&s);
                                view! {
                                    <div class="stage-line">
                                        <span>{amt_display}</span>
                                        <span class="stage-date">{format!("\u{2192} {date_display}")}</span>
                                    </div>
                                }
                            }).collect_view()
                        }}
                    </div>
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
                        <p>{format!("Send {total:.6} KX to {em} in {count} stages?")}</p>
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

// ── PromisesPanel (hidden — merged into HistoryPanel in v1.4.25) ──────────────

#[component]
fn PromisesPanel(
    info: RwSignal<Option<AccountInfo>>,
    lang: RwSignal<String>,
) -> impl IntoView {
    let incoming = RwSignal::new(Vec::<TimeLockInfo>::new());
    let loading = RwSignal::new(false);

    let reload = move || {
        spawn_local(async move {
            loading.set(true);
            if let Ok(locks) = call::<Vec<TimeLockInfo>>("get_pending_incoming", no_args()).await {
                incoming.set(locks);
            }
            loading.set(false);
        });
    };

    Effect::new(move |_| { reload(); });

    // Auto-refresh every 30 seconds to catch auto-delivered promises
    Effect::new(move |_| {
        spawn_local(async move {
            loop {
                delay_ms(30_000).await;
                if let Ok(locks) = call::<Vec<TimeLockInfo>>("get_pending_incoming", no_args()).await {
                    incoming.set(locks);
                }
            }
        });
    });

    view! {
        <div class="card">
            <h3 class="section-title">{move || t(&lang.get(), "promises_header")}</h3>
            <p class="muted" style="font-size:13px;margin-bottom:16px">
                {move || t(&lang.get(), "promises_sub")}
            </p>
            {move || {
                let locks = incoming.get();
                if loading.get() {
                    view! { <p class="muted">"\u{2026}"</p> }.into_any()
                } else if locks.is_empty() {
                    view! {
                        <div style="text-align:center;padding:32px 16px">
                            <div style="font-size:48px;margin-bottom:12px;opacity:0.4">{"\u{1f4e6}"}</div>
                            <p style="font-size:16px;font-weight:600;color:#e0e0e0;margin-bottom:6px">
                                {move || t(&lang.get(), "promises_empty_title")}
                            </p>
                            <p class="muted" style="font-size:13px">
                                {move || t(&lang.get(), "promises_empty_sub")}
                            </p>
                        </div>
                    }.into_any()
                } else {
                    view! {
                        <div class="timelock-list">
                            {locks.into_iter().map(|lock| {
                                let now = (js_sys::Date::now() / 1000.0) as i64;
                                let sender_label = shorten_addr(&lock.sender);
                                let status_label = {
                                    let diff = lock.unlock_at - now;
                                    if diff <= 0 {
                                        t(&lang.get(), "arriving_soon")
                                    } else if diff < 3600 {
                                        let mins = diff / 60;
                                        format!("{} {}m", t(&lang.get(), "unlocks_in"), mins.max(1))
                                    } else if diff < 86400 {
                                        let hours = diff / 3600;
                                        let mins = (diff % 3600) / 60;
                                        format!("{} {}h {}m", t(&lang.get(), "unlocks_in"), hours, mins)
                                    } else {
                                        let days = diff / 86400;
                                        format!("{} {}d", t(&lang.get(), "unlocks_in"), days)
                                    }
                                };
                                let amount_str = format!("{} KX", format_kx(&lock.amount_chronos));
                                let memo_text = lock.memo.clone().unwrap_or_default();
                                view! {
                                    <div class="timelock-item">
                                        <div class="tl-row">
                                            <span class="tl-amount">{amount_str}</span>
                                            <span class="tl-status" style="color:#DAA520">{status_label}</span>
                                        </div>
                                        <div class="tl-row">
                                            <span class="tl-label">{format!("From: {}", sender_label)}</span>
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
    let contacts = RwSignal::new(Vec::<TrustedContact>::new());
    let req_email = RwSignal::new(String::new());
    let req_amount = RwSignal::new(String::new());
    let req_note = RwSignal::new(String::new());
    let req_msg = RwSignal::new(String::new());
    let req_busy = RwSignal::new(false);

    Effect::new(move |_| {
        spawn_local(async move {
            if let Ok(c) = call::<Vec<TrustedContact>>("get_trusted_contacts", no_args()).await {
                contacts.set(c);
            }
        });
    });

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
        let wallet = info.get_untracked().map(|a| a.account_id).unwrap_or_default();
        req_busy.set(true);
        req_msg.set(String::new());
        spawn_local(async move {
            let args = serde_wasm_bindgen::to_value(&serde_json::json!({
                "fromWallet": wallet,
                "fromEmail": "",
                "toEmail": email,
                "amountKx": amount,
                "note": note,
            })).unwrap_or(no_args());
            match call::<serde_json::Value>("send_poke_request", args).await {
                Ok(_) => {
                    req_msg.set("Request sent!".to_string());
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
        <div class="card">
            <h3 class="section-title">{move || t(&lang.get(), "request_money")}</h3>
            <div class="form-group">
                <label>"Email"</label>
                <input type="email" class="input" placeholder="recipient@email.com"
                    prop:value=move || req_email.get()
                    on:input=move |ev| {
                        use wasm_bindgen::JsCast;
                        let val = ev.target().and_then(|t| t.dyn_into::<web_sys::HtmlInputElement>().ok()).map(|i| i.value()).unwrap_or_default();
                        req_email.set(val);
                    }
                />
            </div>
            <div class="form-group">
                <label>{move || t(&lang.get(), "amount_kx")}</label>
                <input type="number" class="input" step="0.01" min="0.01"
                    prop:value=move || req_amount.get()
                    on:input=move |ev| {
                        use wasm_bindgen::JsCast;
                        let val = ev.target().and_then(|t| t.dyn_into::<web_sys::HtmlInputElement>().ok()).map(|i| i.value()).unwrap_or_default();
                        req_amount.set(val);
                    }
                />
            </div>
            <div class="form-group">
                <label>{move || t(&lang.get(), "memo_optional")}</label>
                <input type="text" class="input" maxlength="256"
                    prop:value=move || req_note.get()
                    on:input=move |ev| {
                        use wasm_bindgen::JsCast;
                        let val = ev.target().and_then(|t| t.dyn_into::<web_sys::HtmlInputElement>().ok()).map(|i| i.value()).unwrap_or_default();
                        req_note.set(val);
                    }
                />
            </div>
            <button class="btn gold" on:click=on_send_request disabled=move || req_busy.get()>
                {move || t(&lang.get(), "send_request")}
            </button>
            {move || {
                let s = req_msg.get();
                if s.is_empty() { view! { <span></span> }.into_any() }
                else {
                    let cls = if s.starts_with("Error") { "msg error" } else { "msg success" };
                    view! { <p class=cls>{s}</p> }.into_any()
                }
            }}
        </div>

        // Trusted Contacts
        <div class="card" style="margin-top:16px">
            <h3 class="section-title">{move || t(&lang.get(), "trusted_contacts")}</h3>
            {move || {
                let list = contacts.get();
                if list.is_empty() {
                    view! { <p class="muted">{move || t(&lang.get(), "no_trusted")}</p> }.into_any()
                } else {
                    view! {
                        <div class="timelock-list">
                            {list.into_iter().map(|c| {
                                let email = c.email.clone();
                                let email_for_remove = email.clone();
                                view! {
                                    <div class="timelock-item" style="display:flex;justify-content:space-between;align-items:center">
                                        <span>{email}</span>
                                        <button class="btn-outline small" on:click=move |_| {
                                            let e = email_for_remove.clone();
                                            spawn_local(async move {
                                                let args = serde_wasm_bindgen::to_value(&serde_json::json!({ "email": e }))
                                                    .unwrap_or(no_args());
                                                let _ = call::<()>("remove_trusted_contact", args).await;
                                            });
                                        }>{move || t(&lang.get(), "remove")}</button>
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
                let inc_locks = incoming.get();
                for lock in &inc_locks {
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
                        claim_secret_hash: lock.claim_secret_hash.clone(),
                    });
                }

                // Build cascade maps: which claim_secret_hash groups have any claimed lock?
                let mut cascade_claimed: std::collections::HashMap<String, bool> = std::collections::HashMap::new();
                let mut cascade_lock_ids: std::collections::HashMap<String, Vec<String>> = std::collections::HashMap::new();
                for e in &list {
                    if let Some(ref hash) = e.claim_secret_hash {
                        cascade_lock_ids.entry(hash.clone()).or_default().push(e.tx_id.clone());
                        if e.status == "Claimed" || e.status.contains("Reverted") {
                            cascade_claimed.insert(hash.clone(), true);
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
                            {page_list.into_iter().map(|entry| {
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
                                let type_label = match entry.tx_type.as_str() {
                                    "Transfer Sent" => "SENT",
                                    "Transfer Received" => "RECEIVED",
                                    "Email Send" => "SENT",
                                    "Email Claimed" => "RECEIVED",
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
                                        if unlock_ts <= now {
                                            format!("{} \u{b7} Ready to claim", from)
                                        } else {
                                            format!("{} \u{b7} Unlocks {}", from, unix_to_date_str(unlock_ts))
                                        }
                                    } else { from }
                                } else if is_incoming {
                                    // Show "From: <shortened account>" for incoming entries
                                    entry.counterparty.as_deref()
                                        .map(|a| format!("From {}", shorten_addr(a)))
                                        .unwrap_or_default()
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

                                // Determine if this entry can be cancelled
                                let can_cancel_base = (entry.status == "Pending" || entry.status == "Pending Claim")
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
                                } else {
                                    entry.status.clone()
                                };

                                let cancel_lock_id = entry.tx_id.clone();
                                let inline_cancel_id = cancel_lock_id.clone();
                                let entry_claim_code = entry.claim_code.clone();

                                view! {
                                    <div class="history-row"
                                        on:click=move |_| {
                                            let current = expanded.get_untracked();
                                            if current.as_deref() == Some(&tx_id_for_toggle) {
                                                expanded.set(None);
                                            } else {
                                                expanded.set(Some(tx_id_for_toggle.clone()));
                                            }
                                        }>
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
                                            <span class="history-addr">{addr_display}</span>
                                            <span class="history-date">{date_display}</span>
                                        </div>
                                        // Email send status badge + inline Cancel/Reclaim for email sends
                                        {if is_email_send {
                                            let badge_class = match entry_status.as_str() {
                                                "Pending Claim" => "email-badge pending-claim",
                                                "Claimed"       => "email-badge claimed",
                                                "Expired \u{2014} Reclaiming" => "email-badge reclaiming",
                                                _               => "email-badge expired",
                                            };
                                            let badge_text = entry_status.clone();
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
                                                    } else if can_cancel {
                                                        view! {
                                                            <button class="cancel-btn" style="margin-top:0;font-size:11px;padding:2px 10px"
                                                                on:click={move |ev: web_sys::MouseEvent| {
                                                                    ev.stop_propagation();
                                                                    cancel_msg.set(String::new());
                                                                    cancel_is_email.set(true);
                                                                    cancel_cascade_ids.set(cancel_cascade_for_click.clone());
                                                                    cancel_target.set(Some(inline_cancel_id.clone()));
                                                                }}>
                                                                {if is_cascade { "Cancel Series" } else { "Cancel" }}
                                                            </button>
                                                        }.into_any()
                                                    } else { view! { <span></span> }.into_any() }}
                                                </div>
                                            }.into_any()
                                        } else if is_incoming_promise {
                                            let now = (js_sys::Date::now() / 1000.0) as i64;
                                            let matured = entry.unlock_date.map_or(false, |u| u <= now) && entry_status == "Pending";
                                            let lid_claim = entry.tx_id.clone();
                                            if matured {
                                                view! {
                                                    <div style="margin-top:4px">
                                                        <button class="claim-btn"
                                                            style="background:#d4a84b;color:#0a0a0a;padding:4px 12px;border-radius:4px;border:none;cursor:pointer;font-weight:600;font-size:12px"
                                                            on:click=move |ev: web_sys::MouseEvent| {
                                                                ev.stop_propagation();
                                                                let lid = lid_claim.clone();
                                                                spawn_local(async move {
                                                                    inc_claim_msg.set("Mining PoW\u{2026}".into());
                                                                    let args = serde_wasm_bindgen::to_value(
                                                                        &serde_json::json!({ "lockIdHex": lid })
                                                                    ).unwrap_or(no_args());
                                                                    match call::<String>("claim_timelock", args).await {
                                                                        Ok(txid) => {
                                                                            inc_claim_msg.set(format!("Claimed! TxId: {}", &txid[..16.min(txid.len())]));
                                                                            // Poll until node confirms
                                                                            poll_balance_update(info).await;
                                                                            if let Ok(locks) = call::<Vec<TimeLockInfo>>("get_pending_incoming", no_args()).await {
                                                                                incoming.set(locks);
                                                                            }
                                                                        }
                                                                        Err(e) => inc_claim_msg.set(format!("Error: {e}")),
                                                                    }
                                                                });
                                                            }>
                                                            "Claim Now"
                                                        </button>
                                                    </div>
                                                }.into_any()
                                            } else {
                                                view! {
                                                    <span class="badge pending" style="margin-top:4px;display:inline-block">{entry_status.clone()}</span>
                                                }.into_any()
                                            }
                                        } else { view! { <span></span> }.into_any() }}
                                        {move || {
                                            let is_expanded = expanded.get().as_deref() == Some(tx_id.as_str());
                                            if is_expanded {
                                                let cancel_id = cancel_lock_id.clone();
                                                let detail_cascade_ids = entry_cascade_ids.clone();
                                                let btn_label = if is_cascade && is_email_send {
                                                    "Cancel Series"
                                                } else if is_email_send { "Cancel Send" } else { "Cancel Promise" };
                                                let code_opt = entry_claim_code.clone();
                                                let detail_reclaim_id = entry.tx_id.clone();
                                                view! {
                                                    <div class="history-detail">
                                                        <p>"TxID: " {tx_id_short.clone()}</p>
                                                        <p class="muted">"Status: " {status_display.clone()}</p>
                                                        // Show claim code for email sends so Alice can re-share it
                                                        {if is_email_send {
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
                                                        } else if is_cascade && cascade_has_claim {
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
                                }
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
                let modal_title = if is_series {
                    "Cancel Email Series?"
                } else if is_email { "Cancel Email Send?" } else { "Cancel Promise?" };
                let modal_body = if is_series {
                    "Cancel all sends in this series? The KX will return to your balance immediately."
                } else if is_email {
                    "Cancel this send? The KX will return to your balance immediately."
                } else {
                    "Are you sure you wish to cancel this Promise? The KX will be returned to your balance immediately. This cannot be undone."
                };
                let confirm_label = if is_series {
                    "Yes, Cancel Series"
                } else if is_email { "Yes, Cancel Send" } else { "Yes, Cancel Promise" };
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
                            <p style="font-size:13px;color:var(--muted);">
                                "Watch your inbox for free KX opportunities!"
                            </p>
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
) -> impl IntoView {
    // These signals are available for future use in the Settings panel
    let _ = (app_phase, pin_digits, pin_msg, pin_shake);
    let node_url   = RwSignal::new(String::new());
    let save_msg   = RwSignal::new(String::new());
    let pubkey_hex = RwSignal::new(String::new());
    let pk_loading = RwSignal::new(false);

    // Update check state
    let update_result   = RwSignal::new(Option::<UpdateInfo>::None);
    let update_checking = RwSignal::new(false);

    // Export/Import state
    let show_export       = RwSignal::new(false);
    let export_confirmed  = RwSignal::new(false);
    let export_key        = RwSignal::new(String::new());
    let export_loading    = RwSignal::new(false);
    let show_import       = RwSignal::new(false);
    let import_key        = RwSignal::new(String::new());
    let import_msg        = RwSignal::new(String::new());
    let import_busy       = RwSignal::new(false);

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

    // Multi claim emails (up to 3, local only, never sent to server)
    let claim_emails = RwSignal::new(Vec::<String>::new());
    let claim_email_msg = RwSignal::new(String::new());

    // Change PIN state
    let cp_phase    = RwSignal::new(0u8); // 0=verify current, 1=enter new, 2=confirm new
    let cp_digits   = RwSignal::new(String::new());
    let cp_first    = RwSignal::new(String::new());
    let cp_msg      = RwSignal::new(String::new());
    let cp_shake    = RwSignal::new(false);

    Effect::new(move |_| {
        spawn_local(async move {
            let url = call::<String>("get_node_url", no_args()).await.unwrap_or_default();
            node_url.set(url);
            let emails = call::<Vec<String>>("get_claim_emails", no_args()).await.unwrap_or_default();
            claim_emails.set(emails);
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

    let on_save_emails = move |_: web_sys::MouseEvent| {
        let emails = claim_emails.get_untracked();
        spawn_local(async move {
            let args = serde_wasm_bindgen::to_value(
                &serde_json::json!({ "emails": emails })
            ).unwrap_or(no_args());
            match call::<()>("set_claim_emails", args).await {
                Ok(_) => claim_email_msg.set(t(&lang.get(), "settings_emails_saved")),
                Err(e) => claim_email_msg.set(format!("Error: {e}")),
            }
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
        <div class="card">
            <p class="section-title">{move || t(&lang.get(), "tab_settings")}</p>

            // Language picker
            <div class="settings-section" style="margin-bottom:12px">
                <div class="row" style="justify-content:space-between;align-items:center;cursor:pointer"
                    on:click=move |_| show_lang_picker.set(!show_lang_picker.get_untracked())>
                    <span>{move || format!("\u{1f310} {}", t(&lang.get(), "settings_language"))}</span>
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

            // Mobile-only: History and Rewards links
            {if !desktop {
                view! {
                    <div class="settings-section" style="margin-bottom:8px">
                        <div class="row" style="cursor:pointer;padding:8px 0"
                            on:click=move |_| show_mobile_history.set(true)>
                            <span>{move || format!("\u{1f4dc} {} \u{2192}", t(&lang.get(), "transaction_history"))}</span>
                        </div>
                    </div>
                    <div class="settings-section" style="margin-bottom:8px">
                        <div class="row" style="cursor:pointer;padding:8px 0"
                            on:click=move |_| show_mobile_rewards.set(true)>
                            <span>{move || format!("\u{1f381} {} \u{2192}", t(&lang.get(), "tab_rewards"))}</span>
                        </div>
                    </div>
                }.into_any()
            } else {
                view! { <span></span> }.into_any()
            }}

            // Node URL (desktop only)
            {if desktop {
                view! {
                    <div class="field">
                        <label>{move || t(&lang.get(), "settings_node_url")}</label>
                        <input type="text" placeholder="http://127.0.0.1:8545"
                            prop:value=move || node_url.get()
                            on:input=move |ev| node_url.set(event_target_value(&ev)) />
                    </div>
                    <button class="primary" on:click=on_save>{move || t(&lang.get(), "settings_save_reconnect")}</button>
                    {move || {
                        let s = save_msg.get();
                        if s.is_empty() { view! { <span></span> }.into_any() }
                        else {
                            let cls = if s.starts_with("Error") { "msg error" } else { "msg success" };
                            view! { <p class=cls>{s}</p> }.into_any()
                        }
                    }}
                }.into_any()
            } else {
                view! { <span></span> }.into_any()
            }}

            // Public Key
            <div class="settings-section">
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
                                    {format!("\u{1f4cb} {}", t(&lang.get(), "settings_copy_pubkey"))}
                                </button>
                            </div>
                        }.into_any()
                    }
                }}
            </div>

            // Notices
            <div class="settings-section">
                <p class="label">{move || t(&lang.get(), "settings_notices")}</p>
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
            </div>

            // Security
            <div class="settings-section">
                <p class="label">{move || t(&lang.get(), "settings_security")}</p>
                <button on:click=move |_| {
                    cp_phase.set(0); cp_digits.set(String::new());
                    cp_msg.set(String::new()); show_change_pin.set(true);
                }>{move || format!("\u{1f510} {}", t(&lang.get(), "settings_change_pin"))}</button>

                <p class="muted" style="font-size:12px;margin-top:12px;margin-bottom:6px">{move || t(&lang.get(), "settings_pin_length")}</p>
                <div style="display:flex;gap:8px">
                    {[4u8, 6, 8].into_iter().map(|n| {
                        view! {
                            <button
                                class=move || if pin_len.get() == n { "pin-len-btn active" } else { "pin-len-btn" }
                                on:click=move |_| {
                                    if pin_len.get() != n {
                                        // Changing PIN length requires re-setting the PIN
                                        pin_len.set(n);
                                        spawn_local(async move {
                                            let args = serde_wasm_bindgen::to_value(
                                                &serde_json::json!({ "length": n })
                                            ).unwrap_or(no_args());
                                            let _ = call::<()>("set_pin_length", args).await;
                                        });
                                        // Open Change PIN modal so user sets a new PIN at the new length
                                        cp_phase.set(0); cp_digits.set(String::new());
                                        cp_msg.set(format!("PIN length changed to {} digits. Enter current PIN, then set a new {}-digit PIN.", n, n));
                                        show_change_pin.set(true);
                                    }
                                }
                            >{format!("{} {}", n, t(&lang.get(), "settings_digits"))}</button>
                        }
                    }).collect::<Vec<_>>()}
                </div>
            </div>

            // Backup Your Wallet
            <div class="settings-section">
                <p class="label">{move || t(&lang.get(), "settings_backup")}</p>
                <p class="muted" style="font-size:12px;margin-bottom:8px">
                    {move || t(&lang.get(), "settings_backup_sub")}
                </p>
                <button on:click=move |_| {
                    export_confirmed.set(false);
                    export_key.set(String::new());
                    show_export.set(true);
                }>{move || format!("\u{1f511} {}", t(&lang.get(), "settings_export_key"))}</button>
            </div>

            // Restore Wallet
            <div class="settings-section">
                <p class="label">{move || t(&lang.get(), "settings_restore")}</p>
                <p class="muted" style="font-size:12px;margin-bottom:8px">
                    {move || t(&lang.get(), "settings_restore_sub")}
                </p>
                <button on:click=move |_| {
                    import_key.set(String::new());
                    import_msg.set(String::new());
                    show_import.set(true);
                }>{move || format!("\u{1f4e5} {}", t(&lang.get(), "settings_import_key"))}</button>
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
                        }>{move || format!("\u{1f9ca} {}", t(&lang.get(), "settings_gen_cold"))}</button>
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

            // My Emails for KX Claims (up to 3)
            <div class="settings-section">
                <p class="label">{move || t(&lang.get(), "settings_claim_emails")}</p>
                <p class="muted" style="font-size:12px;margin-bottom:8px">
                    {move || t(&lang.get(), "settings_claim_emails_sub")}
                </p>
                <div class="claim-emails-list">
                    {move || {
                        let emails = claim_emails.get();
                        let rows: Vec<_> = emails.iter().enumerate().map(|(i, email)| {
                            let email_clone = email.clone();
                            let idx = i;
                            view! {
                                <div class="claim-email-row">
                                    <input type="email" placeholder="you@example.com"
                                        prop:value=email_clone
                                        on:input=move |ev| {
                                            let val = event_target_value(&ev);
                                            claim_emails.update(|list| {
                                                if idx < list.len() { list[idx] = val; }
                                            });
                                            claim_email_msg.set(String::new());
                                        } />
                                    <button style="font-size:12px;padding:4px 8px;color:#f87171;background:transparent;border:1px solid #f87171;border-radius:4px"
                                        on:click=move |_| {
                                            claim_emails.update(|list| { if idx < list.len() { list.remove(idx); } });
                                            claim_email_msg.set(String::new());
                                        }
                                    >"\u{2716}"</button>
                                </div>
                            }
                        }).collect();
                        view! { <div>{rows}</div> }.into_any()
                    }}
                </div>
                <div style="display:flex;gap:8px;flex-wrap:wrap">
                    {move || {
                        if claim_emails.get().len() < 3 {
                            view! {
                                <button on:click=move |_| {
                                    claim_emails.update(|list| list.push(String::new()));
                                    claim_email_msg.set(String::new());
                                }>{move || t(&lang.get(), "settings_add_email")}</button>
                            }.into_any()
                        } else {
                            view! { <span></span> }.into_any()
                        }
                    }}
                    <button class="primary" on:click=on_save_emails>{move || t(&lang.get(), "settings_save_emails")}</button>
                </div>
                {move || {
                    let s = claim_email_msg.get();
                    if s.is_empty() { view! { <span></span> }.into_any() }
                    else {
                        let cls = if s.starts_with("Error") { "msg error" } else { "msg success" };
                        view! { <p class=cls>{s}</p> }.into_any()
                    }
                }}
            </div>

            // About & Updates
            <div class="settings-section">
                <p class="label">{move || t(&lang.get(), "settings_about")}</p>
                <div style="display:flex;gap:8px;flex-wrap:wrap">
                    <button on:click=move |_| show_about.set(true)>{move || format!("\u{2139} {}", t(&lang.get(), "settings_about_chronx"))}</button>
                    <button on:click=move |_| show_updates.set(true)>{move || format!("\u{1f504} {}", t(&lang.get(), "settings_check_updates"))}</button>
                </div>
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
                        <p class="modal-title">{format!("\u{1f504} {}", t(&lang.get(), "settings_check_updates"))}</p>
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
                                    } else {
                                        let dl_url = info.download_url.clone();
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
                                                        spawn_local(async move {
                                                            let args = serde_wasm_bindgen::to_value(
                                                                &serde_json::json!({ "url": url })
                                                            ).unwrap_or(no_args());
                                                            let _ = call::<()>("open_url", args).await;
                                                        });
                                                    }>"\u{2b07} Download Update"</button>
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
                        <p class="modal-title">"📥 Import Private Key"</p>
                        <div class="modal-body" style="text-align:left">
                            <div class="export-warning" style="margin-bottom:12px">
                                <p style="font-weight:700;color:#f87171;font-size:13px">
                                    "⚠ Importing a private key will replace your current wallet. Make sure you have backed up your current private key first."
                                </p>
                            </div>
                            <p class="label" style="margin-bottom:6px">"Paste your backup key:"</p>
                            <textarea
                                class="restore-textarea"
                                style="width:100%;min-height:80px;font-family:monospace;font-size:11px"
                                placeholder="Paste your ChronX wallet backup key here"
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
                                    let cls = if m.starts_with("Error") || m.starts_with("Invalid") || m.starts_with("A wallet") { "msg error" } else { "msg success" };
                                    view! { <p class=cls>{m}</p> }.into_any()
                                }
                            }}
                            <div style="display:flex;gap:8px;margin-top:10px;flex-wrap:wrap">
                                <button class="primary"
                                    disabled=move || import_busy.get() || import_key.get().trim().is_empty()
                                    on:click=move |_| {
                                        let key = import_key.get_untracked().trim().to_string();
                                        import_busy.set(true);
                                        import_msg.set(String::new());
                                        spawn_local(async move {
                                            let args = serde_wasm_bindgen::to_value(
                                                &serde_json::json!({ "backupKey": key })
                                            ).unwrap_or(no_args());
                                            match call::<String>("restore_wallet", args).await {
                                                Ok(acct) => {
                                                    import_msg.set(format!("Wallet restored! Account: {}", acct));
                                                    // Delay then reload
                                                    delay_ms(2000).await;
                                                    let _ = web_sys::window().map(|w| w.location().reload());
                                                }
                                                Err(e) => import_msg.set(format!("{e}")),
                                            }
                                            import_busy.set(false);
                                        });
                                    }>
                                    {move || if import_busy.get() { "Restoring\u{2026}" } else { "Restore Wallet" }}
                                </button>
                                <button disabled=move || import_busy.get()
                                    on:click=move |_| show_import.set(false)>"Cancel"</button>
                            </div>
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
