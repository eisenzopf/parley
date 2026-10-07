//! Explicit conference signaling/media configuration; credentials stay private.
use crate::{ApiError, Result};
use serde::{Deserialize, Serialize};
use std::{
    fmt,
    net::{IpAddr, SocketAddr},
};

#[derive(Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ConferenceNetwork {
    pub sip_username: String,
    pub sip_password: String,
    pub sip_realm: Option<String>,
    pub sip_asserted_identity: Option<String>,
    pub sip_advertised_addr: Option<SocketAddr>,
    pub sip_media_public_addr: Option<SocketAddr>,
    pub sip_media_ports: [u16; 2],
    pub webrtc_udp: SocketAddr,
    pub webrtc_ports: Option<[u16; 2]>,
    pub webrtc_public_ips: Vec<IpAddr>,
    pub server_ice: Vec<IceServer>,
    /// Explicitly client-facing ICE credentials, never copied from server_ice.
    pub browser_ice: Vec<IceServer>,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct IceServer {
    pub urls: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub username: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub credential: Option<String>,
}

impl Default for ConferenceNetwork {
    fn default() -> Self {
        Self {
            sip_username: String::new(),
            sip_password: String::new(),
            sip_realm: None,
            sip_asserted_identity: None,
            sip_advertised_addr: None,
            sip_media_public_addr: None,
            sip_media_ports: [44000, 44200],
            webrtc_udp: "127.0.0.1:0".parse().unwrap(),
            webrtc_ports: None,
            webrtc_public_ips: vec![],
            server_ice: vec![],
            browser_ice: vec![],
        }
    }
}

impl fmt::Debug for ConferenceNetwork {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ConferenceNetwork")
            .field(
                "sip_auth_configured",
                &(!self.sip_username.is_empty() && !self.sip_password.is_empty()),
            )
            .field("sip_advertised_addr", &self.sip_advertised_addr)
            .field("sip_media_public_addr", &self.sip_media_public_addr)
            .field("webrtc_udp", &self.webrtc_udp)
            .field("server_ice_count", &self.server_ice.len())
            .field("browser_ice_count", &self.browser_ice.len())
            .finish()
    }
}

impl ConferenceNetwork {
    pub fn overlay_env(&mut self) -> Result<()> {
        self.apply_env(|name| std::env::var(name).ok())
    }

    fn apply_env(&mut self, get: impl Fn(&str) -> Option<String>) -> Result<()> {
        if let Some(v) = get("CONFERENCE_SIP_USERNAME") {
            self.sip_username = v;
        }
        if let Some(v) = get("CONFERENCE_SIP_PASSWORD") {
            self.sip_password = v;
        }
        if let Some(v) = get("CONFERENCE_SIP_REALM") {
            self.sip_realm = Some(v);
        }
        if let Some(v) = get("CONFERENCE_SIP_ASSERTED_IDENTITY") {
            self.sip_asserted_identity = Some(v);
        }
        for (name, target) in [
            ("CONFERENCE_SIP_ADVERTISE", &mut self.sip_advertised_addr),
            (
                "CONFERENCE_SIP_MEDIA_PUBLIC",
                &mut self.sip_media_public_addr,
            ),
        ] {
            if let Some(v) = get(name) {
                *target =
                    Some(v.parse().map_err(|_| {
                        ApiError::bad_request(format!("{name} must be an IP:port"))
                    })?);
            }
        }
        if let Some(v) = get("CONFERENCE_WEBRTC_UDP") {
            self.webrtc_udp = v
                .parse()
                .map_err(|_| ApiError::bad_request("CONFERENCE_WEBRTC_UDP must be an IP:port"))?;
        }
        if let Some(v) = get("CONFERENCE_SIP_MEDIA_PORTS") {
            self.sip_media_ports = parse_ports(&v)?;
        }
        if let Some(v) = get("CONFERENCE_WEBRTC_PORTS") {
            self.webrtc_ports = Some(parse_ports(&v)?);
        }
        if let Some(v) = get("CONFERENCE_WEBRTC_PUBLIC_IPS") {
            self.webrtc_public_ips = v
                .split(',')
                .map(|ip| {
                    ip.trim().parse().map_err(|_| {
                        ApiError::bad_request(
                            "CONFERENCE_WEBRTC_PUBLIC_IPS must contain IP addresses",
                        )
                    })
                })
                .collect::<Result<_>>()?;
        }
        for (name, target) in [
            ("CONFERENCE_SERVER_ICE_JSON", &mut self.server_ice),
            ("CONFERENCE_BROWSER_ICE_JSON", &mut self.browser_ice),
        ] {
            if let Some(v) = get(name) {
                *target = serde_json::from_str(&v).map_err(|_| {
                    ApiError::bad_request(format!("{name} must be an ICE server array"))
                })?;
            }
        }
        self.validate()
    }

