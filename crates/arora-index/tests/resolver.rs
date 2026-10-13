mod fake_studio;
use arora_index::{content_hash, AssetType, Cache, Error, NoToken, ResolutionSource, Resolver};
use fake_studio::*;
use serde_json::json;
use std::time::Duration;
use wiremock::matchers::{method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};
/// Exercises alias lookup, three release pages and Storage without an authorization header.
#[tokio::test]
async fn public_alias_resolves_and_fetches_without_authorization() {
    bounded(async {
        let server = MockServer::start().await;
        documents(&server, false, "model").await;
        let listing = format!("{ROOT}/publishedAssets/asset/releases");
        Mock::given(path(&listing))
            .and(|r: &wiremock::Request| !r.url.query_pairs().any(|(k, _)| k == "pageToken"))
            .and(query_param("pageSize", "100"))
            .respond_with(ResponseTemplate::new(200).set_body_json(
                json!({"documents": [release("a", "2.9.0")], "nextPageToken": "page + two"}),
            ))
            .mount(&server)
            .await;
        Mock::given(path(&listing))
            .and(query_param("pageToken", "page + two"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(json!({"nextPageToken": "three"})),
            )
            .mount(&server)
            .await;
        Mock::given(path(&listing))
            .and(query_param("pageToken", "three"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(json!({"documents": [release("c", "2.10.0")]})),
            )
            .mount(&server)
            .await;
        storage(&server, BYTES, 200).await;
        let resolver = Resolver::new(config(&server), NoToken).unwrap();
        let pin = resolver.resolve(NAME, "*", AssetType::Model).await.unwrap();
        assert_eq!(pin.scoped_name, CURRENT);
        assert_eq!(pin.published_asset_id, "asset");
        assert_eq!(pin.version, "2.10.0");
        assert_eq!(pin.release_id, "c");
        assert_eq!(pin.artifact_key, "primary");
        assert_eq!(pin.family.as_deref(), Some("test"));
        assert!(!pin.private);
        assert!(pin.resolved_at > 0);
        let fetched = resolver.fetch(&pin).await.unwrap();
        assert_eq!(fetched.bytes, BYTES);
        assert_eq!(fetched.content_hash, content_hash(BYTES));
        assert_eq!(fetched.resolved, Some(pin));
        let requests = server.received_requests().await.unwrap();
        assert_eq!(requests.len(), 6);
        assert!(requests
            .iter()
            .all(|r| !r.headers.contains_key("authorization")));
        assert!(requests
            .last()
            .unwrap()
            .url
            .as_str()
            .contains("publishedAssets%2Fasset%2Freleases%2Frelease%2Fartifacts%2Fprimary"));
    })
    .await;
}

/// Exercises private Firestore and Storage reads with each service's authorization scheme.
#[tokio::test]
async fn authenticated_private_chain_uses_each_services_header() {
    bounded(async {
        let server = MockServer::start().await;
        chain(&server, true).await;
        storage(&server, BYTES, 200).await;
        let resolver = Resolver::new(config(&server), Token).unwrap();
        let pin = resolver.resolve(NAME, "*", AssetType::Model).await.unwrap();
        assert!(pin.private);
        resolver.fetch(&pin).await.unwrap();
        for request in server.received_requests().await.unwrap() {
            assert_eq!(
                request.headers["authorization"],
                if request.url.path().starts_with("/v0/") {
                    "Firebase test-token"
                } else {
                    "Bearer test-token"
                }
            );
        }
    })
    .await;
}

/// The socket failure, saved pin, cache bytes and definitive server response cross crate boundaries.
#[tokio::test]
async fn refused_connection_falls_back_and_definitive_404_clears() {
    bounded(async {
        let server = MockServer::start().await;
        let dir = tempfile::tempdir().unwrap();
        let cache = Cache::new(dir.path()).unwrap();
        let pin = pin();
        cache.save_pin(NAME, &pin).unwrap();
        cache.store(&pin.content_hash, BYTES, true).unwrap();
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let mut cfg = config(&server);
        cfg.firestore_url = format!("http://{}", listener.local_addr().unwrap());
        drop(listener);
        let offline = Resolver::builder(cfg, NoToken)
            .connect_timeout(Duration::from_millis(100))
            .build()
            .unwrap();
        let error = offline
            .resolve(NAME, "*", AssetType::Model)
            .await
            .unwrap_err();
        let Error::Network { detail, .. } = error else {
            panic!("{error}")
        };
        assert!(detail.to_lowercase().contains("connection refused"));
        let result = offline
            .resolve_with_fallback(NAME, "*", AssetType::Model, &cache)
            .await
            .unwrap();
        assert_eq!(result.source, ResolutionSource::Cache);
        assert_eq!(
            cache
                .fetch_async(&result.resolved)
                .await
                .unwrap()
                .unwrap()
                .bytes,
            BYTES
        );
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(404))
            .mount(&server)
            .await;
        let online = Resolver::new(config(&server), NoToken).unwrap();
        assert!(matches!(
            online
                .resolve_with_fallback(NAME, "*", AssetType::Model, &cache)
                .await,
            Err(Error::NotFound {
                http_status: 404,
                ..
            })
        ));
        assert!(cache.last_pin(NAME).unwrap().is_none());
        assert!(cache.read(&pin.content_hash, true).unwrap().is_none());
    })
    .await;
}
