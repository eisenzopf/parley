//! Telnyx inbound/outbound lives behind `--features sms-telnyx`.

#[allow(dead_code)]
pub fn inbound_path() -> &'static str {
    "/v1/sms/inbound"
}
