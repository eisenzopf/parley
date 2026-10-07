use serde_json::json;
use vapi::{CreateChatRequest, VapiClient, VapiError};
use wiremock::matchers::{header, header_exists, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

async fn client(server: &MockServer) -> VapiClient {
    VapiClient::builder()
        .token("sk-test")
        .base_url(server.uri())
        .max_retries(0)
        .build()
        .unwrap()
}

#[tokio::test]
async fn chat_create_sends_bearer_and_user_agent() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/chat"))
        .and(header("authorization", "Bearer sk-test"))
        .and(header_exists("user-agent"))
        .and(header("x-fern-language", "Rust"))
        .respond_with(ResponseTemplate::new(201).set_body_json(json!({
            "id": "chat_1",
            "assistantId": "asst_1",
            "output": [{ "role": "assistant", "text": "hello from vapi" }]
        })))
        .mount(&server)
        .await;

    let chat = client(&server)
        .await
        .chats()
        .create(CreateChatRequest {
            assistant_id: Some("asst_1".into()),
            input: Some("hi".into()),
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(chat.id, "chat_1");
    assert_eq!(chat.output_text(), "hello from vapi");
}

#[tokio::test]
async fn list_assistants_accepts_wrapped_results() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/assistant"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "results": [{ "id": "asst_1", "name": "Parley", "mysteryField": true }]
        })))
        .mount(&server)
        .await;

    let list = client(&server).await.assistants().list().await.unwrap();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].name.as_deref(), Some("Parley"));
    assert_eq!(list[0].extra["mysteryField"], true);
}

#[tokio::test]
async fn unauthorized_status_does_not_include_token() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/assistant"))
        .respond_with(ResponseTemplate::new(401).set_body_json(json!({
            "message": "nope"
        })))
        .mount(&server)
        .await;

    let err: VapiError = client(&server).await.assistants().list().await.unwrap_err();
    assert!(err.is_unauthorized());
    assert_eq!(err.status_code, Some(401));
    let shown = format!("{err}");
    assert!(!shown.contains("sk-test"));
    assert!(shown.contains("Status code: 401"));
}

#[tokio::test]
async fn create_call() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/call"))
        .respond_with(ResponseTemplate::new(201).set_body_json(json!({
            "id": "call_1",
            "status": "queued",
            "assistantId": "asst_1"
        })))
        .mount(&server)
        .await;

    let call = client(&server)
        .await
        .calls()
        .create(vapi::CreateCallRequest {
            assistant_id: Some("asst_1".into()),
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(call.id, "call_1");
    assert!(call.is_active());
}
