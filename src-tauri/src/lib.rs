// ╔══════════════════════════════════════════════════════════════════════════════╗
// ║  PLATFORM ENTRY POINT — ChronX Wallet (Tauri v2)                           ║
// ║                                                                            ║
// ║  This is the native app shell. Compiles per-platform:                      ║
// ║    Desktop (Windows/macOS/Linux) — #[cfg(desktop)]                         ║
// ║    Mobile  (Android/iOS)         — #[cfg(mobile)]                          ║
// ║                                                                            ║
// ║  Desktop-only: single-instance plugin (prevents duplicate windows)         ║
// ║  Desktop-only: deep-link scheme registration (runtime)                     ║
// ║  Mobile:       deep links handled by OS (Intent filters / Universal Links) ║
// ║                                                                            ║
// ║  All Tauri commands are registered for ALL platforms. UI gating in the      ║
// ║  WASM frontend (lib.rs) controls which features are shown.                 ║
// ╚══════════════════════════════════════════════════════════════════════════════╝

mod commands;
mod contacts;

use std::sync::Mutex;
use tauri::Emitter;
use tauri::Manager;
use tauri_plugin_deep_link::DeepLinkExt;

/// Managed state: holds the raw deep-link URL from the Android launch Intent
/// (cold start). The frontend reads and clears it after PIN unlock.
pub struct PendingDeepLink(pub Mutex<Option<String>>);

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let mut builder = tauri::Builder::default();

    #[cfg(desktop)]
    {
        builder = builder.plugin(
            tauri_plugin_single_instance::init(|_app, _argv, _cwd| {
                // The deep-link feature on single-instance handles forwarding URLs
            })
        );
    }

    builder
        .plugin(tauri_plugin_deep_link::init())
        .plugin(tauri_plugin_opener::init())
        .manage(PendingDeepLink(Mutex::new(None)))
        .invoke_handler(tauri::generate_handler![
            commands::check_node,
            commands::get_account_info,
            commands::send_transfer,
            commands::create_timelock,
            commands::get_timelocks,
            commands::claim_timelock,
            commands::cancel_timelock,
            commands::create_email_timelock,
            commands::export_public_key,
            commands::get_node_url,
            commands::set_node_url,
            commands::generate_wallet,
            commands::generate_wallet_with_mnemonic,
            commands::import_wallet_from_mnemonic,
            commands::get_mnemonic,
            commands::get_pending_incoming,
            commands::get_all_promises,
            commands::check_pin_set,
            commands::set_pin,
            commands::verify_pin,
            commands::get_transaction_history,
            commands::get_app_version,
            commands::export_secret_key,
            commands::restore_wallet,
            commands::open_url,
            commands::check_for_updates,
            commands::fetch_notices,
            commands::get_seen_notices,
            commands::mark_notice_seen,
            commands::mark_notice_dismissed,
            commands::notify_email_recipient,
            commands::register_for_rewards,
            commands::check_rewards_status,
            commands::get_claim_email,
            commands::set_claim_email,
            commands::save_email_send,
            commands::check_email_timelocks,
            commands::claim_email_timelock,
            commands::claim_by_code,
            commands::get_claim_emails,
            commands::set_claim_emails,
            commands::create_email_timelock_series,
            commands::claim_email_series,
            commands::cancel_timelock_series,
            commands::reclaim_expired_lock,
            commands::get_launch_deep_link,
            commands::get_pin_length,
            commands::set_pin_length,
            commands::generate_cold_wallet,
            commands::save_cold_wallet,
            commands::get_cold_wallets,
            commands::get_trusted_contacts,
            commands::add_trusted_contact,
            commands::remove_trusted_contact,
            commands::is_trusted_contact,
            commands::get_pending_pokes,
            commands::send_poke_request,
            commands::decline_poke,
            commands::confirm_poke_paid,
            commands::get_language,
            commands::set_language,
            commands::get_verified_emails,
            commands::send_verify_email,
            commands::confirm_verify_email,
            commands::get_blocked_senders,
            commands::add_blocked_sender,
            commands::is_sender_blocked,
            commands::get_poke_by_id,
            commands::get_base_address,
            commands::set_base_address,
            commands::get_base_address_nickname,
            commands::get_base_addresses,
            commands::add_base_address,
            commands::delete_base_address,
            commands::convert_kx_to_usdc,
            commands::get_axiom_consent_hash,
            commands::create_freeform_timelock,
            commands::get_claim_info,
            commands::whitelist_email,
            contacts::get_contacts,
            contacts::search_contacts,
            contacts::add_contact,
            contacts::update_contact,
            contacts::delete_contact,
            contacts::record_send_to_contact,
            contacts::check_if_contact,
            commands::upload_avatar,
            commands::upload_avatar_bytes,
            commands::create_loan_offer,
            commands::accept_loan_offer,
            commands::decline_loan_offer,
            commands::withdraw_loan_offer,
            commands::get_wallet_loans,
            commands::get_loan_offers,
            commands::get_loan_nicknames,
            commands::set_loan_nickname,
            commands::get_loan_contacts,
            commands::set_loan_contact,
            commands::get_wallet_label,
            commands::get_loan_summary,
            commands::get_avatar_meta,
            commands::update_display_name,
            commands::get_sender_info,
            // Genesis 8 — new transaction types
            commands::get_open_invoices,
            commands::get_invoice,
            commands::create_invoice,
            commands::cancel_invoice,
            commands::get_open_credits,
            commands::create_credit,
            commands::draw_credit,
            commands::revoke_credit,
            commands::get_active_deposits,
            commands::create_deposit,
            commands::settle_deposit,
            commands::get_pending_conditionals,
            commands::create_conditional,
            commands::attest_conditional,
            commands::create_ledger_entry,
            commands::get_sign_of_life_status,
            commands::submit_sign_of_life,
            commands::get_promises_needing_sign_of_life,
            // v2.2.2 — Verified Identity + KXGO Badges + Commitments
            commands::get_verified_identity,
            commands::get_wallet_badges,
            commands::get_commitments,
            commands::cancel_commitment,
            // v2.3.7 — Biometric login + Forgot PIN
            commands::get_auth_method,
            commands::set_auth_method,
            commands::authenticate_biometric,
            commands::check_biometric_available,
            commands::reset_pin_with_mnemonic,
            commands::reset_pin_with_key,
            // v2.4.0 — Invoice rejection
            commands::get_pending_invoices,
            commands::reject_invoice,
            // v2.4.1 — Address Book + KX Requests
            commands::get_address_book,
            commands::add_to_address_book,
            commands::remove_from_address_book,
            commands::check_email_registered,
            commands::send_kx_request,
            commands::get_pending_kx_requests,
            commands::decline_kx_request,
            commands::block_kx_sender,
            commands::unblock_kx_sender,
            commands::get_request_permission,
            commands::set_request_permission,
            commands::get_show_badges,
            commands::set_show_badges,
            commands::get_show_identity,
            commands::set_show_identity,
        ])
        .setup(|app| {
            #[cfg(any(windows, target_os = "linux"))]
            {
                let _ = app.deep_link().register_all();
            }

            // ── Cold start: read the launch Intent URL via get_current() ──────
            // on_open_url does NOT fire on Android cold start, so we must
            // retrieve the initial URL here and store it in managed state.
            if let Ok(Some(urls)) = app.deep_link().get_current() {
                if let Some(url) = urls.first() {
                    let url_str = url.to_string();
                    if !url_str.is_empty() {
                        let state = app.state::<PendingDeepLink>();
                        let mut pending = state.0.lock().unwrap();
                        *pending = Some(url_str);
                    }
                }
            }

            // ── Warm start: on_open_url fires when app is already running ─────
            let handle = app.handle().clone();
            app.deep_link().on_open_url(move |event| {
                for url in event.urls() {
                    let url_str = url.to_string();

                    // Also update managed state (in case frontend polls before event fires)
                    if let Some(state) = handle.try_state::<PendingDeepLink>() {
                        if let Ok(mut pending) = state.0.lock() {
                            *pending = Some(url_str.clone());
                        }
                    }

                    // Route 1: PAY
                    if url_str.starts_with("chronx://pay") || url_str.starts_with("chronx://poke/pay") {
                        let normalized = if url_str.starts_with("chronx://poke/") {
                            url_str.clone()
                        } else {
                            url_str.replacen("chronx://pay", "chronx://poke/pay", 1)
                        };
                        let _ = handle.emit("deep-link-poke", &normalized);
                    }
                    // Route 2: DECLINE
                    else if url_str.starts_with("chronx://decline") || url_str.starts_with("chronx://poke/decline") {
                        let normalized = if url_str.starts_with("chronx://poke/") {
                            url_str.clone()
                        } else {
                            url_str.replacen("chronx://decline", "chronx://poke/decline", 1)
                        };
                        let _ = handle.emit("deep-link-poke", &normalized);
                    }
                    // Route 3: CLAIM
                    else if url_str.starts_with("chronx://claim") {
                        if let Some(code) = url_str
                            .split("code=")
                            .nth(1)
                            .map(|c| c.split('&').next().unwrap_or(c))
                        {
                            let code = code
                                .replace("%20", " ")
                                .replace("+", " ")
                                .replace("%2D", "-")
                                .replace("%2d", "-");
                            let _ = handle.emit("deep-link-claim", &code);
                        }
                    }
                }
            });

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running ChronX Wallet");
}
