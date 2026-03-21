use serde::Serialize;

#[derive(Serialize)]
pub struct SystemResources {
    pub cpu_cores: u32,
    pub memory_gb: f64,
    pub available_memory_gb: f64,
}

#[tauri::command]
pub fn get_system_resources() -> Result<SystemResources, String> {
    use sysinfo::System;
    let mut sys = System::new();
    sys.refresh_memory();

    let cpu_cores = std::thread::available_parallelism()
        .map(|n| n.get() as u32)
        .unwrap_or(4);
    let total_mem = sys.total_memory() as f64 / (1024.0 * 1024.0 * 1024.0);
    let available_mem = sys.available_memory() as f64 / (1024.0 * 1024.0 * 1024.0);

    Ok(SystemResources {
        cpu_cores,
        memory_gb: (total_mem * 100.0).round() / 100.0,
        available_memory_gb: (available_mem * 100.0).round() / 100.0,
    })
}
