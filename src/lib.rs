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
//
// In Tauri v2 the low-level IPC lives at window.__TAURI_INTERNALS__.invoke,
// NOT at window.__TAURI__.core.invoke (which requires the bundled JS API).

#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(js_namespace = ["window", "__TAURI_INTERNALS__"])]
    fn invoke(cmd: &str, args: JsValue) -> Promise;
}

/// Shorthand: call a Tauri command and deserialise the result.
async fn call<T: serde::de::DeserializeOwned>(
    cmd: &str,
    args: JsValue,
) -> Result<T, String> {
    JsFuture::from(invoke(cmd, args))
        .await
        .map_err(|e| e.as_string().unwrap_or_else(|| format!("{e:?}")))
        .and_then(|v| serde_wasm_bindgen::from_value(v).map_err(|e| e.to_string()))
}

/// Empty JS object `{}` — required for Tauri v2 commands that take no args.
fn no_args() -> JsValue {
    js_sys::Object::new().into()
}

// ── Shared types (mirror the Tauri backend) ───────────────────────────────────

#[derive(Clone, Deserialize, Default)]
struct AccountInfo {
    account_id: String,
    balance_kx: String,
    #[allow(dead_code)]
    balance_chronos: String,
    #[allow(dead_code)]
    nonce: u64,
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
    let info    = RwSignal::new(Option::<AccountInfo>::None);
    let loading = RwSignal::new(false);
    let err_msg = RwSignal::new(String::new());
    let online  = RwSignal::new(false);
    let tx_msg  = RwSignal::new(String::new());
    let to_addr = RwSignal::new(String::new());
    let amount  = RwSignal::new(String::new());
    let sending = RwSignal::new(false);

    // ── Helpers ───────────────────────────────────────────────────────────────

    async fn refresh_data(
        online: RwSignal<bool>,
        loading: RwSignal<bool>,
        err_msg: RwSignal<String>,
        info: RwSignal<Option<AccountInfo>>,
    ) {
        online.set(call::<bool>("check_node", no_args()).await.unwrap_or(false));
        loading.set(true);
        err_msg.set(String::new());
        match call::<AccountInfo>("get_account_info", no_args()).await {
            Ok(a)  => { info.set(Some(a)); }
            Err(e) => { err_msg.set(e); }
        }
        loading.set(false);
    }

    // Auto-load on mount.
    Effect::new(move |_| {
        spawn_local(async move {
            refresh_data(online, loading, err_msg, info).await;
        });
    });

    // ── Refresh handler ───────────────────────────────────────────────────────

    let on_refresh = move |_: web_sys::MouseEvent| {
        spawn_local(async move {
            refresh_data(online, loading, err_msg, info).await;
        });
    };

    // ── Send handler ──────────────────────────────────────────────────────────

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
                    // Refresh balance.
                    if let Ok(a) = call::<AccountInfo>("get_account_info", no_args()).await {
                        info.set(Some(a));
                    }
                }
                Err(e) => tx_msg.set(format!("Error: {e}")),
            }
            sending.set(false);
        });
    };

    // ── View ──────────────────────────────────────────────────────────────────

    view! {
        <div class="app">

            <header>
                <a href="https://www.chronx.io" target="_blank" rel="noopener" class="logo-link">
                    <img src=logo_src() alt="ChronX Logo" style="height:48px;width:auto;display:block;" />
                </a>
                <span class="node-status">
                    <span class=move || if online.get() { "dot online" } else { "dot offline" }></span>
                    {move || if online.get() { "Online" } else { "Offline" }}
                </span>
            </header>

            // ── Account / balance card ────────────────────────────────────
            <div class="card">
                <p class="label">"Account ID"</p>
                <p class="mono">
                    {move || info.get().map(|a| a.account_id)
                        .unwrap_or_else(|| "\u{2014}".into())}
                </p>

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
            </div>

            // ── Send card ─────────────────────────────────────────────────
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

        </div>
    }
}
