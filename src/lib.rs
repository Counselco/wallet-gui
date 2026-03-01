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
    balance_kx: String,
    #[allow(dead_code)]
    balance_chronos: String,
    spendable_kx: String,
    #[allow(dead_code)]
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
    let active_tab  = RwSignal::new(0u8); // 0=Account 1=Send 2=Lock 3=Timelocks 4=Settings

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
                                    on:click=move |_| active_tab.set(2)>"🔒 Lock"</button>
                                <button class=move || if active_tab.get()==3 {"tab active"} else {"tab"}
                                    on:click=move |_| active_tab.set(3)>"📋 Locks"</button>
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
                            2 => view! { <LockPanel info=info /> }.into_any(),
                            3 => view! { <TimelocksPanel /> }.into_any(),
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

    // Load pending incoming locks on mount.
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
            </div>

            <div class="row">
                <div>
                    <p class="label">"Balance"</p>
                    <p class="balance">
                        {move || {
                            if loading.get() { "\u{2026}".into() }
                            else {
                                info.get()
                                    .map(|a| format!("{} KX", a.balance_kx))
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
                                    .map(|a| format!("{} KX", a.spendable_kx))
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

            // ── Pending incoming locks ────────────────────────────────────────
            {move || {
                let locks = incoming.get();
                if inc_loading.get() {
                    view! { <p class="muted" style="margin-top:12px">"Checking incoming locks\u{2026}"</p> }.into_any()
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
                                    {lock.amount_kx} " KX"
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
                            <p class="label">"Incoming Pending Locks"</p>
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
    let to_addr = RwSignal::new(String::new());
    let amount  = RwSignal::new(String::new());
    let sending = RwSignal::new(false);
    let tx_msg  = RwSignal::new(String::new());

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

            let args = serde_wasm_bindgen::to_value(
                &serde_json::json!({ "to": to, "amount_kx": amt })
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
                <input
                    type="text"
                    placeholder="Base-58 address\u{2026}"
                    prop:value=move || to_addr.get()
                    on:input=move |ev| to_addr.set(event_target_value(&ev))
                    disabled=move || sending.get()
                />
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

// ── LockPanel ─────────────────────────────────────────────────────────────────

#[component]
fn LockPanel(info: RwSignal<Option<AccountInfo>>) -> impl IntoView {
    let lock_amount = RwSignal::new(String::new());
    let lock_date   = RwSignal::new(String::new());
    let lock_memo   = RwSignal::new(String::new());
    let locking     = RwSignal::new(false);
    let lock_msg    = RwSignal::new(String::new());

    // Quick-date helpers: set the date input value.
    let set_date = move |date: String| lock_date.set(date);

    let on_lock = move |_: web_sys::MouseEvent| {
        let amt_str = lock_amount.get_untracked();
        let date_str = lock_date.get_untracked();
        let memo_str = lock_memo.get_untracked();

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

        spawn_local(async move {
            locking.set(true);
            lock_msg.set("Mining PoW\u{2026} (~10s)".into());

            let args = serde_wasm_bindgen::to_value(&serde_json::json!({
                "amount_kx": amt,
                "unlock_at_unix": unlock_unix,
                "memo": memo
            })).unwrap_or(no_args());

            match call::<String>("create_timelock", args).await {
                Ok(txid) => {
                    lock_msg.set(format!("Locked! Lock ID: {txid}"));
                    lock_amount.set(String::new());
                    lock_date.set(String::new());
                    lock_memo.set(String::new());
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
            <p class="section-title">"Lock Funds"</p>
            <p class="label">"Locking to: yourself (self-commitment)"</p>

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
                {move || if locking.get() { "Locking\u{2026}" } else { "Lock Funds — Irrevocable" }}
            </button>

            <p class="lock-warning">
                "\u{26A0} Locked funds cannot be recovered before the unlock date. "
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

// ── TimelocksPanel ────────────────────────────────────────────────────────────

#[component]
fn TimelocksPanel() -> impl IntoView {
    let timelocks   = RwSignal::new(Vec::<TimeLockInfo>::new());
    let tl_loading  = RwSignal::new(false);
    let tl_err      = RwSignal::new(String::new());
    let claim_msg   = RwSignal::new(String::new());

    // Load timelocks on mount.
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
                <p class="section-title">"My Timelocks"</p>
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
                            <p>"No timelocks found."</p>
                            <p class="muted">
                                "On-chain timelock indexing is coming soon. "
                                "Locks you create will appear here once the node supports full scanning."
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
                                            let args = serde_wasm_bindgen::to_value(
                                                &serde_json::json!({ "lock_id_hex": lid2 })
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
                                            <span class="tl-amount">{lock.amount_kx.clone()} " KX"</span>
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
                <p class="label">"My Public Key (share with others to receive timelocks)"</p>
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
