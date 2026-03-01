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
        ])
        .run(tauri::generate_context!())
        .expect("error while running ChronX Wallet");
}