    pub fn validate(&self) -> Result<()> {
        if self.sip_username.is_empty() != self.sip_password.is_empty() {
            return Err(ApiError::bad_request(
                "conference SIP username and password must be supplied together",
            ));
        }
        for ports in std::iter::once(self.sip_media_ports).chain(self.webrtc_ports) {
            if ports[0] == 0 || ports[0] > ports[1] {
                return Err(ApiError::bad_request(
                    "conference media port range must be nonzero and ascending",
                ));
            }
        }
        if self.sip_media_ports[0] == self.sip_media_ports[1] {
            return Err(ApiError::bad_request(
                "SIP media range must include RTP and RTCP ports",
            ));
        }
        if self
            .sip_advertised_addr
            .is_some_and(|a| a.ip().is_unspecified() || a.port() == 0)
            || self
                .sip_media_public_addr
                .is_some_and(|a| a.ip().is_unspecified())
        {
            return Err(ApiError::bad_request(
                "advertised SIP addresses must name a reachable interface",
            ));
        }
        if self.webrtc_public_ips.len() > 1
            || self
                .webrtc_public_ips
                .iter()
                .any(|ip| ip.is_unspecified() || ip.is_ipv4() != self.webrtc_udp.is_ipv4())
        {
            return Err(ApiError::bad_request(
                "WebRTC static NAT requires one IP matching the UDP bind address family",
            ));
        }
        for ice in self.server_ice.iter().chain(&self.browser_ice) {
            if ice.urls.is_empty() || ice.urls.len() > 8 {
                return Err(ApiError::bad_request("ICE server needs one to eight URLs"));
            }
            for url in &ice.urls {
                let turn = url.starts_with("turn:") || url.starts_with("turns:");
                if !(turn || url.starts_with("stun:") || url.starts_with("stuns:"))
                    || url
                        .split_once(':')
                        .is_none_or(|(_, address)| address.is_empty())
                    || url.len() > 2048
                    || url.contains('@')
                    || url.chars().any(char::is_whitespace)
                {
                    return Err(ApiError::bad_request(
                        "ICE URLs must use STUN/TURN without embedded credentials",
                    ));
                }
                if turn
                    && (ice.username.as_deref().is_none_or(str::is_empty)
                        || ice.credential.as_deref().is_none_or(str::is_empty))
                {
                    return Err(ApiError::bad_request(
                        "TURN requires explicit username and credential",
                    ));
                }
            }
        }
        if self.server_ice.len() > 8 || self.browser_ice.len() > 8 {
            return Err(ApiError::bad_request("too many ICE servers"));
        }
        Ok(())
    }

