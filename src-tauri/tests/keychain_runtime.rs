//! The OS keychain used the way the app uses it: from a blocking thread of the async runtime
//! (`AppState::with_db`). Touches the real keychain under a throw-away service name and cleans up,
//! so it only runs on request: `cargo test -p cairn --test keychain_runtime -- --ignored`.

use cairn_core::secrets::{OsKeychain, SecretStore};

#[tokio::test(flavor = "multi_thread")]
#[ignore = "uses the real system keychain"]
async fn keychain_works_from_the_runtime_blocking_threads() {
    tokio::task::spawn_blocking(|| {
        let store = OsKeychain::new("cairn-test-throwaway");
        let account = "runtime-round-trip";
        store.delete(account).unwrap();
        assert_eq!(store.get(account).unwrap(), None);
        store.set(account, "secret-1").unwrap();
        assert_eq!(store.get(account).unwrap().as_deref(), Some("secret-1"));
        store.delete(account).unwrap();
        assert_eq!(store.get(account).unwrap(), None);
    })
    .await
    .unwrap();
}
