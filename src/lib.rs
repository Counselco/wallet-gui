use base64::Engine as _;
use js_sys::Promise;
use leptos::prelude::*;
use serde::Deserialize;
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::{spawn_local, JsFuture};

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

    /// QR scan helper injected via index.html (BarcodeDetector Web API).
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

// ── Number formatting ─────────────────────────────────────────────────────────

/// Format a u128 integer with comma separators: 10000000 → "10,000,000"
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

/// Convert raw Chronos string → "X,XXX,XXX.XXXXXX" KX display.
/// 10_000_000_000_000 Chronos → "10,000,000.000000"
/// 500_000 Chronos            → "0.500000"
fn format_kx(chronos_str: &str) -> String {
    let c: u128 = chronos_str.parse().unwrap_or(0);
    format!("{}.{:06}", format_int_with_commas(c / 1_000_000), (c % 1_000_000) as u32)
}

/// Add comma separators to a KX integer string: "10000000" → "10,000,000"
fn format_kx_str(kx_str: &str) -> String {
    let n: u128 = kx_str.trim().parse().unwrap_or(0);
    format_int_with_commas(n)
}

// ── QR code generation ────────────────────────────────────────────────────────

/// Generate an inline SVG QR code for the given text.
/// Returns an empty string on failure.
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

/// Scan a QR image file using the BarcodeDetector Web API.
/// Returns the raw string value or an error message.
async fn scan_qr_file(file: web_sys::File) -> Result<String, String> {
    let result = JsFuture::from(scan_qr_js(&file))
        .await
        .map_err(|e| e.as_string().unwrap_or_else(|| format!("{e:?}")))?;
    result
        .as_string()
        .ok_or_else(|| "No QR code found in image (or scanner unavailable)".to_string())
}

/// Extract account_id from a scanned QR value.
/// Accepts "chronx:<id>:<pk>" or plain "<id>".
fn qr_extract_account_id(raw: &str) -> String {
    if let Some(rest) = raw.strip_prefix("chronx:") {
        rest.split(':').next().unwrap_or("").to_string()
    } else {
        raw.to_string()
    }
}

/// Extract pubkey hex from a scanned QR value.
/// Accepts "chronx:<id>:<pk>" or plain "<pk>".
fn qr_extract_pubkey(raw: &str) -> String {
    if let Some(rest) = raw.strip_prefix("chronx:") {
        rest.split(':').nth(1).unwrap_or("").to_string()
    } else {
        raw.to_string()
    }
}

// ── Date helpers ──────────────────────────────────────────────────────────────

/// Return current UTC date as "YYYY-MM-DD" for the date input min attribute.
fn today_str() -> String {
    let d = js_sys::Date::new_0();
    let y = d.get_utc_full_year();
    let m = d.get_utc_month() + 1; // 0-indexed
    let day = d.get_utc_date();
    format!("{y:04}-{m:02}-{day:02}")
}

/// "YYYY-MM-DD" → Unix timestamp at midnight UTC.
fn date_str_to_unix(s: &str) -> Option<i64> {
    if s.len() != 10 { return None; }
    // Append UTC time component so the browser parses it as UTC midnight.
    let utc_str = format!("{s}T00:00:00Z");
    let d = js_sys::Date::new(&wasm_bindgen::JsValue::from_str(&utc_str));
    let ms = d.get_time();
    if ms.is_nan() { return None; }
    Some((ms / 1000.0) as i64)
}

/// Add `months` calendar months to today's date, return "YYYY-MM-DD".
fn date_plus_months(months: u32) -> String {
    let d = js_sys::Date::new_0();
    let mut y = d.get_utc_full_year() as u32;
    let mut m = d.get_utc_month() + months; // 0-indexed, may overflow
    y += m / 12;
    m %= 12;
    let day = d.get_utc_date();
    format!("{y:04}-{m1:02}-{day:02}", m1 = m + 1)
}

/// Add `years` to today's date, return "YYYY-MM-DD".
fn date_plus_years(years: u32) -> String {
    let d = js_sys::Date::new_0();
    let y = d.get_utc_full_year() as u32 + years;
    let m = d.get_utc_month() + 1;
    let day = d.get_utc_date();
    format!("{y:04}-{m:02}-{day:02}")
}

// ── Clipboard ─────────────────────────────────────────────────────────────────

