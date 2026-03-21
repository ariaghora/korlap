use crate::state::AppState;
use std::sync::{Arc, Mutex};
use tauri::State;
use uuid::Uuid;

// ── Repo Settings ─────────────────────────────────────────────────────

#[tauri::command]
pub fn get_repo_settings(
    repo_id: String,
    state: State<'_, Arc<Mutex<AppState>>>,
) -> Result<crate::state::RepoSettings, String> {
    let st = state.lock().map_err(|e| e.to_string())?;
    Ok(st
        .repo_settings
        .get(&repo_id)
        .cloned()
        .unwrap_or_default())
}

#[tauri::command]
pub fn save_repo_settings(
    repo_id: String,
    settings: crate::state::RepoSettings,
    state: State<'_, Arc<Mutex<AppState>>>,
) -> Result<(), String> {
    let mut st = state.lock().map_err(|e| e.to_string())?;
    st.repo_settings.insert(repo_id, settings);
    st.save_repo_settings()?;
    Ok(())
}

// ── Message persistence ──────────────────────────────────────────────

#[tauri::command]
pub fn save_messages(
    workspace_id: String,
    messages: serde_json::Value,
    state: State<'_, Arc<Mutex<AppState>>>,
) -> Result<(), String> {
    let msg_dir = {
        let st = state.lock().map_err(|e| e.to_string())?;
        st.messages_dir()
    };
    std::fs::create_dir_all(&msg_dir).map_err(|e| e.to_string())?;
    let msg_file = msg_dir.join(format!("{}.json", workspace_id));
    let data = serde_json::to_string(&messages).map_err(|e| e.to_string())?;
    std::fs::write(&msg_file, data).map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub fn load_messages(
    workspace_id: String,
    state: State<'_, Arc<Mutex<AppState>>>,
) -> Result<serde_json::Value, String> {
    let msg_file = {
        let st = state.lock().map_err(|e| e.to_string())?;
        st.messages_dir().join(format!("{}.json", workspace_id))
    };

    if !msg_file.exists() {
        return Ok(serde_json::json!([]));
    }

    let data = std::fs::read_to_string(&msg_file).map_err(|e| e.to_string())?;
    serde_json::from_str(&data).map_err(|e| e.to_string())
}

// ── Todo persistence ─────────────────────────────────────────────────

#[tauri::command]
pub fn save_todos(
    repo_id: String,
    todos: serde_json::Value,
    state: State<'_, Arc<Mutex<AppState>>>,
) -> Result<(), String> {
    let todos_dir = {
        let st = state.lock().map_err(|e| e.to_string())?;
        st.todos_dir()
    };
    std::fs::create_dir_all(&todos_dir).map_err(|e| e.to_string())?;
    let todos_file = todos_dir.join(format!("{}.json", repo_id));
    let data = serde_json::to_string(&todos).map_err(|e| e.to_string())?;
    std::fs::write(&todos_file, data).map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub fn load_todos(
    repo_id: String,
    state: State<'_, Arc<Mutex<AppState>>>,
) -> Result<serde_json::Value, String> {
    let todos_file = {
        let st = state.lock().map_err(|e| e.to_string())?;
        st.todos_dir().join(format!("{}.json", repo_id))
    };

    if !todos_file.exists() {
        return Ok(serde_json::json!([]));
    }

    let data = std::fs::read_to_string(&todos_file).map_err(|e| e.to_string())?;
    serde_json::from_str(&data).map_err(|e| e.to_string())
}

// ── Image commands ───────────────────────────────────────────────────

/// Save base64-encoded image data to the app data directory.
/// Returns the absolute path to the saved image.
/// Images are stored under `<data_dir>/images/<workspace_id>/` — never in the worktree.
#[tauri::command]
pub fn save_image(
    workspace_id: String,
    data: String,
    extension: String,
    state: State<'_, Arc<Mutex<AppState>>>,
) -> Result<String, String> {
    let images_dir = {
        let st = state.lock().map_err(|e| e.to_string())?;
        st.data_dir.join("images").join(&workspace_id)
    };

    // Decode base64
    use base64::Engine;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(&data)
        .map_err(|e| format!("Invalid base64 data: {}", e))?;

    std::fs::create_dir_all(&images_dir)
        .map_err(|e| format!("Failed to create images dir: {}", e))?;

    let ext = if extension.is_empty() { "png" } else { &extension };
    let filename = format!("{}.{}", Uuid::new_v4(), ext);
    let file_path = images_dir.join(&filename);

    std::fs::write(&file_path, &bytes)
        .map_err(|e| format!("Failed to save image: {}", e))?;

    Ok(file_path.to_string_lossy().to_string())
}
