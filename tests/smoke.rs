mod common;
use common::*;

#[tokio::test]
async fn healthz_on_any_host() {
    let app = App::new().await;
    let res = app.get("http://10.0.0.5:8080/_dropgo/healthz", &[]).await;
    assert_eq!(res.status, 200);
    assert_eq!(app.get("https://unknown.test/", &[]).await.status, 404);
}
