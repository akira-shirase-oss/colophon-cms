//! The object store against the local Garage, end to end (needs `scripts/stack.sh up`).
#![expect(clippy::expect_used, reason = "integration test")]

use object_store::{ObjectStoreExt, path::Path};

/// Same builder shape `colophon-server::asset_store` uses, against `deploy/local.env`.
fn garage() -> object_store::aws::AmazonS3 {
    object_store::aws::AmazonS3Builder::new()
        .with_endpoint("http://127.0.0.1:53900")
        .with_region("garage")
        .with_bucket_name("colophon-dev")
        .with_access_key_id("GK636f6c6f70686f6e2d646576")
        .with_secret_access_key("636f6c6f70686f6e2d6c6f63616c2d6465762d73332d7365637265742d6b6579")
        .with_allow_http(true)
        .build()
        .expect("build")
}

#[tokio::test]
#[ignore = "needs the local stack's Garage"]
async fn puts_and_gets_and_deletes() {
    let store = garage();
    let key = Path::from("media-test/probe.txt");
    store
        .put(&key, b"hello".as_slice().into())
        .await
        .expect("put");
    let got = store
        .get(&key)
        .await
        .expect("get")
        .bytes()
        .await
        .expect("bytes");
    assert_eq!(&got[..], b"hello");
    store.delete(&key).await.expect("delete");
}
