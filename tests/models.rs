use createos::{
    ComputerButtonRequest, ComputerCreateScreenRequest, ComputerScrollRequest,
    CreateSandboxRequest, PtySize, SandboxStatus, TemplateLogEvent,
};

#[test]
fn request_uses_api_wire_names_and_omits_defaults() {
    let request = CreateSandboxRequest {
        shape: "s-4vcpu-4gb".into(),
        ingress_enabled: true,
        ssh_public_keys: vec!["ssh-ed25519 example".into()],
        ..Default::default()
    };
    let value = serde_json::to_value(request).unwrap();
    assert_eq!(value["shape"], "s-4vcpu-4gb");
    assert_eq!(value["ingress_enabled"], true);
    assert!(value.get("ssh_public_keys").is_none());
    assert_eq!(value["ssh_pubkeys"][0], "ssh-ed25519 example");
}

#[test]
fn wire_string_types_preserve_unknown_values() {
    let status: SandboxStatus = serde_json::from_str(r#""future-state""#).unwrap();
    assert_eq!(status.as_str(), "future-state");
}

#[test]
fn template_log_final_field_round_trips() {
    let event: TemplateLogEvent = serde_json::from_str(r#"{"final":true,"new_field":42}"#).unwrap();
    assert!(event.final_);
    assert_eq!(event.extra["new_field"], 42);
}

#[test]
fn optional_request_defaults_are_omitted_from_the_wire_format() {
    for value in [
        serde_json::to_value(PtySize::default()).unwrap(),
        serde_json::to_value(ComputerScrollRequest::default()).unwrap(),
        serde_json::to_value(ComputerButtonRequest::default()).unwrap(),
        serde_json::to_value(ComputerCreateScreenRequest::default()).unwrap(),
    ] {
        assert_eq!(value, serde_json::json!({}));
    }
}
