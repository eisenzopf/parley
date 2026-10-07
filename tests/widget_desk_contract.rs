//! Widget and desk speak UCTP nouns — not a private signaling protocol.

#[test]
fn widget_sets_visitor_cookie_and_creates_over_uctp() {
    let html = include_str!("../web/widget/index.html");
    let uctp = include_str!("../web/widget/uctp.js");
    assert!(html.contains("parley_vid"), "durable visitor cookie");
    assert!(html.contains("conversation.create"));
    assert!(html.contains("session.invite"));
    assert!(html.contains("connection.offer"));
    assert!(html.contains("websocket+webrtc"));
    assert!(html.contains("substrate_setup"));
    assert!(html.contains("./uctp.js"));
    assert!(html.contains("EventSource"));
    assert!(html.contains("/v1/events"));
    assert!(
        !html.contains("Call us"),
        "widget must not present a PSTN call button"
    );
    assert!(
        !html.contains("type: \"offer\""),
        "must not send private {{type,sdp}} JSON"
    );
    assert!(html.contains("ipv4OnlySdp"));
    assert!(uctp.contains("ipv4OnlySdp"));
    assert!(uctp.contains("auth.hello"));
    assert!(uctp.contains("auth.response"));
    assert!(uctp.contains("waitType"));
}

#[test]
fn desk_pickup_offers_operator_voice_over_uctp() {
    let html = include_str!("../web/desk/index.html");
    assert!(html.contains("/v1/conversations"));
    assert!(html.contains("EventSource"));
    assert!(html.contains("/v1/events"));
    assert!(html.contains("pickup/accept"));
    assert!(html.contains("session.invite"));
    assert!(html.contains("connection.offer"));
    assert!(html.contains("conn_operator_"));
    assert!(html.contains("ipv4OnlySdp"));
}

#[test]
fn http_router_has_no_call_or_leg_resources() {
    let src = include_str!("../src/http/mod.rs");
    assert!(!src.contains("/v1/calls"));
    assert!(!src.contains("/v1/legs"));
    assert!(!src.contains("/v1/dialogs"));
    assert!(src.contains("/v1/conversations"));
    assert!(src.contains("/v1/sessions"));
    assert!(src.contains("/v1/events"));
    assert!(src.contains("/healthz"));
    assert!(src.contains("/v1/public"));
}
