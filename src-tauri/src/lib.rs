mod commands;

use tauri::Emitter;
use tauri_plugin_deep_link::DeepLinkExt;

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
            commands::get_pending_incoming,
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
            commands::get_pending_deep_link,
            commands::get_pin_length,
            commands::set_pin_length,
            commands::generate_cold_wallet,
            commands::save_cold_wallet,
            commands::get_cold_wallets,
        ])
        .setup(|app| {
            #[cfg(any(windows, target_os = "linux"))]
            {
                let _ = app.deep_link().register_all();
            }

            // Listen for deep link URLs and store claim code for frontend
            let handle = app.handle().clone();
            app.deep_link().on_open_url(move |event| {
                for url in event.urls() {
                    let url_str = url.to_string();
                    // Parse chronx://claim?code=KX-XXXX-...
                    if url_str.starts_with("chronx://claim") {
                        if let Some(code) = url_str
                            .split("code=")
                            .nth(1)
                            .map(|c| c.split('&').next().unwrap_or(c))
                        {
                            // Basic URL decode for the claim code
                            let code = code
                                .replace("%20", " ")
                                .replace("+", " ")
                                .replace("%2D", "-")
                                .replace("%2d", "-");
                            // Store in file for frontend to pick up via get_pending_deep_link
                            let home = std::env::var("USERPROFILE")
                                .or_else(|_| std::env::var("HOME"))
                                .unwrap_or_else(|_| ".".to_string());
                            let dir = std::path::PathBuf::from(home).join(".chronx");
                            let _ = std::fs::create_dir_all(&dir);
                            let _ = std::fs::write(dir.join("pending-deep-link.txt"), &code);
                            // Also emit event to frontend
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