    #[cfg(feature = "sip")]
    pub fn sip_config(&self, bind: SocketAddr, from: &str) -> Result<rvoip_sip::Config> {
        self.validate()?;
        let mut config = rvoip_sip::Config::on("parley-conference", bind.ip(), bind.port());
        config.media_port_start = self.sip_media_ports[0];
        config.media_port_end = self.sip_media_ports[1];
        config.sip_advertised_addr = self.sip_advertised_addr;
        config.media_public_addr = self.sip_media_public_addr;
        // The pinned RTP Session has one socket; require the peer to agree.
        config.rtcp_mux_required = true;
        config.pai_uri = self.sip_asserted_identity.clone();
        if !from.is_empty() {
            config.local_uri = from.into();
        }
        if !self.sip_username.is_empty() {
            config.credentials = Some(rvoip_sip::types::Credentials {
                username: self.sip_username.clone(),
                password: self.sip_password.clone(),
                realm: self.sip_realm.clone(),
            });
        }
        Ok(config)
    }

    #[cfg(feature = "media-webrtc")]
    pub fn webrtc_config(&self) -> Result<rvoip_webrtc::WebRtcConfig> {
        self.validate()?;
        let mut config = rvoip_webrtc::WebRtcConfig::loopback();
        config.udp_bind = self.webrtc_udp.to_string();
        config.udp_port_range =
            self.webrtc_ports
                .map(|p| rvoip_webrtc::config::UdpPortRangeConfig {
                    bind_ip: self.webrtc_udp.ip(),
                    port_start: p[0],
                    port_end: p[1],
                });
        config.nat_1to1_ips = self
            .webrtc_public_ips
            .iter()
            .map(ToString::to_string)
            .collect();
        config.ice_servers = self
            .server_ice
            .iter()
            .map(|s| rvoip_webrtc::IceServerConfig {
                urls: s.urls.clone(),
                username: s.username.clone(),
                credential: s.credential.clone(),
            })
            .collect();
        Ok(config)
    }
}

fn parse_ports(value: &str) -> Result<[u16; 2]> {
    let (a, b) = value
        .split_once('-')
        .ok_or_else(|| ApiError::bad_request("media port range must be START-END"))?;
    Ok([
        a.parse()
            .map_err(|_| ApiError::bad_request("invalid media port range"))?,
        b.parse()
            .map_err(|_| ApiError::bad_request("invalid media port range"))?,
    ])
}

