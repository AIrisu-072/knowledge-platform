//! Validated same-origin build artifacts. Business/API paths are dispatched elsewhere.
use crate::composition::StartupError;
use axum::{
    Router,
    body::Body,
    extract::{Request, State},
    http::{Method, StatusCode, header},
    response::{IntoResponse, Response},
};
use html5ever::tendril::TendrilSink;
use markup5ever_rcdom::{Handle, NodeData, RcDom};
use percent_encoding::percent_decode_str;
use std::{
    path::{Component, Path, PathBuf},
    sync::Arc,
};
use tower::ServiceExt;
use tower_http::services::ServeFile;

fn safe_relative(path: &str) -> Option<PathBuf> {
    let decoded = percent_decode_str(path).decode_utf8().ok()?;
    if decoded.contains(['\\', '%', '\0']) || decoded.chars().any(char::is_control) {
        return None;
    }
    let relative = decoded.strip_prefix('/').unwrap_or(&decoded);
    // Check text first: Path::components normalizes a single dot component.
    if relative.split('/').any(|part| part.starts_with('.')) {
        return None;
    }
    let path = PathBuf::from(relative);
    if path
        .components()
        .any(|part| !matches!(part, Component::Normal(_)))
        || relative.ends_with(".map")
    {
        return None;
    }
    Some(path)
}

fn inside_file(root: &Path, relative: &Path) -> Result<PathBuf, StartupError> {
    let path = root
        .join(relative)
        .canonicalize()
        .map_err(|_| StartupError::Web)?;
    if !path.starts_with(root) || !path.is_file() {
        return Err(StartupError::Web);
    }
    // A benign-looking alias must not reveal a dotfile or source map either.
    let relative = path.strip_prefix(root).map_err(|_| StartupError::Web)?;
    if safe_relative(&relative.to_string_lossy()).is_none() {
        return Err(StartupError::Web);
    }
    Ok(path)
}

fn referenced_assets(node: &Handle, assets: &mut Vec<String>) {
    if let NodeData::Element { name, attrs, .. } = &node.data {
        let attribute = match name.local.as_ref() {
            "script" | "img" => Some("src"),
            "link" => Some("href"),
            _ => None,
        };
        if let Some(attribute) = attribute {
            for attr in attrs
                .borrow()
                .iter()
                .filter(|attr| attr.name.local.as_ref() == attribute)
            {
                assets.push(attr.value.to_string());
            }
        }
    }
    for child in node.children.borrow().iter() {
        referenced_assets(child, assets);
    }
}

/// Validate the built index and its referenced artifacts with the already-selected
/// HTML parser. This never evaluates scripts or rewrites the production bundle.
pub(crate) fn validate_dist(dist: &Path) -> Result<PathBuf, StartupError> {
    let root = dist.canonicalize().map_err(|_| StartupError::Web)?;
    if !root.is_dir() {
        return Err(StartupError::Web);
    }
    let index = inside_file(&root, Path::new("index.html"))?;
    let metadata = std::fs::metadata(&index).map_err(|_| StartupError::Web)?;
    if metadata.len() > document_api_http::limits::MAX_JSON_BODY_BYTES as u64 {
        return Err(StartupError::Web);
    }
    let html = std::fs::read_to_string(index).map_err(|_| StartupError::Web)?;
    let document = html5ever::parse_document(RcDom::default(), Default::default()).one(html);
    let mut assets = Vec::new();
    referenced_assets(&document.document, &mut assets);
    // A plain/unbuilt source index is not the production GUI.
    if !assets.iter().any(|asset| asset.ends_with(".js")) {
        return Err(StartupError::Web);
    }
    for asset in assets {
        if asset.starts_with("//") || asset.contains([':', '?', '#']) {
            return Err(StartupError::Web);
        }
        let relative = safe_relative(&asset).ok_or(StartupError::Web)?;
        let path = inside_file(&root, &relative)?;
        std::fs::File::open(path).map_err(|_| StartupError::Web)?;
    }
    Ok(root)
}

pub fn web_router(dist: &Path) -> Result<Router, StartupError> {
    let root = validate_dist(dist)?;
    Ok(Router::new().fallback(serve).with_state(Arc::new(root)))
}

// Missing SPA routes are distinct from existing unsafe aliases. Check each
// existing ancestor so a missing child beneath an escaping symlink is rejected.
fn navigation_missing_inside(root: &Path, relative: &Path) -> bool {
    let mut candidate = root.to_path_buf();
    for component in relative.components() {
        candidate.push(component);
        match std::fs::symlink_metadata(&candidate) {
            Ok(_) => {
                let Ok(canonical) = candidate.canonicalize() else {
                    return false;
                };
                let Ok(within) = canonical.strip_prefix(root) else {
                    return false;
                };
                if safe_relative(&within.to_string_lossy()).is_none() {
                    return false;
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return true,
            Err(_) => return false,
        }
    }
    candidate.is_dir()
}

async fn serve(State(root): State<Arc<PathBuf>>, request: Request) -> Response {
    if request.method() != Method::GET && request.method() != Method::HEAD {
        return StatusCode::METHOD_NOT_ALLOWED.into_response();
    }
    let Some(relative) = safe_relative(request.uri().path()) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    if relative
        .components()
        .next()
        .is_some_and(|part| part.as_os_str() == "v1" || part.as_os_str() == "health")
    {
        return StatusCode::NOT_FOUND.into_response();
    }
    let is_navigation = relative.extension().is_none()
        && relative
            .components()
            .next()
            .is_none_or(|part| part.as_os_str() != "assets")
        && request
            .headers()
            .get(header::ACCEPT)
            .and_then(|v| v.to_str().ok())
            .is_some_and(|v| {
                v.split(',')
                    .any(|part| part.trim().starts_with("text/html"))
            });
    let path = match inside_file(&root, &relative) {
        Ok(path) => path,
        Err(_) if is_navigation && navigation_missing_inside(&root, &relative) => {
            match inside_file(&root, Path::new("index.html")) {
                Ok(path) => path,
                Err(_) => return StatusCode::SERVICE_UNAVAILABLE.into_response(),
            }
        }
        Err(_) => return StatusCode::NOT_FOUND.into_response(),
    };
    let response = ServeFile::new(path).oneshot(request).await;
    let mut response = match response {
        Ok(response) => response.map(Body::new),
        Err(error) => match error {},
    };
    let headers = response.headers_mut();
    headers.insert(header::CACHE_CONTROL, "no-cache".parse().unwrap());
    headers.insert("x-content-type-options", "nosniff".parse().unwrap());
    headers.insert("referrer-policy", "no-referrer".parse().unwrap());
    headers.insert("content-security-policy","default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; img-src 'self' data:; object-src 'none'; frame-ancestors 'none'; base-uri 'self'".parse().unwrap());
    response
}
