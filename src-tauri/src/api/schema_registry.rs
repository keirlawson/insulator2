use crate::lib::schema_registry::{SchemaRegistryClient, Subject};
use log::debug;

use super::{
    error::{Result, TauriError},
    AppState,
};

#[tauri::command]
pub async fn list_subjects(cluster_id: &str, state: tauri::State<'_, AppState>) -> Result<Vec<String>> {
    debug!("List schema schema registry subjects");
    let client = state.get_schema_reg_client(cluster_id).await.ok_or(TauriError {
        error_type: "Configuration error".into(),
        message: "Missing schema registry configuration".into(),
    })?;
    Ok(client.list_subjects().await?)
}

#[tauri::command]
pub async fn get_subject(subject_name: &str, cluster_id: &str, state: tauri::State<'_, AppState>) -> Result<Subject> {
    debug!("Retrieve all schema version for subject {}", subject_name);
    let client = state.get_schema_reg_client(cluster_id).await.ok_or(TauriError {
        error_type: "Configuration error".into(),
        message: "Missing schema registry configuration".into(),
    })?;
    Ok(client.get_subject(subject_name).await?)
}

#[tauri::command]
pub async fn delete_subject(subject_name: &str, cluster_id: &str, state: tauri::State<'_, AppState>) -> Result<()> {
    debug!("Deleting subject {}", subject_name);
    let client = state.get_schema_reg_client(cluster_id).await.ok_or(TauriError {
        error_type: "Configuration error".into(),
        message: "Missing schema registry configuration".into(),
    })?;
    Ok(client.delete_subject(subject_name).await?)
}

#[tauri::command]
pub async fn delete_subject_version(
    subject_name: &str,
    version: i32,
    cluster_id: &str,
    state: tauri::State<'_, AppState>,
) -> Result<()> {
    debug!("Deleting subject {} version {}", subject_name, version);
    let client = state.get_schema_reg_client(cluster_id).await.ok_or(TauriError {
        error_type: "Configuration error".into(),
        message: "Missing schema registry configuration".into(),
    })?;
    Ok(client.delete_version(subject_name, version).await?)
}
