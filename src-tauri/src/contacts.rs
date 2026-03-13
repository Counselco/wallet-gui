// ── Address Book — local contact storage ─────────────────────────────────────
//
// contacts.json lives alongside wallet.json — never synced to any server.
// Desktop: ~/.chronx/contacts.json
// Mobile:  app_data_dir()/contacts.json

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tauri::AppHandle;
#[cfg(mobile)]
use tauri::Manager;

// ── Data types ──────────────────────────────────────────────────────────────

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Contact {
    pub id: String,
    pub name: String,
    pub email: Option<String>,
    pub kx_address: Option<String>,
    pub notes: Option<String>,
    pub last_sent: Option<i64>,
    pub send_count: u32,
    pub created_at: i64,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct ContactBook {
    pub contacts: Vec<Contact>,
}

// ── Path helpers ────────────────────────────────────────────────────────────

fn contacts_path(app: &AppHandle) -> PathBuf {
    #[cfg(mobile)]
    {
        app.path()
            .app_data_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join("contacts.json")
    }
    #[cfg(not(mobile))]
    {
        let _ = app;
        let home = std::env::var("USERPROFILE")
            .or_else(|_| std::env::var("HOME"))
            .unwrap_or_else(|_| ".".to_string());
        PathBuf::from(home).join(".chronx").join("contacts.json")
    }
}

fn generate_id() -> String {
    use rand::Rng;
    let mut rng = rand::thread_rng();
    let b: [u8; 16] = rng.gen();
    format!(
        "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7],
        b[8], b[9], b[10], b[11], b[12], b[13], b[14], b[15]
    )
}

// ── ContactBook impl ────────────────────────────────────────────────────────

impl ContactBook {
    pub fn load(app: &AppHandle) -> Self {
        let path = contacts_path(app);
        std::fs::read_to_string(&path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    pub fn save(&self, app: &AppHandle) -> Result<(), String> {
        let path = contacts_path(app);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| format!("Creating dir: {e}"))?;
        }
        let json = serde_json::to_string_pretty(self).map_err(|e| e.to_string())?;
        std::fs::write(&path, json).map_err(|e| format!("Writing contacts: {e}"))
    }

    pub fn add(&mut self, name: String, email: Option<String>, kx_address: Option<String>, notes: Option<String>) -> Contact {
        let contact = Contact {
            id: generate_id(),
            name,
            email,
            kx_address,
            notes,
            last_sent: None,
            send_count: 0,
            created_at: chrono::Utc::now().timestamp(),
        };
        self.contacts.push(contact.clone());
        contact
    }

    pub fn update(&mut self, id: &str, name: String, email: Option<String>, kx_address: Option<String>, notes: Option<String>) -> Result<(), String> {
        let c = self.contacts.iter_mut().find(|c| c.id == id)
            .ok_or_else(|| format!("Contact {} not found", id))?;
        c.name = name;
        c.email = email;
        c.kx_address = kx_address;
        c.notes = notes;
        Ok(())
    }

    pub fn delete(&mut self, id: &str) -> Result<(), String> {
        let len = self.contacts.len();
        self.contacts.retain(|c| c.id != id);
        if self.contacts.len() == len {
            Err(format!("Contact {} not found", id))
        } else {
            Ok(())
        }
    }

    pub fn search(&self, query: &str) -> Vec<Contact> {
        let q = query.to_lowercase();
        let mut results: Vec<Contact> = self.contacts.iter()
            .filter(|c| {
                c.name.to_lowercase().contains(&q)
                || c.email.as_ref().map(|e| e.to_lowercase().contains(&q)).unwrap_or(false)
                || c.kx_address.as_ref().map(|a| a.to_lowercase().contains(&q)).unwrap_or(false)
            })
            .cloned()
            .collect();
        results.sort_by(|a, b| b.send_count.cmp(&a.send_count));
        results
    }

    pub fn find_by_email(&self, email: &str) -> Option<&Contact> {
        let e = email.to_lowercase();
        self.contacts.iter().find(|c|
            c.email.as_ref().map(|ce| ce.to_lowercase() == e).unwrap_or(false)
        )
    }

    pub fn find_by_kx_address(&self, address: &str) -> Option<&Contact> {
        self.contacts.iter().find(|c|
            c.kx_address.as_ref().map(|a| a == address).unwrap_or(false)
        )
    }

    pub fn record_send(&mut self, id: &str, kx_address: Option<String>) {
        if let Some(c) = self.contacts.iter_mut().find(|c| c.id == id) {
            c.send_count += 1;
            c.last_sent = Some(chrono::Utc::now().timestamp());
            if let Some(addr) = kx_address {
                if c.kx_address.is_none() {
                    c.kx_address = Some(addr);
                }
            }
        }
    }
}

// ── Tauri commands ──────────────────────────────────────────────────────────

#[tauri::command]
pub async fn get_contacts(app: AppHandle) -> Result<Vec<Contact>, String> {
    let book = ContactBook::load(&app);
    let mut contacts = book.contacts;
    contacts.sort_by(|a, b| b.send_count.cmp(&a.send_count));
    Ok(contacts)
}

#[tauri::command]
pub async fn search_contacts(app: AppHandle, query: String) -> Result<Vec<Contact>, String> {
    let book = ContactBook::load(&app);
    Ok(book.search(&query))
}

#[tauri::command]
pub async fn add_contact(
    app: AppHandle,
    name: String,
    email: Option<String>,
    kx_address: Option<String>,
    notes: Option<String>,
) -> Result<Contact, String> {
    if email.is_none() && kx_address.is_none() {
        return Err("Contact must have at least an email or KX address".into());
    }
    if let Some(ref e) = email {
        if !e.contains('@') || !e.contains('.') {
            return Err("Invalid email address".into());
        }
    }
    if let Some(ref a) = kx_address {
        if a.len() < 32 || a.len() > 50 {
            return Err("Invalid KX address length".into());
        }
    }
    let mut book = ContactBook::load(&app);
    let contact = book.add(name, email, kx_address, notes);
    book.save(&app)?;
    Ok(contact)
}

#[tauri::command]
pub async fn update_contact(
    app: AppHandle,
    id: String,
    name: String,
    email: Option<String>,
    kx_address: Option<String>,
    notes: Option<String>,
) -> Result<Contact, String> {
    if email.is_none() && kx_address.is_none() {
        return Err("Contact must have at least an email or KX address".into());
    }
    let mut book = ContactBook::load(&app);
    book.update(&id, name, email, kx_address, notes)?;
    book.save(&app)?;
    let contact = book.contacts.iter().find(|c| c.id == id).cloned()
        .ok_or("Contact not found after update")?;
    Ok(contact)
}

#[tauri::command]
pub async fn delete_contact(app: AppHandle, id: String) -> Result<(), String> {
    let mut book = ContactBook::load(&app);
    book.delete(&id)?;
    book.save(&app)
}

#[tauri::command]
pub async fn record_send_to_contact(
    app: AppHandle,
    id: String,
    kx_address: Option<String>,
) -> Result<(), String> {
    let mut book = ContactBook::load(&app);
    book.record_send(&id, kx_address);
    book.save(&app)
}

#[tauri::command]
pub async fn check_if_contact(
    app: AppHandle,
    email: Option<String>,
    kx_address: Option<String>,
) -> Result<Option<Contact>, String> {
    let book = ContactBook::load(&app);
    if let Some(ref e) = email {
        if let Some(c) = book.find_by_email(e) {
            return Ok(Some(c.clone()));
        }
    }
    if let Some(ref a) = kx_address {
        if let Some(c) = book.find_by_kx_address(a) {
            return Ok(Some(c.clone()));
        }
    }
    Ok(None)
}
