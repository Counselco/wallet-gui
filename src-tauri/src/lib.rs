mod commands;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
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
        ])
        .run(tauri::generate_context!())
        .expect("error while running ChronX Wallet");
}
