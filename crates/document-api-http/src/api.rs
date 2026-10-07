use axum::Router;
use axum::body::Body;
use axum::extract::{Request, State};
use axum::http::Method;
use axum::response::Response;
use tower::Service;

/// Complete HTTP surface assembled from independently testable route families.
///
/// Each family remains responsible for its own trusted identity, timeout and
/// security middleware. The dispatcher only resolves the action-style paths
/// which cannot be merged as ordinary axum routes because they share a method
/// and a dynamic segment.
#[derive(Clone)]
pub struct DocumentApiRouters {
    read: Router,
    management: Router,
    create: Router,
    versioning: Router,
    publication: Router,
    file: Router,
    diff: Router,
}

impl DocumentApiRouters {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        read: Router,
        management: Router,
        create: Router,
        versioning: Router,
        publication: Router,
        file: Router,
        diff: Router,
    ) -> Self {
        Self {
            read,
            management,
            create,
            versioning,
            publication,
            file,
            diff,
        }
    }

    fn select(&self, method: &Method, path: &str) -> &Router {
        if path == "/v1/documents" {
            return if method == Method::POST {
                &self.create
            } else {
                &self.read
            };
        }
        if path.starts_with("/v1/document-creation-outcomes/") {
            return &self.create;
        }
        if path.starts_with("/v1/folders") {
            return if method == Method::GET {
                &self.read
            } else {
                &self.management
            };
        }
        if path.starts_with("/v1/documents/") {
            if method == Method::POST
                && (path.ends_with("/comparisons") || path.ends_with("/revision-comparisons"))
            {
                return &self.diff;
            }
            if method == Method::GET && path.contains("/files/") {
                return &self.file;
            }
            if method == Method::POST && is_publication_action(path) {
                return &self.publication;
            }
            if method == Method::POST && path.ends_with(":rebase") {
                return &self.versioning;
            }
            if method == Method::POST && path.ends_with("/versions") {
                return &self.versioning;
            }
            if (method == Method::GET || method == Method::PUT) && path.ends_with("/read-state") {
                return &self.management;
            }
            if method == Method::POST
                && (path.ends_with("/read-state/view") || path.ends_with("/read-state/reset"))
            {
                return &self.management;
            }
            if method == Method::PUT && path.ends_with("/access-policy") {
                return &self.management;
            }
            if method == Method::PUT && path.contains("/versions/") {
                return &self.versioning;
            }
            if method == Method::PATCH || method == Method::POST && path.ends_with(":move") {
                return &self.management;
            }
        }
        &self.read
    }
}

fn is_publication_action(path: &str) -> bool {
    [
        ":publish",
        ":withdraw",
        ":schedule-publication",
        ":cancel-publication-schedule",
        ":end-publication",
    ]
    .iter()
    .any(|suffix| path.ends_with(suffix))
}

/// Returns one authenticated API router covering every v0 Document operation.
pub fn compose_document_api(routers: DocumentApiRouters) -> Router {
    Router::new().fallback(dispatch).with_state(routers)
}

async fn dispatch(State(routers): State<DocumentApiRouters>, request: Request<Body>) -> Response {
    let selected = routers
        .select(request.method(), request.uri().path())
        .clone();
    let mut selected = selected;
    match selected.call(request).await {
        Ok(response) => response,
        Err(error) => match error {},
    }
}
