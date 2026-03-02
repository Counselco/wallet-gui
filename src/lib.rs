use base64::Engine as _;
use js_sys::Promise;
use leptos::prelude::*;
use serde::Deserialize;
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::{spawn_local, JsFuture};

// ── Diagnostic console logger ─────────────────────────────────────────────────
macro_rules! clog {
    ($($t:tt)*) => {{
        web_sys::console::log_1(&wasm_bindgen::JsValue::from_str(&format!($($t)*)));
    }}
}

const LOGO_PNG: &[u8] = include_bytes!("../assets/chronx-logo.png");

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
    #[allow(dead_code)]
    sender: String,
    #[allow(dead_code)]
    recipient_account_id: String,
    amount_kx: String,
    unlock_at: i64,
    #[allow(dead_code)]
    created_at: i64,
    status: String,
    memo: Option<String>,
}

#[derive(Clone, Deserialize, Default)]
struct TxHistoryEntry {
    tx_id: String,
    tx_type: String,
    amount_chronos: Option<String>,
    counterparty: Option<String>,
    timestamp: i64,
    status: String,
}

// ── Server-pushed types ───────────────────────────────────────────────────────

#[derive(Clone, serde::Deserialize)]
struct Notice {
    id: String,
    title: String,
    body: String,
    severity: String, // "info" | "warning" | "critical"
    date: String,
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
    format!("{}.{:06}", format_int_with_commas(c / 1_000_000), (c % 1_000_000) as u32)
}

