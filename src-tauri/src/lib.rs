mod commands;

pub fn run() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            commands::check_node,
            commands::get_account_info,
            commands::send_transfer,
        ])
        .run(tauri::generate_context!())
        .expect("error while running ChronX Wallet");
}