/// Stage evidence is a redacted projection; durable replay keeps the original.
pub fn redact_evidence(mut value: serde_json::Value) -> serde_json::Value {
    fn scrub(value: &mut serde_json::Value) {
        match value {
            serde_json::Value::Object(fields) => {
                for (key, value) in fields {
                    if matches!(
                        key.to_ascii_lowercase().as_str(),
                        "credential" | "password" | "token" | "session_token" | "authorization"
                    ) {
                        *value = serde_json::json!("[redacted]");
                    } else if key.eq_ignore_ascii_case("sdp") && value.is_string() {
                        let sdp = value.as_str().expect("SDP string");
                        let safe: String = sdp
                            .split_inclusive('\n')
                            .map(|line| {
                                let lower = line.to_ascii_lowercase();
                                if let Some(prefix) = [
                                    "a=ice-pwd:",
                                    "a=ice-ufrag:",
                                    "a=crypto:",
                                    "a=key-mgmt:",
                                    "k=",
                                ]
                                .into_iter()
                                .find(|prefix| lower.starts_with(prefix))
                                {
                                    let ending = if line.ends_with("\r\n") {
                                        "\r\n"
                                    } else if line.ends_with('\n') {
                                        "\n"
                                    } else {
                                        ""
                                    };
                                    format!("{}[redacted]{}", &line[..prefix.len()], ending)
                                } else {
                                    line.into()
                                }
                            })
                            .collect();
                        *value = serde_json::Value::String(safe);
                    } else {
                        scrub(value);
                    }
                }
            }
            serde_json::Value::Array(values) => {
                for value in values {
                    scrub(value);
                }
            }
            _ => {}
        }
    }
    scrub(&mut value);
    value
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn network_env_validation_and_redaction() {
        let mut config = ConferenceNetwork::default();
        config.apply_env(|key|match key {
            "CONFERENCE_SIP_USERNAME"=>Some("private-user".into()),
            "CONFERENCE_SIP_PASSWORD"=>Some("private-password".into()),
            "CONFERENCE_SERVER_ICE_JSON"=>Some(r#"[{"urls":["turn:relay.invalid:3478"],"username":"turn-user","credential":"turn-password"}]"#.into()),
            "CONFERENCE_SIP_ADVERTISE"=>Some("203.0.113.20:5060".into()),
            "CONFERENCE_SIP_MEDIA_PUBLIC"=>Some("203.0.113.20:0".into()),
            "CONFERENCE_WEBRTC_PUBLIC_IPS"=>Some("203.0.113.20".into()),
            "CONFERENCE_WEBRTC_PORTS"=>Some("46000-46100".into()),
            _=>None,
        }).unwrap();
        let debug = format!("{config:?}");
        for secret in [
            "private-user",
            "private-password",
            "turn-user",
            "turn-password",
        ] {
            assert!(!debug.contains(secret));
        }
        assert!(config.browser_ice.is_empty());
        let mut invalid = ConferenceNetwork::default();
        invalid.webrtc_public_ips = vec![
            "203.0.113.1".parse().unwrap(),
            "203.0.113.2".parse().unwrap(),
        ];
        assert!(invalid.validate().is_err());
        invalid.webrtc_public_ips = vec!["::1".parse().unwrap()];
        assert!(invalid.validate().is_err());
        invalid.webrtc_public_ips.clear();
        invalid.browser_ice = vec![IceServer {
            urls: vec!["stun:".into()],
            username: None,
            credential: None,
        }];
        assert!(invalid.validate().is_err());
        let private = serde_json::json!({"response":{"payload":{"ice_servers":[{"credential":"PRIVATE_TURN","urls":["turn:relay.invalid"]}]}}});
        assert!(!redact_evidence(private.clone())
            .to_string()
            .contains("PRIVATE_TURN"));
        assert!(private.to_string().contains("PRIVATE_TURN"));
        let sdp = serde_json::json!({"sdp":"v=0\r\na=ice-ufrag:PRIVATE_USER\r\na=ice-pwd:PRIVATE_ICE\r\na=crypto:1 AES_CM_128_HMAC_SHA1_80 inline:PRIVATE_KEY\r\nm=audio 9 UDP/TLS/RTP/SAVPF 111\r\n"});
        let safe = redact_evidence(sdp.clone()).to_string();
        assert!(!safe.contains("PRIVATE_"));
        assert!(safe.contains("m=audio"));
        assert!(sdp.to_string().contains("PRIVATE_ICE"));

        #[cfg(feature = "sip")]
        {
            let sip = config
                .sip_config("0.0.0.0:5060".parse().unwrap(), "sip:demo@example.invalid")
                .unwrap();
            assert_eq!(sip.bind_addr, "0.0.0.0:5060".parse::<SocketAddr>().unwrap());
            assert_eq!(sip.media_public_addr.unwrap().port(), 0);
            assert_eq!(sip.credentials.unwrap().password, "private-password");
        }
        #[cfg(feature = "media-webrtc")]
        {
            let rtc = config.webrtc_config().unwrap();
            assert_eq!(rtc.udp_port_range.unwrap().port_start, 46000);
            assert_eq!(
                rtc.ice_servers[0].credential.as_deref(),
                Some("turn-password")
            );
        }
        assert!(config
            .apply_env(|key| (key == "CONFERENCE_SIP_PASSWORD").then(String::new))
            .is_err());
        assert!(ConferenceNetwork::default()
            .apply_env(|key| (key == "CONFERENCE_SERVER_ICE_JSON")
                .then(|| "private-malformed-json".into()))
            .unwrap_err()
            .detail
            .find("private-malformed-json")
            .is_none());
    }
}
