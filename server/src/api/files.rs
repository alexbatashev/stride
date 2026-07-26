use std::sync::Arc;

use axum::{
    Json,
    extract::{Multipart, Path, Query, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};
use serde::{Deserialize, Serialize};

use crate::{
    ServerState,
    api::auth::{self, AuthError},
    vfs::{EntryKind, USER_HOME},
};

/// Manages the user's global files: nodes with no workspace, owned by the user.
/// The agent sees them mounted at `/home/user`; this API addresses them by the
/// same absolute paths, translating to the storage-relative form at the edge.

#[derive(Debug)]
pub enum FilesApiError {
    Auth(AuthError),
    BadRequest,
    NotFound,
    Internal,
}

impl IntoResponse for FilesApiError {
    fn into_response(self) -> Response {
        match self {
            FilesApiError::Auth(error) => error.into_response(),
            FilesApiError::BadRequest => StatusCode::BAD_REQUEST.into_response(),
            FilesApiError::NotFound => StatusCode::NOT_FOUND.into_response(),
            FilesApiError::Internal => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
        }
    }
}

impl From<AuthError> for FilesApiError {
    fn from(error: AuthError) -> Self {
        FilesApiError::Auth(error)
    }
}

#[derive(Deserialize)]
pub struct FilesQuery {
    path: Option<String>,
}

#[derive(Deserialize)]
pub struct DownloadQuery {
    version: Option<i64>,
}

#[derive(Deserialize)]
pub struct VersionsQuery {
    path: String,
}

#[derive(Deserialize)]
pub struct CreateDirectoryRequest {
    path: String,
}

#[derive(Deserialize)]
pub struct RenameRequest {
    path: String,
    name: String,
}

#[derive(Deserialize)]
pub struct RestoreVersionRequest {
    path: String,
    version: i64,
}

#[derive(Serialize)]
pub struct FileEntry {
    name: String,
    path: String,
    kind: &'static str,
    size: Option<i64>,
    updated_at: i64,
    mime_type: Option<String>,
}

#[derive(Serialize)]
pub struct FileListResponse {
    path: String,
    entries: Vec<FileEntry>,
}

#[derive(Serialize)]
pub struct FileVersionResponse {
    version: i64,
    size: i64,
    created_at: i64,
    mime_type: Option<String>,
}

#[derive(Serialize)]
pub struct FileVersionsResponse {
    path: String,
    versions: Vec<FileVersionResponse>,
}

#[derive(Serialize)]
pub struct UploadedFile {
    name: String,
    path: String,
    size: usize,
}

#[derive(Serialize)]
pub struct UploadResponse {
    files: Vec<UploadedFile>,
}

pub async fn list_files(
    State(state): State<Arc<ServerState>>,
    Query(query): Query<FilesQuery>,
    headers: HeaderMap,
) -> Result<Json<FileListResponse>, FilesApiError> {
    let Some(ref vfs) = state.vfs else {
        return Err(FilesApiError::NotFound);
    };
    let owner = auth::authenticated_user(&state, &headers).await?;
    let path = relative_path(query.path.as_deref());

    let entries = vfs
        .list_global(owner, &path)
        .await
        .map_err(|_| FilesApiError::NotFound)?
        .into_iter()
        .map(|entry| FileEntry {
            path: absolute(&join_path(&path, &entry.name)),
            kind: match entry.kind {
                EntryKind::Directory => "directory",
                EntryKind::File => "file",
            },
            size: entry.size,
            updated_at: entry.updated_at,
            mime_type: entry.mime_type,
            name: entry.name,
        })
        .collect();

    Ok(Json(FileListResponse {
        path: absolute(&path),
        entries,
    }))
}

pub async fn list_versions(
    State(state): State<Arc<ServerState>>,
    Query(query): Query<VersionsQuery>,
    headers: HeaderMap,
) -> Result<Json<FileVersionsResponse>, FilesApiError> {
    let Some(ref vfs) = state.vfs else {
        return Err(FilesApiError::NotFound);
    };
    let owner = auth::authenticated_user(&state, &headers).await?;
    let path = relative_path(Some(&query.path));
    if path.is_empty() {
        return Err(FilesApiError::BadRequest);
    }

    let versions = vfs
        .list_versions_global(owner, &path)
        .await
        .map_err(|_| FilesApiError::NotFound)?
        .into_iter()
        .map(|version| FileVersionResponse {
            version: version.version,
            size: version.size,
            created_at: version.created_at,
            mime_type: version.mime_type,
        })
        .collect();

    Ok(Json(FileVersionsResponse {
        path: absolute(&path),
        versions,
    }))
}

pub async fn restore_version(
    State(state): State<Arc<ServerState>>,
    headers: HeaderMap,
    Json(request): Json<RestoreVersionRequest>,
) -> Result<StatusCode, FilesApiError> {
    let Some(ref vfs) = state.vfs else {
        return Err(FilesApiError::NotFound);
    };
    let owner = auth::authenticated_user(&state, &headers).await?;
    let path = relative_path(Some(&request.path));
    if path.is_empty() || request.version < 0 {
        return Err(FilesApiError::BadRequest);
    }

    vfs.restore_version_global(owner, &path, request.version)
        .await
        .map_err(|_| FilesApiError::NotFound)?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn create_directory(
    State(state): State<Arc<ServerState>>,
    headers: HeaderMap,
    Json(request): Json<CreateDirectoryRequest>,
) -> Result<StatusCode, FilesApiError> {
    let Some(ref vfs) = state.vfs else {
        return Err(FilesApiError::NotFound);
    };
    let owner = auth::authenticated_user(&state, &headers).await?;
    let path = relative_path(Some(&request.path));
    if path.is_empty() {
        return Err(FilesApiError::BadRequest);
    }

    vfs.create_dir_global(owner, &path)
        .await
        .map_err(|_| FilesApiError::BadRequest)?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn rename(
    State(state): State<Arc<ServerState>>,
    headers: HeaderMap,
    Json(request): Json<RenameRequest>,
) -> Result<StatusCode, FilesApiError> {
    let Some(ref vfs) = state.vfs else {
        return Err(FilesApiError::NotFound);
    };
    let owner = auth::authenticated_user(&state, &headers).await?;
    let path = relative_path(Some(&request.path));
    let name = request.name.trim();
    if path.is_empty() || name.is_empty() || name.contains('/') {
        return Err(FilesApiError::BadRequest);
    }

    vfs.rename_global(owner, &path, name)
        .await
        .map_err(|_| FilesApiError::BadRequest)?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn upload_file(
    State(state): State<Arc<ServerState>>,
    Query(query): Query<FilesQuery>,
    headers: HeaderMap,
    mut multipart: Multipart,
) -> Result<Json<UploadResponse>, FilesApiError> {
    let Some(ref vfs) = state.vfs else {
        return Err(FilesApiError::Internal);
    };
    let owner = auth::authenticated_user(&state, &headers).await?;
    let directory = relative_path(query.path.as_deref());

    let mut uploaded = Vec::new();
    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|_| FilesApiError::BadRequest)?
    {
        let name = field
            .file_name()
            .filter(|n| !n.is_empty())
            .map(|n| n.to_string())
            .unwrap_or_else(|| "file".to_string());
        let mime_type = field
            .content_type()
            .filter(|ct| !ct.is_empty())
            .map(|ct| ct.to_string());
        let bytes = field.bytes().await.map_err(|_| FilesApiError::BadRequest)?;
        let size = bytes.len();
        let path = join_path(&directory, &name);

        vfs.write_bytes_global(owner, &path, &bytes, mime_type.as_deref())
            .await
            .map_err(|_| FilesApiError::Internal)?;

        uploaded.push(UploadedFile {
            path: absolute(&path),
            name,
            size,
        });
    }

    Ok(Json(UploadResponse { files: uploaded }))
}

pub async fn delete_file(
    State(state): State<Arc<ServerState>>,
    Path(path): Path<String>,
    headers: HeaderMap,
) -> Result<StatusCode, FilesApiError> {
    let Some(ref vfs) = state.vfs else {
        return Err(FilesApiError::NotFound);
    };
    let owner = auth::authenticated_user(&state, &headers).await?;
    let path = relative_path(Some(&path));
    if path.is_empty() {
        return Err(FilesApiError::BadRequest);
    }

    vfs.delete_global(owner, &path)
        .await
        .map_err(|_| FilesApiError::NotFound)?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn download_file(
    State(state): State<Arc<ServerState>>,
    Path(path): Path<String>,
    Query(query): Query<DownloadQuery>,
    headers: HeaderMap,
) -> Result<Response, FilesApiError> {
    let Some(ref vfs) = state.vfs else {
        return Err(FilesApiError::NotFound);
    };
    let owner = auth::authenticated_user(&state, &headers).await?;
    let path = relative_path(Some(&path));

    let (bytes, mime_type) = if let Some(version) = query.version {
        if version < 0 {
            return Err(FilesApiError::BadRequest);
        }
        vfs.read_version_global(owner, &path, version)
            .await
            .map_err(|_| FilesApiError::NotFound)?
    } else {
        vfs.read_bytes_global(owner, &path)
            .await
            .map_err(|_| FilesApiError::NotFound)?
    };

    super::file_response(&path, bytes, mime_type).map_err(|_| FilesApiError::Internal)
}

/// Normalizes an incoming path (absolute `/home/user/...` or bare) into the
/// storage-relative form the `*_global` VFS methods take, stripping the
/// `/home/user` mount prefix and `.`/`..`/empty segments.
fn relative_path(path: Option<&str>) -> String {
    let mut segments: Vec<&str> = path
        .unwrap_or_default()
        .split('/')
        .filter(|segment| !segment.is_empty() && *segment != "." && *segment != "..")
        .collect();
    if segments.first() == Some(&"home") && segments.get(1) == Some(&"user") {
        segments.drain(0..2);
    }
    segments.join("/")
}

/// Renders a storage-relative global path as the absolute mounted path clients
/// address (`/home/user/...`).
fn absolute(rel: &str) -> String {
    if rel.is_empty() {
        USER_HOME.to_string()
    } else {
        format!("{USER_HOME}/{rel}")
    }
}

fn join_path(parent: &str, name: &str) -> String {
    let name = relative_path(Some(name));
    if parent.is_empty() {
        name
    } else {
        format!("{parent}/{name}")
    }
}