/// Copy text to clipboard; resolves when done (or silently fails).
async fn copy_to_clipboard(text: String) {
    if let Some(win) = web_sys::window() {
        let clip = win.navigator().clipboard();
        let _ = JsFuture::from(clip.write_text(&text)).await;
    }
}

/// Delay ~1.5 s using a JS Promise.
async fn delay_1500ms() {
    let promise = Promise::new(&mut |resolve, _| {
        if let Some(win) = web_sys::window() {
            let _ = win.set_timeout_with_callback_and_timeout_and_arguments_0(&resolve, 1500);
        }
    });
    let _ = JsFuture::from(promise).await;
}

// ── Trigger a hidden file input ───────────────────────────────────────────────

/// Programmatically click a hidden file input and return the chosen File.
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
    // capture="environment" is respected by Android WebView (opens back camera);
    // desktop browsers ignore it and show a normal file picker.
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
    // Clean up the temporary input element.
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
    let active_tab  = RwSignal::new(0u8); // 0=Account 1=Send 2=SendLater 3=Promises 4=Settings

    // First-run / setup state (wallet not found).
    let need_setup  = RwSignal::new(false);
    let setup_msg   = RwSignal::new(String::new());
    let setup_busy  = RwSignal::new(false);

    // ── Data refresh ─────────────────────────────────────────────────────────

    async fn refresh_data(
        online: RwSignal<bool>,
        loading: RwSignal<bool>,
        err_msg: RwSignal<String>,
        info: RwSignal<Option<AccountInfo>>,
        need_setup: RwSignal<bool>,
    ) {
        online.set(call::<bool>("check_node", no_args()).await.unwrap_or(false));
        loading.set(true);
        err_msg.set(String::new());
        match call::<AccountInfo>("get_account_info", no_args()).await {
            Ok(a) => {
                info.set(Some(a));
                need_setup.set(false);
            }
            Err(e) => {
                // "Wallet not found" → show first-run setup screen.
                if e.contains("Wallet not found") || e.contains("keygen") {
                    need_setup.set(true);
                } else {
                    err_msg.set(e);
                }
            }
        }
        loading.set(false);
    }

    Effect::new(move |_| {
        spawn_local(async move {
            refresh_data(online, loading, err_msg, info, need_setup).await;
        });
    });

    let on_refresh = move |_: web_sys::MouseEvent| {
        spawn_local(async move {
            refresh_data(online, loading, err_msg, info, need_setup).await;
        });
    };

    // ── Setup: generate new wallet ────────────────────────────────────────────

    let on_generate = move |_: web_sys::MouseEvent| {
        spawn_local(async move {
            setup_busy.set(true);
            setup_msg.set(String::new());
            match call::<String>("generate_wallet", no_args()).await {
                Ok(account_id) => {
                    setup_msg.set(format!("Wallet created! Account ID:\n{account_id}"));
                    // Re-load account info.
                    refresh_data(online, loading, err_msg, info, need_setup).await;
                }
                Err(e) => setup_msg.set(format!("Error: {e}")),
            }
            setup_busy.set(false);
        });
    };

    // ── View ──────────────────────────────────────────────────────────────────

    view! {
        <div class="app">

            // Header: logo + node status + tab bar
            <header>
                <a href="https://www.chronx.io" target="_blank" rel="noopener" class="logo-link">
                    <img src=logo_src() alt="ChronX Logo" style="height:40px;width:auto;display:block;" />
                </a>
                <div class="header-right">
                    <span class="node-status">
                        <span class=move || if online.get() { "dot online" } else { "dot offline" }></span>
                        {move || if online.get() { "Online" } else { "Offline" }}
                    </span>
                    {move || if !need_setup.get() {
                        view! {
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
                                    on:click=move |_| active_tab.set(4)>"⚙ Settings"</button>
                            </nav>
                        }.into_any()
                    } else {
                        view! { <span></span> }.into_any()
                    }}
                </div>
            </header>

            // First-run / setup screen
            {move || if need_setup.get() {
                view! { <SetupPanel on_generate=on_generate busy=setup_busy msg=setup_msg /> }.into_any()
            } else {
                view! {
                    <div>
                        {move || match active_tab.get() {
                            0 => view! {
                                <AccountPanel info=info loading=loading err_msg=err_msg on_refresh=on_refresh />
                            }.into_any(),
                            1 => view! { <SendPanel info=info /> }.into_any(),
                            2 => view! { <SendLaterPanel info=info /> }.into_any(),
                            3 => view! { <PromisesPanel /> }.into_any(),
                            4 => view! { <SettingsPanel online=online /> }.into_any(),
                            _ => view! { <span></span> }.into_any(),
                        }}
                    </div>
                }.into_any()
            }}

        </div>
    }
}