fn format_kx_str(kx_str: &str) -> String {
    let n: u128 = kx_str.trim().parse().unwrap_or(0);
    format_int_with_commas(n)
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

fn today_str() -> String {
    let d = js_sys::Date::new_0();
    let y = d.get_utc_full_year();
    let m = d.get_utc_month() + 1;
    let day = d.get_utc_date();
    format!("{y:04}-{m:02}-{day:02}")
}

fn date_str_to_unix(s: &str) -> Option<i64> {
    if s.len() != 10 { return None; }
    let utc_str = format!("{s}T00:00:00Z");
    let d = js_sys::Date::new(&wasm_bindgen::JsValue::from_str(&utc_str));
    let ms = d.get_time();
    if ms.is_nan() { return None; }
    Some((ms / 1000.0) as i64)
}

fn date_plus_months(months: u32) -> String {
    let d = js_sys::Date::new_0();
    let mut y = d.get_utc_full_year() as u32;
    let mut m = d.get_utc_month() + months;
    y += m / 12;
    m %= 12;
    let day = d.get_utc_date();
    format!("{y:04}-{m1:02}-{day:02}", m1 = m + 1)
}

fn date_plus_years(years: u32) -> String {
    let d = js_sys::Date::new_0();
    let y = d.get_utc_full_year() as u32 + years;
    let m = d.get_utc_month() + 1;
    let day = d.get_utc_date();
    format!("{y:04}-{m:02}-{day:02}")
}

// ── Clipboard ─────────────────────────────────────────────────────────────────

async fn copy_to_clipboard(text: String) {
    if let Some(win) = web_sys::window() {
        let clip = win.navigator().clipboard();
        let _ = JsFuture::from(clip.write_text(&text)).await;
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
    // 0=Account 1=Send 2=SendLater 3=Promises 4=History 5=Settings
    let active_tab  = RwSignal::new(0u8);
    let app_version = RwSignal::new("1.0.0".to_string());

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

            // Fetch notices & seen IDs in background (best effort)
            spawn_local(async move {
                if let Ok(ids) = call::<Vec<String>>("get_seen_notices", no_args()).await {
                    seen_ids.set(ids);
                }
                if let Ok(n) = call::<Vec<Notice>>("fetch_notices", no_args()).await {
                    notices.set(n);
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
        });
    };

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
                    on_submit=handle_pin
                />
            }.into_any(),

            AppPhase::Wallet => view! {
                <div class="app">
                    // Critical notices banner
                    {move || {
                        let crits: Vec<Notice> = notices.get().into_iter()
                            .filter(|n| n.severity == "critical" && !crit_dismissed.get().contains(&n.id))
                            .collect();
                        if crits.is_empty() {
                            view! { <span></span> }.into_any()
                        } else {
                            view! {
                                <div class="critical-notices-bar">
                                    {crits.into_iter().map(|n| {
                                        let nid = n.id.clone();
                                        view! {
                                            <div class="critical-notice-item">
                                                <span>"⚠ " {n.title.clone()} " — " {n.body.clone()}</span>
                                                <button class="critical-notice-close" on:click=move |_| {
                                                    let mut d = crit_dismissed.get_untracked();
                                                    d.push(nid.clone());
                                                    crit_dismissed.set(d);
                                                }>"✕"</button>
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
                            <nav class="tab-bar">
                                <button class=move || if active_tab.get()==0 {"tab active"} else {"tab"}
                                    on:click=move |_| active_tab.set(0)>"💰 Account"</button>
                                <button class=move || if active_tab.get()==1 {"tab active"} else {"tab"}
                                    on:click=move |_| active_tab.set(1)>"↗ Send"</button>
                                <button class=move || if active_tab.get()==2 {"tab active"} else {"tab"}
                                    on:click=move |_| active_tab.set(2)>"⏳ Send Later"</button>
                                <button class=move || if active_tab.get()==3 {"tab active"} else {"tab"}
                                    on:click=move |_| active_tab.set(3)>"📋 Promises"</button>
                                <button class=move || if active_tab.get()==4 {"tab active"} else {"tab"}
                                    on:click=move |_| active_tab.set(4)>"📜 History"</button>
                                <button class=move || if active_tab.get()==5 {"tab active"} else {"tab"}
                                    on:click=move |_| active_tab.set(5)>
                                    "⚙ Settings"
                                    {move || {
                                        let unread = notices.get().iter()
                                            .filter(|n| !seen_ids.get().contains(&n.id))
                                            .count();
                                        if unread > 0 {
                                            view! { <span class="notice-badge">{unread}</span> }.into_any()
                                        } else {
                                            view! { <span></span> }.into_any()
                                        }
                                    }}
                                </button>
                            </nav>
                        </div>
                    </header>

                    // Main content
                    <div>
                        {move || match active_tab.get() {
                            0 => view! {
                                <AccountPanel info=info loading=loading err_msg=err_msg on_refresh=on_refresh />
                            }.into_any(),
                            1 => view! { <SendPanel info=info /> }.into_any(),
                            2 => view! { <SendLaterPanel info=info /> }.into_any(),
                            3 => view! { <PromisesPanel /> }.into_any(),
                            4 => view! { <HistoryPanel /> }.into_any(),
                            5 => view! {
                                <SettingsPanel
                                    online=online
                                    app_phase=app_phase
                                    pin_digits=pin_digits
                                    pin_msg=pin_msg
                                    pin_shake=pin_shake
                                    app_version=app_version
                                    notices=notices
                                    seen_ids=seen_ids
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
                                            let _ = call::<()>("mark_notice_seen", args).await;
                                        });
                                    }
                                />
                            }.into_any(),
                            _ => view! { <span></span> }.into_any(),
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

// ── PinScreen ─────────────────────────────────────────────────────────────────

#[component]
fn PinScreen(
    phase: RwSignal<AppPhase>,
    pin_digits: RwSignal<String>,
    pin_msg: RwSignal<String>,
    pin_shake: RwSignal<bool>,
    countdown: RwSignal<u32>,
    on_submit: impl Fn(String) + Clone + Send + 'static,
) -> impl IntoView {
    let input_ref = NodeRef::<leptos::html::Input>::new();

    // Auto-focus the hidden input whenever the PIN screen phase changes
    Effect::new(move |_| {
        let _ = phase.get(); // track phase changes
        if let Some(el) = input_ref.get() {
            let _ = el.focus();
        }
    });

    // on_submit clones — auto Effect, keydown Enter, and Confirm button
    let on_submit_auto   = on_submit.clone();
    let on_submit_btn    = on_submit.clone();
    let on_submit_enter  = on_submit.clone();

    // Auto-submit when 4th digit is entered.
    // Clears pin_digits first to prevent double-fire from Confirm button.
    Effect::new(move |_| {
        let d = pin_digits.get();
        if d.len() == 4 {
            let captured = d.clone();
            pin_digits.set(String::new()); // clear before submit to block Confirm button race
            on_submit_auto(captured);
        }
    });

    // Keydown handler — handles digits and Backspace.
    // On desktop: keydown fires before input, prevent_default() stops input from firing.
    // On Android: keydown does NOT fire for virtual keyboard — handled by on_input instead.
    let on_keydown = move |ev: web_sys::KeyboardEvent| {
        let key = ev.key();
        if key.len() == 1 {
            if let Some(ch) = key.chars().next() {
                if ch.is_ascii_digit() {
                    ev.prevent_default();
                    let mut d = pin_digits.get_untracked();
                    if d.len() < 4 {
                        d.push(ch);
                        pin_digits.set(d);
                    }
                }
            }
        } else if key == "Backspace" {
            ev.prevent_default();
            let mut d = pin_digits.get_untracked();
            d.pop();
            pin_digits.set(d);
        } else if key == "Enter" {
            let d = pin_digits.get_untracked();
            if d.len() == 4 {
                ev.prevent_default();
                pin_digits.set(String::new());
                on_submit_enter(d);
            }
        }
    };

    // Input handler — handles Android virtual keyboard where keydown doesn't fire.
    // On desktop, keydown runs first with prevent_default(), so input.value() will be empty here.
    let on_input = move |ev: web_sys::Event| {
        use wasm_bindgen::JsCast;
        if let Some(input) = ev.target()
            .and_then(|t| t.dyn_into::<web_sys::HtmlInputElement>().ok())
        {
            let val = input.value();
            // Always clear the hidden input so it never accumulates text
            input.set_value("");
            // Only act if there's a digit present (mobile case)
            if let Some(ch) = val.chars().find(|c| c.is_ascii_digit()) {
                let mut d = pin_digits.get_untracked();
                if d.len() < 4 {
                    d.push(ch);
                    pin_digits.set(d);
                }
            }
        }
    };

    view! {
        <div class="app">
            <div style="text-align:center;padding:20px 0 8px">
                <img src=logo_src() alt="ChronX" style="height:44px;width:auto;display:inline-block" />
            </div>

            <div class="pin-screen">
                // Title
                <p class="pin-title">
                    {move || match phase.get() {
                        AppPhase::PinSetup   => "Create Your PIN",
                        AppPhase::PinConfirm => "Confirm Your PIN",
                        AppPhase::PinUnlock  => "Enter Your PIN",
                        _ => "PIN",
                    }}
                </p>

                // Subtitle
                <p class="pin-subtitle">
                    {move || match phase.get() {
                        AppPhase::PinSetup   => "Choose a 4-digit PIN to secure your wallet",
                        AppPhase::PinConfirm => "Enter the same PIN again to confirm",
                        AppPhase::PinUnlock  => "Enter your PIN to access your wallet",
                        _ => "",
                    }}
                </p>

                // PIN blocks
                <div class=move || if pin_shake.get() { "pin-blocks-wrap pin-shake" } else { "pin-blocks-wrap" }>
                    <div class="pin-blocks">
                        {(0..4usize).map(|i| {
                            view! {
                                <div class=move || {
                                    let len = pin_digits.get().len();
                                    if len > i { "pin-block filled" }
                                    else if len == i { "pin-block active" }
                                    else { "pin-block" }
                                }>
                                    {move || if pin_digits.get().len() > i { "\u{25cf}" } else { "" }}
                                </div>
                            }
                        }).collect_view()}
                    </div>
                </div>

                // Confirm button — appears when all 4 digits are entered
                {move || if pin_digits.get().len() == 4 {
                    let on_submit_btn2 = on_submit_btn.clone();
                    view! {
                        <button class="pin-confirm-btn" on:click=move |_| {
                            let d = pin_digits.get_untracked();
                            if d.len() == 4 {
                                pin_digits.set(String::new());
                                on_submit_btn2(d);
                            }
                        }>"Confirm"</button>
                    }.into_any()
                } else {
                    view! { <span></span> }.into_any()
                }}

                // Message / countdown
                {move || {
                    let c = countdown.get();
                    let msg = pin_msg.get();
                    if c > 0 {
                        view! {
                            <p class="pin-lockout-msg">
                                "\u{23f1} Please wait " {c} " seconds"
                            </p>
                        }.into_any()
                    } else if !msg.is_empty() {
                        view! { <p class="pin-msg">{msg}</p> }.into_any()
                    } else {
                        view! { <span></span> }.into_any()
                    }
                }}

                // Hidden input — captures keyboard on desktop and virtual keypad on Android
                <input
                    node_ref=input_ref
                    type="tel"
                    inputmode="numeric"
                    autocomplete="off"
                    class="pin-hidden-input"
                    on:keydown=on_keydown
                    on:input=on_input
                />

                // Coming soon helper
                <p class="pin-coming-soon">"Biometric / Windows Hello \u{2014} Coming Soon"</p>
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
) -> impl IntoView {
    let copy_success = RwSignal::new(false);
    let incoming     = RwSignal::new(Vec::<TimeLockInfo>::new());
    let inc_loading  = RwSignal::new(false);
    let qr_svg       = RwSignal::new(String::new());

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
        if !qr_svg.get_untracked().is_empty() {
            qr_svg.set(String::new());
            return;
        }
        let account_id = info.get_untracked().map(|a| a.account_id).unwrap_or_default();
        if account_id.is_empty() { return; }
        spawn_local(async move {
            let pk = call::<String>("export_public_key", no_args()).await.unwrap_or_default();
            let qr_data = if pk.is_empty() {
                account_id
            } else {
                format!("chronx:{account_id}:{pk}")
            };
            qr_svg.set(make_qr_svg(&qr_data));
        });
    };

    view! {
        <div class="card">
            <p class="label">"Account ID"</p>
            <div class="copy-row">
                <p class="mono"
                   title="Click to copy address"
                   style="cursor:pointer;flex:1"
                   on:click=on_copy>
                    {move || info.get()
                        .map(|a| a.account_id)
                        .unwrap_or_else(|| "\u{2014}".into())}
                </p>
                <button style="font-size:12px;padding:4px 10px" on:click=on_copy title="Copy address">
                    {move || if copy_success.get() { "Copied!" } else { "\u{1f4cb} Copy" }}
                </button>
                <button style="font-size:12px;padding:4px 10px" on:click=on_toggle_qr>
                    {move || if qr_svg.get().is_empty() { "📷 QR" } else { "Hide QR" }}
                </button>
            </div>

            {move || {
                let svg = qr_svg.get();
                if svg.is_empty() {
                    view! { <span></span> }.into_any()
                } else {
                    view! {
                        <div style="text-align:center;margin-top:12px;padding:12px;background:#fff;border-radius:8px">
                            <div inner_html=svg></div>
                            <p style="color:#555;font-size:11px;margin-top:6px">
                                "Scan on Send to transfer \u{b7} Scan on Send Later to make a promise"
                            </p>
                        </div>
                    }.into_any()
                }
            }}

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
                                info.get()
                                    .map(|a| format!("{} KX", format_kx(&a.spendable_chronos)))
                                    .unwrap_or_else(|| "\u{2014}".into())
                            }
                        }}
                    </p>
                    <p style="margin-top:8px;font-size:12px">
                        <a class="exchange-link" href="#" on:click=move |ev| {
                            ev.prevent_default();
                            spawn_local(async move {
                                let args = serde_wasm_bindgen::to_value(
                                    &serde_json::json!({ "url": "https://coinmarketcap.com/currencies/chronx/" })
                                ).unwrap_or(no_args());
                                let _ = call::<()>("open_url", args).await;
                            });
                        }>"\u{1f4c8} View KX on CoinMarketCap"</a>
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

            {move || {
                let locks = incoming.get();
                if inc_loading.get() {
                    view! { <p class="muted" style="margin-top:12px">"Checking incoming promises\u{2026}"</p> }.into_any()
                } else if locks.is_empty() {
                    view! { <span></span> }.into_any()
                } else {
                    let rows: Vec<_> = locks.into_iter().map(|lock| {
                        let unlock_date = {
                            let d = js_sys::Date::new(
                                &wasm_bindgen::JsValue::from_f64(lock.unlock_at as f64 * 1000.0)
                            );
                            format!("{:04}-{:02}-{:02}",
                                d.get_utc_full_year(),
                                d.get_utc_month() + 1,
                                d.get_utc_date())
                        };
                        view! {
                            <div class="incoming-lock-row">
                                <span class="tl-amount" style="color:#d4a84b">
                                    {format_kx_str(&lock.amount_kx)} " KX"
                                </span>
                                <span class="tl-unlock">"Unlocks " {unlock_date}</span>
                                {lock.memo.map(|m| view! { <span class="tl-memo">{m}</span> })}
                            </div>
                        }
                    }).collect();
                    view! {
                        <div style="margin-top:12px;border-top:1px solid #1e2130;padding-top:12px">
                            <p class="label">"Incoming Promises"</p>
                            <div class="timelock-list">{rows}</div>
                        </div>
                    }.into_any()
                }
            }}
        </div>
    }
}

// ── SendPanel ─────────────────────────────────────────────────────────────────

#[component]
fn SendPanel(info: RwSignal<Option<AccountInfo>>) -> impl IntoView {
    let to_addr  = RwSignal::new(String::new());
    let amount   = RwSignal::new(String::new());
    let sending  = RwSignal::new(false);
    let tx_msg   = RwSignal::new(String::new());
    let scan_msg = RwSignal::new(String::new());

    let on_scan_qr = move |_: web_sys::MouseEvent| {
        spawn_local(async move {
            scan_msg.set(String::new());
            match pick_image_file().await {
                None => scan_msg.set("No file selected.".into()),
                Some(file) => match scan_qr_file(file).await {
                    Ok(raw) => {
                        to_addr.set(qr_extract_account_id(&raw));
                        scan_msg.set("Address filled from QR.".into());
                    }
                    Err(e) => scan_msg.set(format!("Scan failed: {e}")),
                },
            }
        });
    };

    let on_send = move |_: web_sys::MouseEvent| {
        let to  = to_addr.get_untracked();
        let amt_str = amount.get_untracked();
        if to.is_empty() || amt_str.is_empty() {
            tx_msg.set("Error: fill in both To and Amount.".into()); return;
        }
        let amt: f64 = match amt_str.parse() {
            Ok(v) if v > 0.0 => v,
            Ok(_) => { tx_msg.set("Error: amount must be > 0.".into()); return; }
            Err(_) => { tx_msg.set("Error: invalid amount.".into()); return; }
        };
        spawn_local(async move {
            sending.set(true);
            tx_msg.set("Mining PoW\u{2026} (~10s)".into());
            let args = serde_wasm_bindgen::to_value(
                &serde_json::json!({ "to": to, "amountKx": amt })
            ).unwrap_or(no_args());
            match call::<String>("send_transfer", args).await {
                Ok(txid) => {
                    tx_msg.set(format!("Sent! TxId: {txid}"));
                    to_addr.set(String::new());
                    amount.set(String::new());
                    if let Ok(a) = call::<AccountInfo>("get_account_info", no_args()).await {
                        info.set(Some(a));
                    }
                }
                Err(e) => tx_msg.set(format!("Error: {e}")),
            }
            sending.set(false);
        });
    };

    view! {
        <div class="card">
            <p class="section-title">"Send KX"</p>
            <div class="field">
                <label>"To (account ID)"</label>
                <div style="display:flex;gap:8px;align-items:center">
                    <input type="text" placeholder="Base-58 address\u{2026}" style="flex:1"
                        prop:value=move || to_addr.get()
                        on:input=move |ev| to_addr.set(event_target_value(&ev))
                        disabled=move || sending.get() />
                    <button type="button" style="white-space:nowrap" on:click=on_scan_qr
                        disabled=move || sending.get()>"📷 Scan QR"</button>
                </div>
                {move || {
                    let s = scan_msg.get();
                    if s.is_empty() { view! { <span></span> }.into_any() }
                    else {
                        let cls = if s.starts_with("Scan failed") || s.starts_with("No file") { "msg error" } else { "msg success" };
                        view! { <p class=cls style="margin-top:4px">{s}</p> }.into_any()
                    }
                }}
            </div>
            <div class="field">
                <label>"Amount (KX)"</label>
                <input type="number" placeholder="0.000000" step="0.000001" min="0"
                    prop:value=move || amount.get()
                    on:input=move |ev| amount.set(event_target_value(&ev))
                    disabled=move || sending.get() />
            </div>
            <button class="primary" on:click=on_send disabled=move || sending.get()>
                {move || if sending.get() { "Sending\u{2026}" } else { "Send Transfer" }}
            </button>
            {move || {
                let s = tx_msg.get();
                if s.is_empty() { view! { <span></span> }.into_any() }
                else {
                    let cls = if s.starts_with("Error") { "msg error" }
                              else if s.starts_with("Mining") { "msg mining" }
                              else { "msg success" };
                    view! { <p class=cls>{s}</p> }.into_any()
                }
            }}
        </div>
    }
}

// ── SendLaterPanel ────────────────────────────────────────────────────────────

#[component]
fn SendLaterPanel(info: RwSignal<Option<AccountInfo>>) -> impl IntoView {
    let to_pubkey   = RwSignal::new(String::new());
    let lock_amount = RwSignal::new(String::new());
    let lock_date   = RwSignal::new(String::new());
    let lock_memo   = RwSignal::new(String::new());
    let locking     = RwSignal::new(false);
    let lock_msg    = RwSignal::new(String::new());
    let scan_msg    = RwSignal::new(String::new());

    let set_date = move |date: String| lock_date.set(date);

    let on_scan_qr = move |_: web_sys::MouseEvent| {
        spawn_local(async move {
            scan_msg.set(String::new());
            match pick_image_file().await {
                None => scan_msg.set("No file selected.".into()),
                Some(file) => match scan_qr_file(file).await {
                    Ok(raw) => {
                        to_pubkey.set(qr_extract_pubkey(&raw));
                        scan_msg.set("Recipient filled from QR.".into());
                    }
                    Err(e) => scan_msg.set(format!("Scan failed: {e}")),
                },
            }
        });
    };

    let on_lock = move |_: web_sys::MouseEvent| {
        let amt_str  = lock_amount.get_untracked();
        let date_str = lock_date.get_untracked();
        let memo_str = lock_memo.get_untracked();
        let pubkey   = to_pubkey.get_untracked();
        if amt_str.is_empty() { lock_msg.set("Error: enter an amount.".into()); return; }
        if date_str.is_empty() { lock_msg.set("Error: choose an unlock date.".into()); return; }
        let amt: f64 = match amt_str.parse() {
            Ok(v) if v > 0.0 => v,
            Ok(_) => { lock_msg.set("Error: amount must be > 0.".into()); return; }
            Err(_) => { lock_msg.set("Error: invalid amount.".into()); return; }
        };
        let unlock_unix = match date_str_to_unix(&date_str) {
            Some(t) => t,
            None => { lock_msg.set("Error: invalid date.".into()); return; }
        };
        let memo = if memo_str.is_empty() { None } else { Some(memo_str) };
        let to_pubkey_hex: Option<String> = if pubkey.is_empty() { None } else { Some(pubkey) };

        spawn_local(async move {
            locking.set(true);
            lock_msg.set("Mining PoW\u{2026} (~10s)".into());
            let args = serde_wasm_bindgen::to_value(&serde_json::json!({
                "amountKx": amt,
                "unlockAtUnix": unlock_unix,
                "memo": memo,
                "toPubkeyHex": to_pubkey_hex,
            })).unwrap_or(no_args());
            match call::<String>("create_timelock", args).await {
                Ok(txid) => {
                    clog!("[Promise] ✓ create_timelock OK — txid={txid}");
                    lock_msg.set(format!("Promise made! ID: {txid}"));
                    lock_amount.set(String::new());
                    lock_date.set(String::new());
                    lock_memo.set(String::new());
                    to_pubkey.set(String::new());
                    // Poll until the account nonce increments — proves the node has
                    // committed the transaction and updated the balance.
                    let prev_info = info.get_untracked();
                    let prev_nonce = prev_info.as_ref().map(|a| a.nonce).unwrap_or(0);
                    clog!(
                        "[Promise] info before poll: {} — prev_nonce={}",
                        if prev_info.is_some() { "Some" } else { "None" },
                        prev_nonce
                    );
                    let mut updated = false;
                    for i in 0..15u8 {
                        delay_ms(1000).await;
                        match call::<AccountInfo>("get_account_info", no_args()).await {
                            Ok(a) => {
                                clog!(
                                    "[Promise] poll {i}: nonce={} balance_chronos={} spendable_chronos={}",
                                    a.nonce, a.balance_chronos, a.spendable_chronos
                                );
                                if a.nonce > prev_nonce {
                                    clog!("[Promise] ✓ nonce incremented ({prev_nonce} → {}) — updating info", a.nonce);
                                    info.set(Some(a));
                                    updated = true;
                                    break;
                                }
                            }
                            Err(e) => {
                                clog!("[Promise] poll {i}: get_account_info error: {e}");
                            }
                        }
                    }
                    if !updated {
                        clog!("[Promise] ✗ nonce did not increment in 15s — TX likely rejected by node");
                        lock_msg.set("⚠ Transaction not confirmed — the node may have rejected it. Check that your unlock date is at least 1 hour in the future.".to_string());
                        match call::<AccountInfo>("get_account_info", no_args()).await {
                            Ok(a) => {
                                clog!(
                                    "[Promise] final refresh: nonce={} balance_chronos={} spendable_chronos={}",
                                    a.nonce, a.balance_chronos, a.spendable_chronos
                                );
                                info.set(Some(a));
                            }
                            Err(e) => clog!("[Promise] final refresh error: {e}"),
                        }
                    }
                }
                Err(e) => {
                    clog!("[Promise] ✗ create_timelock error: {e}");
                    lock_msg.set(format!("Error: {e}"));
                }
            }
            locking.set(false);
        });
    };

    let today = today_str();

    view! {
        <div class="card">
            <p class="section-title">"Send Later"</p>

            <div class="field">
                <label>"To (recipient public key hex, or leave blank to promise to yourself)"</label>
                <div style="display:flex;gap:8px;align-items:center">
                    <input type="text"
                        placeholder="Leave blank for self \u{b7} paste pubkey hex \u{b7} or scan QR\u{2026}"
                        style="flex:1"
                        prop:value=move || to_pubkey.get()
                        on:input=move |ev| to_pubkey.set(event_target_value(&ev))
                        disabled=move || locking.get() />
                    <button type="button" style="white-space:nowrap" on:click=on_scan_qr
                        disabled=move || locking.get()>"📷 Scan QR"</button>
                </div>
                {move || {
                    let s = scan_msg.get();
                    if s.is_empty() { view! { <span></span> }.into_any() }
                    else {
                        let cls = if s.starts_with("Scan failed") || s.starts_with("No file") { "msg error" } else { "msg success" };
                        view! { <p class=cls style="margin-top:4px">{s}</p> }.into_any()
                    }
                }}
                <p class="label" style="margin-top:4px">
                    {move || if to_pubkey.get().is_empty() {
                        "Promising to: yourself".to_string()
                    } else {
                        "Promising to: recipient (custom key)".to_string()
                    }}
                </p>
            </div>

            <div class="field">
                <label>"Amount (KX)"</label>
                <input type="number" placeholder="0.000000" step="0.000001" min="0"
                    prop:value=move || lock_amount.get()
                    on:input=move |ev| lock_amount.set(event_target_value(&ev))
                    disabled=move || locking.get() />
            </div>

            <div class="field">
                <label>"Unlock Date (UTC)"</label>
                <input type="date" min=today
                    prop:value=move || lock_date.get()
                    on:input=move |ev| lock_date.set(event_target_value(&ev))
                    disabled=move || locking.get() />
                <div class="quick-dates">
                    <button type="button" class="pill"
                        on:click=move |_| { let d=date_plus_months(1); set_date(d); }
                        disabled=move || locking.get()>"1 mo"</button>
                    <button type="button" class="pill"
                        on:click=move |_| { let d=date_plus_years(1); set_date(d); }
                        disabled=move || locking.get()>"1 yr"</button>
                    <button type="button" class="pill"
                        on:click=move |_| { let d=date_plus_years(5); set_date(d); }
                        disabled=move || locking.get()>"5 yr"</button>
                    <button type="button" class="pill"
                        on:click=move |_| { let d=date_plus_years(10); set_date(d); }
                        disabled=move || locking.get()>"10 yr"</button>
                    <button type="button" class="pill"
                        on:click=move |_| { let d=date_plus_years(25); set_date(d); }
                        disabled=move || locking.get()>"25 yr"</button>
                    <button type="button" class="pill"
                        on:click=move |_| { let d=date_plus_years(100); set_date(d); }
                        disabled=move || locking.get()>"100 yr"</button>
                </div>
            </div>

            <div class="field">
                <label>"Memo (optional, max 256 chars)"</label>
                <textarea placeholder="e.g. College fund for Maya \u{2014} do not touch until 2040"
                    maxlength="256" rows="3"
                    prop:value=move || lock_memo.get()
                    on:input=move |ev| lock_memo.set(event_target_value(&ev))
                    disabled=move || locking.get()></textarea>
            </div>

            <button class="primary danger" on:click=on_lock disabled=move || locking.get()>
                {move || if locking.get() { "Promising\u{2026}" } else { "Make a Promise" }}
            </button>

            <p class="lock-warning">
                "\u{26a0} Promised funds cannot be recovered before the unlock date. "
                "This action is permanent and cannot be undone."
            </p>

            {move || {
                let s = lock_msg.get();
                if s.is_empty() { view! { <span></span> }.into_any() }
                else {
                    let cls = if s.starts_with("Error") { "msg error" }
                              else if s.starts_with("Mining") { "msg mining" }
                              else { "msg success" };
                    view! { <p class=cls>{s}</p> }.into_any()
                }
            }}
        </div>
    }
}

// ── PromisesPanel ─────────────────────────────────────────────────────────────

#[component]
fn PromisesPanel() -> impl IntoView {
    let timelocks  = RwSignal::new(Vec::<TimeLockInfo>::new());
    let tl_loading = RwSignal::new(false);
    let tl_err     = RwSignal::new(String::new());
    let claim_msg  = RwSignal::new(String::new());

    let reload = move || {
        spawn_local(async move {
            tl_loading.set(true);
            tl_err.set(String::new());
            match call::<Vec<TimeLockInfo>>("get_timelocks", no_args()).await {
                Ok(locks) => timelocks.set(locks),
                Err(e)    => tl_err.set(e),
            }
            tl_loading.set(false);
        });
    };

    Effect::new(move |_| { reload(); });
    let on_refresh = move |_: web_sys::MouseEvent| { reload(); };

    view! {
        <div class="card">
            <div class="row">
                <p class="section-title">"My Promises"</p>
                <button on:click=on_refresh disabled=move || tl_loading.get()>
                    {move || if tl_loading.get() { "\u{2026}" } else { "\u{21bb} Refresh" }}
                </button>
            </div>

            {move || {
                let e = tl_err.get();
                if e.is_empty() { view! { <span></span> }.into_any() }
                else { view! { <p class="error">{e}</p> }.into_any() }
            }}

            {move || {
                let locks = timelocks.get();
                if tl_loading.get() {
                    view! { <p class="muted">"Loading\u{2026}"</p> }.into_any()
                } else if locks.is_empty() {
                    view! {
                        <div class="empty-state">
                            <p>"No promises found."</p>
                            <p class="muted">
                                "Promises you make will appear here once the node supports full scanning."
                            </p>
                        </div>
                    }.into_any()
                } else {
                    view! {
                        <div class="timelock-list">
                            {locks.into_iter().map(|lock| {
                                let now = (js_sys::Date::now() / 1000.0) as i64;
                                let matured = lock.unlock_at <= now;
                                let status_cls = match lock.status.as_str() {
                                    "Claimed" => "badge claimed",
                                    s if s.starts_with("ForSale") => "badge forsale",
                                    _ if matured => "badge matured",
                                    _ => "badge pending",
                                };
                                let status_label = if matured && lock.status == "Pending" {
                                    "Matured".to_string()
                                } else { lock.status.clone() };
                                let unlock_date = {
                                    let d = js_sys::Date::new(
                                        &wasm_bindgen::JsValue::from_f64(lock.unlock_at as f64 * 1000.0)
                                    );
                                    format!("{:04}-{:02}-{:02}",
                                        d.get_utc_full_year(), d.get_utc_month() + 1, d.get_utc_date())
                                };
                                let lock_id = lock.lock_id.clone();
                                let can_claim = matured && lock.status == "Pending";
                                let on_claim = {
                                    let lid = lock_id.clone();
                                    move |_: web_sys::MouseEvent| {
                                        let lid2 = lid.clone();
                                        spawn_local(async move {
                                            claim_msg.set("Mining PoW\u{2026}".into());
                                            let args = serde_wasm_bindgen::to_value(
                                                &serde_json::json!({ "lockIdHex": lid2 })
                                            ).unwrap_or(no_args());
                                            match call::<String>("claim_timelock", args).await {
                                                Ok(txid) => claim_msg.set(format!("Claimed! TxId: {txid}")),
                                                Err(e)   => claim_msg.set(format!("Error: {e}")),
                                            }
                                        });
                                    }
                                };
                                view! {
                                    <div class="timelock-row">
                                        <div class="tl-main">
                                            <span class="tl-amount">{format_kx_str(&lock.amount_kx)} " KX"</span>
                                            <span class="tl-unlock">"Unlocks " {unlock_date}</span>
                                            {lock.memo.clone().map(|m| view! { <span class="tl-memo">{m}</span> })}
                                        </div>
                                        <div class="tl-right">
                                            <span class=status_cls>{status_label}</span>
                                            {if can_claim {
                                                view! {
                                                    <button class="claim-btn" on:click=on_claim>"Claim"</button>
                                                }.into_any()
                                            } else { view! { <span></span> }.into_any() }}
                                        </div>
                                    </div>
                                }
                            }).collect::<Vec<_>>()}
                        </div>
                    }.into_any()
                }
            }}

            {move || {
                let s = claim_msg.get();
                if s.is_empty() { view! { <span></span> }.into_any() }
                else {
                    let cls = if s.starts_with("Error") { "msg error" }
                              else if s.starts_with("Mining") { "msg mining" }
                              else { "msg success" };
                    view! { <p class=cls>{s}</p> }.into_any()
                }
            }}
        </div>
    }
}

// ── HistoryPanel ──────────────────────────────────────────────────────────────

#[component]
fn HistoryPanel() -> impl IntoView {
    let entries    = RwSignal::new(Vec::<TxHistoryEntry>::new());
    let h_loading  = RwSignal::new(false);
    let h_err      = RwSignal::new(String::new());
    let expanded   = RwSignal::new(Option::<String>::None); // tx_id of expanded row

    let reload = move || {
        spawn_local(async move {
            h_loading.set(true);
            h_err.set(String::new());
            match call::<Vec<TxHistoryEntry>>("get_transaction_history", no_args()).await {
                Ok(e)  => entries.set(e),
                Err(e) => h_err.set(e),
            }
            h_loading.set(false);
        });
    };

    Effect::new(move |_| { reload(); });
    let on_refresh = move |_: web_sys::MouseEvent| { reload(); };

    view! {
        <div class="card">
            <div class="row">
                <p class="section-title">"Transaction History"</p>
                <button on:click=on_refresh disabled=move || h_loading.get()>
                    {move || if h_loading.get() { "\u{2026}" } else { "\u{21bb} Refresh" }}
                </button>
            </div>

            {move || {
                let e = h_err.get();
                if e.is_empty() { view! { <span></span> }.into_any() }
                else { view! { <p class="error">{e}</p> }.into_any() }
            }}

            {move || {
                let list = entries.get();
                if h_loading.get() {
                    view! { <p class="muted">"Loading\u{2026}"</p> }.into_any()
                } else if list.is_empty() && h_err.get().is_empty() {
                    view! {
                        <div class="empty-state">
                            <p>"\u{1f552} No transactions yet"</p>
                            <p class="muted">"Transactions will appear here once confirmed on-chain."</p>
                        </div>
                    }.into_any()
                } else {
                    view! {
                        <div class="history-list">
                            {list.into_iter().map(|entry| {
                                let tx_id = entry.tx_id.clone();
                                let tx_id_for_toggle = tx_id.clone();
                                let type_icon = match entry.tx_type.as_str() {
                                    "TimeLockCreate" => "\u{23f3}",
                                    "TimeLockClaim"  => "\u{2705}",
                                    _                => "\u{2197}",
                                };
                                let amount_display = entry.amount_chronos.as_deref()
                                    .map(|c| format!("{} KX", format_kx(c)))
                                    .unwrap_or_else(|| "\u{2014}".to_string());
                                let addr_display = entry.counterparty.as_deref()
                                    .map(shorten_addr)
                                    .unwrap_or_default();
                                let date_display = format_utc_ts(entry.timestamp);
                                let tx_id_short = shorten_addr(&entry.tx_id);

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
                                            <span class="history-amount">{amount_display}</span>
                                        </div>
                                        <div class="history-row-bottom">
                                            <span class="history-addr">{addr_display}</span>
                                            <span class="history-date">{date_display}</span>
                                        </div>
                                        {move || {
                                            if expanded.get().as_deref() == Some(tx_id.as_str()) {
                                                view! {
                                                    <div class="history-detail">
                                                        "TxID: " {tx_id_short.clone()}
                                                    </div>
                                                }.into_any()
                                            } else { view! { <span></span> }.into_any() }
                                        }}
                                    </div>
                                }
                            }).collect::<Vec<_>>()}
                        </div>
                    }.into_any()
                }
            }}
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
    let update_err      = RwSignal::new(String::new());

    // Modal visibility
    let show_about   = RwSignal::new(false);
    let show_updates = RwSignal::new(false);
    let show_change_pin = RwSignal::new(false);

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
        spawn_local(async move {
            pk_loading.set(true);
            match call::<String>("export_public_key", no_args()).await {
                Ok(pk) => pubkey_hex.set(pk),
                Err(e) => pubkey_hex.set(format!("Error: {e}")),
            }
            pk_loading.set(false);
        });
    };

    // Change PIN: handle submit on 4 digits
    let cp_input_ref = NodeRef::<leptos::html::Input>::new();

    Effect::new(move |_| {
        let _ = show_change_pin.get();
        if let Some(el) = cp_input_ref.get() {
            if show_change_pin.get_untracked() {
                let _ = el.focus();
            }
        }
    });

    let cp_on_input = move |ev: web_sys::Event| {
        use wasm_bindgen::JsCast;
        if let Some(input) = ev.target().and_then(|t| t.dyn_into::<web_sys::HtmlInputElement>().ok()) {
            let val = input.value();
            let digits: String = val.chars().filter(|c| c.is_ascii_digit()).take(4).collect();
            input.set_value("");
            let current = cp_digits.get_untracked();
            let new_val = if digits.len() > current.len() {
                format!("{}{}", current, &digits[current.len()..])
            } else { digits };
            let trimmed: String = new_val.chars().take(4).collect();
            cp_digits.set(trimmed);
        }
    };

    let cp_on_keydown = move |ev: web_sys::KeyboardEvent| {
        if ev.key() == "Backspace" {
            let mut d = cp_digits.get_untracked();
            d.pop();
            cp_digits.set(d);
            ev.prevent_default();
        }
    };

    Effect::new(move |_| {
        let d = cp_digits.get();
        if d.len() == 4 {
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
                                cp_msg.set("Incorrect PIN".to_string());
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
                                    cp_msg.set("PIN changed successfully!".to_string());
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
                        cp_msg.set("PINs do not match \u{2014} please try again".to_string());
                        cp_phase.set(1);
                        cp_first.set(String::new());
                    }
                }
                _ => {}
            }
        }
    });

    view! {
        <div class="card">
            <p class="section-title">"Settings"</p>

            // Node URL
            <div class="field">
                <label>"Node URL"</label>
                <input type="text" placeholder="http://127.0.0.1:8545"
                    prop:value=move || node_url.get()
                    on:input=move |ev| node_url.set(event_target_value(&ev)) />
            </div>
            <button class="primary" on:click=on_save>"Save & Reconnect"</button>
            {move || {
                let s = save_msg.get();
                if s.is_empty() { view! { <span></span> }.into_any() }
                else {
                    let cls = if s.starts_with("Error") { "msg error" } else { "msg success" };
                    view! { <p class=cls>{s}</p> }.into_any()
                }
            }}

            // Public Key
            <div class="settings-section">
                <p class="label">"My Public Key (share so others can promise KX to you)"</p>
                <button on:click=on_show_pubkey disabled=move || pk_loading.get()>
                    {move || if pk_loading.get() { "Loading\u{2026}" } else { "Show Public Key" }}
                </button>
                {move || {
                    let pk = pubkey_hex.get();
                    if pk.is_empty() { view! { <span></span> }.into_any() }
                    else { view! { <p class="mono" style="font-size:10px;word-break:break-all;margin-top:8px">{pk}</p> }.into_any() }
                }}
            </div>

            // Notices
            <div class="settings-section">
                <p class="label">"Notices"</p>
                {move || {
                    let all = notices.get();
                    let seen = seen_ids.get();
                    let unread = all.iter().filter(|n| !seen.contains(&n.id)).count();
                    if all.is_empty() {
                        view! { <p class="muted">"No notices."</p> }.into_any()
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
                                {all.into_iter().map(|n| {
                                    let is_read = seen.contains(&n.id);
                                    let nid = n.id.clone();
                                    let on_mark_n = on_mark_c.clone();
                                    view! {
                                        <div class=format!("notice-card {}", n.severity)
                                             style=format!("opacity:{}", if is_read { "0.55" } else { "1" })>
                                            <p class="notice-card-title">{n.title.clone()}</p>
                                            <p class="notice-card-date">{n.date.clone()}</p>
                                            <p class="notice-card-body">{n.body.clone()}</p>
                                            {if !is_read {
                                                view! {
                                                    <button class="notice-mark-read"
                                                        on:click=move |_| on_mark_n(nid.clone())>
                                                        "Mark as read"
                                                    </button>
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
                <p class="label">"Security"</p>
                <button on:click=move |_| {
                    cp_phase.set(0); cp_digits.set(String::new());
                    cp_msg.set(String::new()); show_change_pin.set(true);
                }>"🔐 Change PIN"</button>
            </div>

            // About & Updates
            <div class="settings-section">
                <p class="label">"About"</p>
                <div style="display:flex;gap:8px;flex-wrap:wrap">
                    <button on:click=move |_| show_about.set(true)>"\u{2139} About ChronX"</button>
                    <button on:click=move |_| show_updates.set(true)>"\u{1f504} Check for Updates"</button>
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
                        <button class="primary" on:click=move |_| show_about.set(false)>"Close"</button>
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
                    update_err.set(String::new());
                }>
                    <div class="modal-card" on:click=move |ev| ev.stop_propagation()>
                        <p class="modal-title">"\u{1f504} Check for Updates"</p>
                        <div class="modal-body">
                            <p class="label">"Current version: " {version}</p>
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
                                    let e = update_err.get();
                                    if e.is_empty() {
                                        view! { <span></span> }.into_any()
                                    } else {
                                        view! { <p class="msg error">{e}</p> }.into_any()
                                    }
                                }
                            }}
                        </div>
                        <div style="display:flex;gap:8px;flex-wrap:wrap;margin-top:4px">
                            <button on:click=move |_| {
                                update_checking.set(true);
                                update_result.set(None);
                                update_err.set(String::new());
                                spawn_local(async move {
                                    match call::<UpdateInfo>("check_for_updates", no_args()).await {
                                        Ok(info) => update_result.set(Some(info)),
                                        Err(e)   => update_err.set(format!("Error: {e}")),
                                    }
                                    update_checking.set(false);
                                });
                            } disabled=move || update_checking.get()>
                                {move || if update_checking.get() { "Checking\u{2026}" } else { "Check Now" }}
                            </button>
                            <button on:click=move |_| {
                                show_updates.set(false);
                                update_result.set(None);
                                update_err.set(String::new());
                            }>"Close"</button>
                        </div>
                    </div>
                </div>
            }.into_any()
        } else { view! { <span></span> }.into_any() }}

        // ── Change PIN modal ──────────────────────────────────────────────────

        {move || if show_change_pin.get() {
            let cp_title = move || match cp_phase.get() {
                0 => "Enter Current PIN",
                1 => "Enter New PIN",
                _ => "Confirm New PIN",
            };
            view! {
                <div class="modal-overlay" on:click=move |_| {
                    show_change_pin.set(false);
                    cp_digits.set(String::new());
                    cp_msg.set(String::new());
                }>
                    <div class="modal-card" on:click=move |ev| ev.stop_propagation()>
                        <p class="modal-title">"Change PIN"</p>
                        <p class="pin-subtitle">{cp_title}</p>

                        // PIN blocks for Change PIN
                        <div class=move || if cp_shake.get() { "pin-blocks-wrap pin-shake" } else { "pin-blocks-wrap" }>
                            <div class="pin-blocks">
                                {(0..4usize).map(|i| {
                                    view! {
                                        <div class=move || {
                                            let len = cp_digits.get().len();
                                            if len > i { "pin-block filled" }
                                            else if len == i { "pin-block active" }
                                            else { "pin-block" }
                                        }>
                                            {move || if cp_digits.get().len() > i { "\u{25cf}" } else { "" }}
                                        </div>
                                    }
                                }).collect_view()}
                            </div>
                        </div>

                        {move || {
                            let msg = cp_msg.get();
                            if msg.is_empty() { view! { <span></span> }.into_any() }
                            else {
                                let cls = if msg.starts_with("PIN changed") { "msg success" } else { "pin-msg" };
                                view! { <p class=cls>{msg}</p> }.into_any()
                            }
                        }}

                        // Hidden capture input
                        <input node_ref=cp_input_ref type="tel" inputmode="numeric"
                            autocomplete="off" class="pin-hidden-input"
                            on:input=cp_on_input on:keydown=cp_on_keydown />

                        <button on:click=move |_| {
                            show_change_pin.set(false);
                            cp_digits.set(String::new());
                            cp_msg.set(String::new());
                        }>"Cancel"</button>
                    </div>
                </div>
            }.into_any()
        } else { view! { <span></span> }.into_any() }}
    }
}
