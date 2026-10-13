//! Utility functions for the RESTful API robot HAL.

/// Normalizes an endpoint path to ensure it starts with `/api/v1/` if the pattern exists.
/// This makes path handling more robust, whether the full path or a relative one is provided.
pub(crate) fn normalize_endpoint_path(path: &str) -> String {
    if let Some(start) = path.find("/api/v1/") {
        path[start..].to_string()
    } else {
        path.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The scheme and authority are stripped, whatever the host's form.
    #[test]
    fn test_normalize_endpoint_path_with_host() {
        for (url, path) in [
            ("http://192.168.1.143:5000/api/v1/head", "/api/v1/head"),
            ("http://localhost/api/v1/arm", "/api/v1/arm"),
            ("http://<host>/api/v1/head", "/api/v1/head"),
        ] {
            assert_eq!(normalize_endpoint_path(url), path, "{url}");
        }
    }

    #[test]
    fn test_normalize_endpoint_path_no_host() {
        let path = "/api/v1/arm";
        assert_eq!(normalize_endpoint_path(path), "/api/v1/arm");
    }

    #[test]
    fn test_normalize_endpoint_path_invalid() {
        let path = "http://localhost/invalid/path";
        assert_eq!(
            normalize_endpoint_path(path),
            "http://localhost/invalid/path"
        );
    }
}