// ── SetupPanel ────────────────────────────────────────────────────────────────

#[component]
fn SetupPanel(
    on_generate: impl Fn(web_sys::MouseEvent) + 'static,
    busy: RwSignal<bool>,
    msg: RwSignal<String>,
) -> impl IntoView {
    view! {
        <div class="card setup-card">
            <p class="section-title">"Welcome to ChronX Wallet"</p>
            <p class="label">"No wallet found on this device."</p>
            <button class="primary" on:click=on_generate disabled=move || busy.get()>
                {move || if busy.get() { "Generating…" } else { "Create New Wallet" }}
            </button>
            {move || {
                let s = msg.get();
                if s.is_empty() { view! { <span></span> }.into_any() }
                else {
                    let cls = if s.starts_with("Error") { "msg error" } else { "msg success" };
                    view! { <p class=cls style="white-space:pre-wrap">{s}</p> }.into_any()
                }
            }}
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
    let qr_svg       = RwSignal::new(String::new()); // empty = hidden

    // Load pending incoming promises on mount.
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
        let addr = info.get_untracked()
            .map(|a| a.account_id)
            .unwrap_or_default();
        if addr.is_empty() { return; }
        spawn_local(async move {
            copy_to_clipboard(addr).await;
            copy_success.set(true);
            delay_1500ms().await;
            copy_success.set(false);
        });
    };

    // Show / hide QR code. When showing, also fetch the public key to embed.
    let on_toggle_qr = move |_: web_sys::MouseEvent| {
        if !qr_svg.get_untracked().is_empty() {
            qr_svg.set(String::new());
            return;
        }
        let account_id = info.get_untracked()
            .map(|a| a.account_id)
            .unwrap_or_default();
        if account_id.is_empty() { return; }
        spawn_local(async move {
            // Fetch public key so the QR encodes both id and pubkey.
            let pk = call::<String>("export_public_key", no_args())
                .await
                .unwrap_or_default();
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
                   title="Click to copy"
                   style="cursor:pointer;flex:1"
                   on:click=on_copy>
                    {move || info.get()
                        .map(|a| a.account_id)
                        .unwrap_or_else(|| "\u{2014}".into())}
                </p>
                <span class=move || if copy_success.get() { "copy-badge visible" } else { "copy-badge" }>
                    "Copied!"
                </span>
                <button style="margin-left:8px;font-size:13px" on:click=on_toggle_qr>
                    {move || if qr_svg.get().is_empty() { "📷 QR" } else { "Hide QR" }}
                </button>
            </div>

            // QR code display
            {move || {
                let svg = qr_svg.get();
                if svg.is_empty() {
                    view! { <span></span> }.into_any()
                } else {
                    view! {
                        <div style="text-align:center;margin-top:12px;padding:12px;background:#fff;border-radius:8px">
                            <div inner_html=svg></div>
                            <p style="color:#555;font-size:11px;margin-top:6px">
                                "Scan on Send to transfer · Scan on Send Later to make a promise"
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

            // ── Incoming promises ─────────────────────────────────────────────
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
                                {lock.memo.map(|m| view! {
                                    <span class="tl-memo">{m}</span>
                                })}
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
            tx_msg.set("Error: fill in both To and Amount.".into());
            return;
        }
        let amt: f64 = match amt_str.parse() {
            Ok(v) if v > 0.0 => v,
            Ok(_) => { tx_msg.set("Error: amount must be > 0.".into()); return; }
            Err(_) => { tx_msg.set("Error: invalid amount.".into()); return; }
        };

        spawn_local(async move {
            sending.set(true);
            tx_msg.set("Mining PoW\u{2026} (~10s)".into());

            // Tauri v2 expects camelCase arg keys.
            let args = serde_wasm_bindgen::to_value(
                &serde_json::json!({ "to": to, "amountKx": amt })
            ).unwrap_or(no_args());

            match call::<String>("send_transfer", args).await {
                Ok(txid) => {
                    tx_msg.set(format!("Sent! TxId: {txid}"));
                    to_addr.set(String::new());
                    amount.set(String::new());
                    // Refresh balance (best effort).
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
                    <input
                        type="text"
                        placeholder="Base-58 address\u{2026}"
                        style="flex:1"
                        prop:value=move || to_addr.get()
                        on:input=move |ev| to_addr.set(event_target_value(&ev))
                        disabled=move || sending.get()
                    />
                    <button type="button" style="white-space:nowrap" on:click=on_scan_qr
                        disabled=move || sending.get()>
                        "📷 Scan QR"
                    </button>
                </div>
                {move || {
                    let s = scan_msg.get();
                    if s.is_empty() { view! { <span></span> }.into_any() }
                    else {
                        let cls = if s.starts_with("Scan failed") || s.starts_with("No file") { "msg error" }
                                  else { "msg success" };
                        view! { <p class=cls style="margin-top:4px">{s}</p> }.into_any()
                    }
                }}
            </div>

            <div class="field">
                <label>"Amount (KX)"</label>
                <input
                    type="number"
                    placeholder="0.000000"
                    step="0.000001"
                    min="0"
                    prop:value=move || amount.get()
                    on:input=move |ev| amount.set(event_target_value(&ev))
                    disabled=move || sending.get()
                />
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
    let to_pubkey   = RwSignal::new(String::new()); // empty = self-lock
    let lock_amount = RwSignal::new(String::new());
    let lock_date   = RwSignal::new(String::new());
    let lock_memo   = RwSignal::new(String::new());
    let locking     = RwSignal::new(false);
    let lock_msg    = RwSignal::new(String::new());
    let scan_msg    = RwSignal::new(String::new());

    // Quick-date helpers: set the date input value.
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

        if amt_str.is_empty() {
            lock_msg.set("Error: enter an amount.".into());
            return;
        }
        if date_str.is_empty() {
            lock_msg.set("Error: choose an unlock date.".into());
            return;
        }
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

            // Tauri v2 expects camelCase arg keys.
            let args = serde_wasm_bindgen::to_value(&serde_json::json!({
                "amountKx": amt,
                "unlockAtUnix": unlock_unix,
                "memo": memo,
                "toPubkeyHex": to_pubkey_hex,
            })).unwrap_or(no_args());

            match call::<String>("create_timelock", args).await {
                Ok(txid) => {
                    lock_msg.set(format!("Promise made! ID: {txid}"));
                    lock_amount.set(String::new());
                    lock_date.set(String::new());
                    lock_memo.set(String::new());
                    to_pubkey.set(String::new());
                    // Refresh balance.
                    if let Ok(a) = call::<AccountInfo>("get_account_info", no_args()).await {
                        info.set(Some(a));
                    }
                }
                Err(e) => lock_msg.set(format!("Error: {e}")),
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
                    <input
                        type="text"
                        placeholder="Leave blank for self · paste pubkey hex · or scan QR\u{2026}"
                        style="flex:1"
                        prop:value=move || to_pubkey.get()
                        on:input=move |ev| to_pubkey.set(event_target_value(&ev))
                        disabled=move || locking.get()
                    />
                    <button type="button" style="white-space:nowrap" on:click=on_scan_qr
                        disabled=move || locking.get()>
                        "📷 Scan QR"
                    </button>
                </div>
                {move || {
                    let s = scan_msg.get();
                    if s.is_empty() { view! { <span></span> }.into_any() }
                    else {
                        let cls = if s.starts_with("Scan failed") || s.starts_with("No file") { "msg error" }
                                  else { "msg success" };
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
                <input
                    type="number"
                    placeholder="0.000000"
                    step="0.000001"
                    min="0"
                    prop:value=move || lock_amount.get()
                    on:input=move |ev| lock_amount.set(event_target_value(&ev))
                    disabled=move || locking.get()
                />
            </div>

            <div class="field">
                <label>"Unlock Date (UTC)"</label>
                <input
                    type="date"
                    min=today
                    prop:value=move || lock_date.get()
                    on:input=move |ev| lock_date.set(event_target_value(&ev))
                    disabled=move || locking.get()
                />
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
                <textarea
                    placeholder="e.g. College fund for Maya — do not touch until 2040"
                    maxlength="256"
                    rows="3"
                    prop:value=move || lock_memo.get()
                    on:input=move |ev| lock_memo.set(event_target_value(&ev))
                    disabled=move || locking.get()
                ></textarea>
            </div>

            <button class="primary danger" on:click=on_lock disabled=move || locking.get()>
                {move || if locking.get() { "Promising\u{2026}" } else { "Make a Promise" }}
            </button>

            <p class="lock-warning">
                "\u{26A0} Promised funds cannot be recovered before the unlock date. "
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
    let timelocks   = RwSignal::new(Vec::<TimeLockInfo>::new());
    let tl_loading  = RwSignal::new(false);
    let tl_err      = RwSignal::new(String::new());
    let claim_msg   = RwSignal::new(String::new());

    // Load promises on mount.
    Effect::new(move |_| {
        spawn_local(async move {
            tl_loading.set(true);
            tl_err.set(String::new());
            match call::<Vec<TimeLockInfo>>("get_timelocks", no_args()).await {
                Ok(locks) => timelocks.set(locks),
                Err(e)    => tl_err.set(e),
            }
            tl_loading.set(false);
        });
    });

    let on_refresh = move |_: web_sys::MouseEvent| {
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
                                "On-chain promise indexing is coming soon. "
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
                                } else {
                                    lock.status.clone()
                                };
                                let unlock_date = {
                                    let d = js_sys::Date::new(
                                        &wasm_bindgen::JsValue::from_f64(lock.unlock_at as f64 * 1000.0)
                                    );
                                    format!("{:04}-{:02}-{:02}",
                                        d.get_utc_full_year(),
                                        d.get_utc_month() + 1,
                                        d.get_utc_date())
                                };
                                let lock_id = lock.lock_id.clone();
                                let can_claim = matured && lock.status == "Pending";
                                let on_claim = {
                                    let lid = lock_id.clone();
                                    move |_: web_sys::MouseEvent| {
                                        let lid2 = lid.clone();
                                        spawn_local(async move {
                                            claim_msg.set("Mining PoW\u{2026}".into());
                                            // Tauri v2 camelCase
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
                                            {lock.memo.clone().map(|m| view! {
                                                <span class="tl-memo">{m}</span>
                                            })}
                                        </div>
                                        <div class="tl-right">
                                            <span class=status_cls>{status_label}</span>
                                            {if can_claim {
                                                view! {
                                                    <button class="claim-btn" on:click=on_claim>
                                                        "Claim"
                                                    </button>
                                                }.into_any()
                                            } else {
                                                view! { <span></span> }.into_any()
                                            }}
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

// ── SettingsPanel ─────────────────────────────────────────────────────────────

#[component]
fn SettingsPanel(online: RwSignal<bool>) -> impl IntoView {
    let node_url    = RwSignal::new(String::new());
    let save_msg    = RwSignal::new(String::new());
    let pubkey_hex  = RwSignal::new(String::new());
    let pk_loading  = RwSignal::new(false);

    // Load current URL on mount.
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
            let args = serde_wasm_bindgen::to_value(
                &serde_json::json!({ "url": url })
            ).unwrap_or(no_args());
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

    view! {
        <div class="card">
            <p class="section-title">"Settings"</p>

            <div class="field">
                <label>"Node URL"</label>
                <input
                    type="text"
                    placeholder="http://127.0.0.1:8545"
                    prop:value=move || node_url.get()
                    on:input=move |ev| node_url.set(event_target_value(&ev))
                />
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

            <div style="margin-top:8px;border-top:1px solid #1e2130;padding-top:16px">
                <p class="label">"My Public Key (share with others so they can promise KX to you)"</p>
                <button on:click=on_show_pubkey disabled=move || pk_loading.get()>
                    {move || if pk_loading.get() { "Loading\u{2026}" } else { "Show Public Key" }}
                </button>
                {move || {
                    let pk = pubkey_hex.get();
                    if pk.is_empty() { view! { <span></span> }.into_any() }
                    else { view! { <p class="mono" style="font-size:10px;word-break:break-all;margin-top:8px">{pk}</p> }.into_any() }
                }}
            </div>
        </div>
    }
}
