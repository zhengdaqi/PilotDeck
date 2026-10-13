//! Unit checks for the gateway server edge; end-to-end parity is rust/parity/gateway/gateway_server_parity.py.
use pilotdeck_gateway_server::http::{decode_uri_component, url_pathname};
use pilotdeck_gateway_server::ws::{create_websocket_accept_value, read_client_frame, Frame};

#[test]
fn accept_value_matches_node_crypto() {
    // node -e 'crypto.createHash("sha1").update(key + GUID).digest("base64")'
    assert_eq!(create_websocket_accept_value("dGhlIHNhbXBsZSBub25jZQ=="), "s3pPLMBiTxaQ9kYGzzhZRbK+xOo=");
}

#[test]
fn url_pathname_matches_whatwg() {
    for (input, want) in [
        ("/health", "/health"), ("/a/../health", "/health"), ("/%2e%2E/health", "/health"), ("/health?x=1#f", "/health"),
        ("//evil/health", "/health"), ("http://other.example/health", "/health"), ("/a/./b/.", "/a/b/"), ("/a/b/..", "/a/"),
        ("/sp ace", "/sp%20ace"), ("/ü", "/%C3%BC"), ("/a\\b", "/a/b"), ("relative", "/relative"), ("/x%zz", "/x%zz"),
    ] {
        assert_eq!(url_pathname(input), want, "{input}");
    }
}

#[test]
fn decode_uri_component_errors_like_js() {
    assert_eq!(decode_uri_component("p%2F1").as_deref(), Some("p/1"));
    assert_eq!(decode_uri_component("%zz"), None);
    assert_eq!(decode_uri_component("%C3"), None);
    assert_eq!(decode_uri_component("%C3%BC").as_deref(), Some("ü"));
}

#[test]
fn client_frames() {
    let mask = [1u8, 2, 3, 4];
    let payload: Vec<u8> = b"hi".iter().enumerate().map(|(i, b)| b ^ mask[i % 4]).collect();
    let mut buf = vec![0x81, 0x82];
    buf.extend_from_slice(&mask);
    buf.extend_from_slice(&payload);
    match read_client_frame(&buf).unwrap() {
        Frame::Frame { opcode, payload, consumed } => assert_eq!((opcode, payload.as_slice(), consumed), (1, &b"hi"[..], 8)),
        Frame::Incomplete => panic!(),
    }
    assert!(matches!(read_client_frame(&buf[..5]).unwrap(), Frame::Incomplete));
    assert_eq!(read_client_frame(&[0x81, 0x02, b'h', b'i']).err(), Some("Client WebSocket frames must be masked."));
}
