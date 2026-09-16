use createos::{Client, CreateSandboxRequest, Error, RequestOptions};
use std::{
    io::{Read as _, Write as _},
    net::TcpListener,
    sync::mpsc,
    time::Duration,
};

fn serve_once(status: &'static str, body: &'static str) -> (String, mpsc::Receiver<String>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let (sender, receiver) = mpsc::channel();
    std::thread::spawn(move || {
        let (mut connection, _) = listener.accept().unwrap();
        let mut request = vec![0_u8; 8192];
        let length = connection.read(&mut request).unwrap();
        sender
            .send(String::from_utf8_lossy(&request[..length]).into_owned())
            .unwrap();
        write!(
            connection,
            "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        )
        .unwrap();
    });
    (format!("http://{address}/prefix"), receiver)
}

#[tokio::test]
async fn unauthenticated_health_preserves_base_path() {
    let (base_url, request) = serve_once("200 OK", r#"{"status":"success","data":{"up":true}}"#);
    let client = Client::builder().base_url(base_url).build().unwrap();

    assert!(client.health().await.unwrap().up);
    let request = request.recv().unwrap();
    assert!(request.starts_with("GET /prefix/healthz HTTP/1.1"));
    assert!(!request.to_ascii_lowercase().contains("x-api-key:"));
}

#[tokio::test]
async fn public_catalog_and_readiness_endpoints_omit_authentication() {
    let cases = [
        ("/readyz", r#"{"status":"success","data":{"ready":true}}"#),
        ("/v1/shapes", r#"{"status":"success","data":[]}"#),
        (
            "/v1/rootfs",
            r#"{"status":"success","data":{"rootfs":[],"default":""}}"#,
        ),
    ];

    for (path, body) in cases {
        let (base_url, request) = serve_once("200 OK", body);
        let client = Client::builder()
            .base_url(base_url)
            .api_key("must-not-be-sent")
            .build()
            .unwrap();
        match path {
            "/readyz" => assert!(client.readiness().await.unwrap().ready),
            "/v1/shapes" => assert!(client.shapes().await.unwrap().is_empty()),
            "/v1/rootfs" => assert!(
                client
                    .root_file_systems()
                    .await
                    .unwrap()
                    .root_file_systems
                    .is_empty()
            ),
            _ => unreachable!(),
        }
        let request = request.recv().unwrap();
        assert!(request.starts_with(&format!("GET /prefix{path}")));
        assert!(!request.to_ascii_lowercase().contains("x-api-key:"));
    }
}

#[tokio::test]
async fn api_errors_remain_inspectable() {
    let (base_url, request) = serve_once(
        "401 Unauthorized",
        r#"{"status":"error","message":"bad key","code":42}"#,
    );
    let client = Client::builder()
        .base_url(base_url)
        .api_key("secret")
        .build()
        .unwrap();

    let error = client.who_am_i().await.unwrap_err();
    let Error::Api(error) = error else {
        panic!("expected API error")
    };
    assert_eq!(error.status.as_u16(), 401);
    assert_eq!(error.code, Some(42));
    assert_eq!(error.message, "bad key");
    assert!(
        request
            .recv()
            .unwrap()
            .to_ascii_lowercase()
            .contains("x-api-key: secret")
    );
}

#[tokio::test]
async fn protected_requests_require_an_api_key_before_sending() {
    let client = Client::builder()
        .base_url("http://127.0.0.1:1")
        .build()
        .unwrap();

    let error = client.who_am_i().await.unwrap_err();
    assert!(matches!(error, Error::Configuration(_)));
}

#[tokio::test]
async fn caller_headers_cannot_replace_or_add_credentials() {
    let (base_url, request) = serve_once(
        "401 Unauthorized",
        r#"{"status":"error","message":"denied"}"#,
    );
    let mut options = RequestOptions::default();
    options
        .headers
        .insert("x-api-key", "attacker".parse().unwrap());
    options
        .headers
        .insert("authorization", "Bearer attacker".parse().unwrap());
    options
        .headers
        .insert("cookie", "session=attacker".parse().unwrap());
    let client = Client::builder()
        .base_url(base_url)
        .api_key("sdk-key")
        .build()
        .unwrap();

    assert!(
        client
            .create_sandbox_with(CreateSandboxRequest::default(), &options)
            .await
            .is_err()
    );
    let request = request.recv().unwrap().to_ascii_lowercase();
    assert!(request.contains("x-api-key: sdk-key"));
    assert!(!request.contains("attacker"));
    assert!(!request.contains("authorization:"));
    assert!(!request.contains("cookie:"));
}

#[tokio::test]
async fn custom_http_builder_cannot_enable_redirects() {
    let attacker = TcpListener::bind("127.0.0.1:0").unwrap();
    let attacker_address = attacker.local_addr().unwrap();
    let (attacker_sender, attacker_request) = mpsc::channel();
    std::thread::spawn(move || {
        let (mut connection, _) = attacker.accept().unwrap();
        let mut request = vec![0_u8; 8192];
        let length = connection.read(&mut request).unwrap();
        attacker_sender
            .send(String::from_utf8_lossy(&request[..length]).into_owned())
            .unwrap();
    });

    let redirector = TcpListener::bind("127.0.0.1:0").unwrap();
    let redirector_address = redirector.local_addr().unwrap();
    std::thread::spawn(move || {
        let (mut connection, _) = redirector.accept().unwrap();
        let mut request = [0_u8; 8192];
        let _ = connection.read(&mut request).unwrap();
        write!(
            connection,
            "HTTP/1.1 302 Found\r\nLocation: http://{attacker_address}/steal\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
        )
        .unwrap();
    });

    let client = Client::builder()
        .base_url(format!("http://{redirector_address}"))
        .api_key("must-not-leak")
        .http_client(reqwest::Client::builder().redirect(reqwest::redirect::Policy::limited(10)))
        .build()
        .unwrap();

    let Error::Api(error) = client.who_am_i().await.unwrap_err() else {
        panic!("expected redirect response to remain visible")
    };
    assert_eq!(error.status.as_u16(), 302);
    assert!(
        attacker_request
            .recv_timeout(Duration::from_millis(100))
            .is_err()
    );
}
