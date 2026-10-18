use std::net::IpAddr;
use std::time::Duration;

use serde::Deserialize;
use serde::de::DeserializeOwned;

use crate::api::{ApiError, DnsApi, Record, RecordType};

/// Default Cloudflare API base URL.
pub(crate) const DEFAULT_API_BASE: &str = "https://api.cloudflare.com/client/v4";

const TIMEOUT: Duration = Duration::from_secs(10);

/// Upper bound on the response body accepted from Cloudflare.
///
/// Responses carry a handful of records, so this only guards against an
/// unexpected payload exhausting memory.
const MAX_RESPONSE_BYTES: u64 = 64 * 1024;

/// Length of a response snippet embedded in error messages.
const MAX_ERROR_SNIPPET: usize = 160;

type ApiResponse = Result<ureq::http::Response<ureq::Body>, ureq::Error>;

/// [`DnsApi`] implementation backed by the Cloudflare v4 HTTP API.
pub(crate) struct HttpApi {
    agent: ureq::Agent,
    base_url: String,
    authorization: String,
}

impl HttpApi {
    /// Uses the official Cloudflare API endpoint.
    pub(crate) fn new(token: impl Into<String>) -> Self {
        Self::with_base_url(token, DEFAULT_API_BASE)
    }

    /// Uses a caller-provided base URL, for mock servers and tests.
    pub(crate) fn with_base_url(token: impl Into<String>, base_url: impl Into<String>) -> Self {
        Self {
            agent: ddrfsh_core::http_agent(TIMEOUT),
            base_url: base_url.into().trim_end_matches('/').to_owned(),
            authorization: format!("Bearer {}", token.into()),
        }
    }
}

impl DnsApi for HttpApi {
    fn find_records(
        &self,
        zone: &str,
        name: &str,
        kind: RecordType,
    ) -> Result<Vec<Record>, ApiError> {
        let url = format!(
            "{}/zones/{zone}/dns_records?name={name}&type={}",
            self.base_url,
            kind.as_str()
        );

        let response = self
            .agent
            .get(&url)
            .header("Authorization", self.authorization.as_str())
            .header("Accept", "application/json")
            .call();

        let found: Vec<RawRecord> = complete(&url, response)?;
        Ok(found.into_iter().map(Record::from).collect())
    }

    fn update_record(&self, zone: &str, id: &str, ip: IpAddr) -> Result<(), ApiError> {
        // The ID comes from the API response and is interpolated into the URL
        // path, so reject anything that is not a plain identifier.
        if id.is_empty() || !id.bytes().all(|byte| byte.is_ascii_alphanumeric()) {
            return Err(ApiError::Malformed {
                url: format!("{}/zones/{zone}/dns_records", self.base_url),
                message: "record id contains unexpected characters".to_owned(),
            });
        }

        let url = format!("{}/zones/{zone}/dns_records/{id}", self.base_url);
        let body = serde_json::json!({ "content": ip.to_string() });

        let response = self
            .agent
            .patch(&url)
            .header("Authorization", self.authorization.as_str())
            .header("Accept", "application/json")
            .header("Content-Type", "application/json")
            .send(body.to_string());

        complete::<serde_json::Value>(&url, response)?;
        Ok(())
    }

    fn create_record(
        &self,
        zone: &str,
        name: &str,
        kind: RecordType,
        ip: IpAddr,
    ) -> Result<(), ApiError> {
        let url = format!("{}/zones/{zone}/dns_records", self.base_url);
        let body = serde_json::json!({
            "type": kind.as_str(),
            "name": name,
            "content": ip.to_string(),
            "ttl": 1,
            "proxied": false,
        });

        let response = self
            .agent
            .post(&url)
            .header("Authorization", self.authorization.as_str())
            .header("Accept", "application/json")
            .header("Content-Type", "application/json")
            .send(body.to_string());

        complete::<serde_json::Value>(&url, response)?;
        Ok(())
    }
}

#[derive(Debug, Deserialize)]
struct RawRecord {
    id: String,
    name: String,
    #[serde(rename = "type")]
    kind: String,
    content: String,
}

impl From<RawRecord> for Record {
    fn from(raw: RawRecord) -> Self {
        Self {
            id: raw.id,
            name: raw.name,
            kind: raw.kind,
            content: raw.content,
        }
    }
}

#[derive(Debug, Deserialize)]
struct Envelope<T> {
    success: bool,
    #[serde(default)]
    errors: Vec<ApiMessage>,
    result: Option<T>,
}

#[derive(Debug, Deserialize)]
struct ApiMessage {
    #[serde(default)]
    code: u32,
    #[serde(default)]
    message: String,
}

fn complete<T: DeserializeOwned>(url: &str, response: ApiResponse) -> Result<T, ApiError> {
    let mut response = response.map_err(|err| ApiError::Request {
        url: url.to_owned(),
        message: err.to_string(),
    })?;

    let status = response.status().as_u16();
    let body = response
        .body_mut()
        .with_config()
        .limit(MAX_RESPONSE_BYTES)
        .read_to_string()
        .map_err(|err| ApiError::Malformed {
            url: url.to_owned(),
            message: err.to_string(),
        })?;

    parse_envelope(url, status, &body)
}

fn parse_envelope<T: DeserializeOwned>(url: &str, status: u16, body: &str) -> Result<T, ApiError> {
    let envelope: Envelope<T> = match serde_json::from_str(body) {
        Ok(envelope) => envelope,
        Err(err) => {
            return Err(if is_success(status) {
                ApiError::Malformed {
                    url: url.to_owned(),
                    message: err.to_string(),
                }
            } else {
                ApiError::Status {
                    url: url.to_owned(),
                    status,
                    details: summarize(body),
                }
            });
        }
    };

    if !is_success(status) || !envelope.success {
        let details = if envelope.errors.is_empty() {
            summarize(body)
        } else {
            envelope
                .errors
                .iter()
                .map(|error| format!("{} (code {})", error.message, error.code))
                .collect::<Vec<_>>()
                .join("; ")
        };

        return Err(ApiError::Status {
            url: url.to_owned(),
            status,
            details,
        });
    }

    envelope.result.ok_or_else(|| ApiError::Malformed {
        url: url.to_owned(),
        message: "response contains no result".to_owned(),
    })
}

fn is_success(status: u16) -> bool {
    (200..=299).contains(&status)
}

fn summarize(body: &str) -> String {
    let flat = body.replace(['\n', '\r'], " ");
    let trimmed = flat.trim();
    let snippet: String = trimmed.chars().take(MAX_ERROR_SNIPPET).collect();

    if snippet.len() < trimmed.len() {
        format!("{snippet}…")
    } else if snippet.is_empty() {
        "<empty body>".to_owned()
    } else {
        snippet
    }
}

#[cfg(test)]
mod tests {
    use std::io::{BufRead, BufReader, Read, Write};
    use std::net::TcpListener;
    use std::sync::{Arc, Mutex};
    use std::thread::{self, JoinHandle};

    use super::*;

    const ZONE: &str = "0123456789abcdef0123456789abcdef";
    const RECORD_NAME: &str = "home.example.com";
    const TOKEN: &str = "test-token";

    /// Serves exactly one request and records it verbatim.
    fn serve(status: u16, body: &str) -> (String, Arc<Mutex<Vec<String>>>, JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let requests = Arc::new(Mutex::new(Vec::new()));
        let recorded = Arc::clone(&requests);
        let body = body.to_owned();

        let handle = thread::spawn(move || {
            let Ok((mut stream, _)) = listener.accept() else {
                return;
            };

            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut request = String::new();
            let mut content_length = 0;

            loop {
                let mut line = String::new();
                if reader.read_line(&mut line).unwrap_or(0) == 0 || line == "\r\n" {
                    break;
                }
                if let Some(value) = line.to_ascii_lowercase().strip_prefix("content-length:") {
                    content_length = value.trim().parse().unwrap_or(0);
                }
                request.push_str(&line);
            }

            let mut payload = vec![0_u8; content_length];
            if content_length > 0 {
                let _ = reader.read_exact(&mut payload);
            }
            request.push_str(&String::from_utf8_lossy(&payload));

            recorded.lock().unwrap().push(request);

            let response = format!(
                "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            let _ = stream.write_all(response.as_bytes());
        });

        (format!("http://{addr}"), requests, handle)
    }

    fn api(base_url: String) -> HttpApi {
        HttpApi::with_base_url(TOKEN, base_url)
    }

    fn one_request(requests: &Arc<Mutex<Vec<String>>>) -> String {
        let requests = requests.lock().unwrap();
        assert_eq!(requests.len(), 1, "{requests:?}");
        requests[0].clone()
    }

    #[test]
    fn find_records_parses_matches() {
        let (base, requests, server) = serve(
            200,
            r#"{"result":[{"id":"abc","name":"home.example.com","type":"A","content":"1.1.1.1"}],"success":true,"errors":[],"messages":[]}"#,
        );

        let found = api(base)
            .find_records(ZONE, RECORD_NAME, RecordType::A)
            .unwrap();

        assert_eq!(
            found,
            vec![Record {
                id: "abc".to_owned(),
                name: "home.example.com".to_owned(),
                kind: "A".to_owned(),
                content: "1.1.1.1".to_owned(),
            }]
        );

        let request = one_request(&requests);
        assert!(
            request.starts_with(&format!(
                "GET /zones/{ZONE}/dns_records?name={RECORD_NAME}&type=A "
            )),
            "{request}"
        );
        assert!(
            request
                .to_ascii_lowercase()
                .contains("authorization: bearer test-token"),
            "{request}"
        );
        server.join().unwrap();
    }

    #[test]
    fn find_records_returns_an_empty_list() {
        let (base, _, server) = serve(200, r#"{"result":[],"success":true,"errors":[]}"#);

        let found = api(base)
            .find_records(ZONE, RECORD_NAME, RecordType::Aaaa)
            .unwrap();

        assert!(found.is_empty());
        server.join().unwrap();
    }

    #[test]
    fn find_records_reports_api_errors() {
        let (base, _, server) = serve(
            403,
            r#"{"result":null,"success":false,"errors":[{"code":9109,"message":"Unauthorized to access zone"}]}"#,
        );

        let err = api(base)
            .find_records(ZONE, RECORD_NAME, RecordType::A)
            .unwrap_err();

        match err {
            ApiError::Status {
                status,
                ref details,
                ..
            } => {
                assert_eq!(status, 403);
                assert!(details.contains("Unauthorized to access zone"), "{details}");
                assert!(details.contains("9109"), "{details}");
            }
            other => panic!("unexpected error: {other}"),
        }
        server.join().unwrap();
    }

    #[test]
    fn find_records_reports_server_errors_with_a_body_summary() {
        let (base, _, server) = serve(500, "<html>internal error</html>");

        let err = api(base)
            .find_records(ZONE, RECORD_NAME, RecordType::A)
            .unwrap_err();

        match err {
            ApiError::Status {
                status,
                ref details,
                ..
            } => {
                assert_eq!(status, 500);
                assert!(details.contains("internal error"), "{details}");
            }
            other => panic!("unexpected error: {other}"),
        }
        server.join().unwrap();
    }

    #[test]
    fn find_records_rejects_malformed_success_responses() {
        let (base, _, server) = serve(200, "not json");

        let err = api(base)
            .find_records(ZONE, RECORD_NAME, RecordType::A)
            .unwrap_err();

        assert!(matches!(err, ApiError::Malformed { .. }), "{err}");
        server.join().unwrap();
    }

    #[test]
    fn update_record_sends_a_partial_patch() {
        let (base, requests, server) =
            serve(200, r#"{"result":{"id":"abc"},"success":true,"errors":[]}"#);

        api(base)
            .update_record(ZONE, "abc", "8.8.8.8".parse().unwrap())
            .unwrap();

        let request = one_request(&requests);
        assert!(
            request.starts_with(&format!("PATCH /zones/{ZONE}/dns_records/abc ")),
            "{request}"
        );
        assert!(request.contains(r#"{"content":"8.8.8.8"}"#), "{request}");
        assert!(
            request
                .to_ascii_lowercase()
                .contains("content-type: application/json"),
            "{request}"
        );
        server.join().unwrap();
    }

    #[test]
    fn update_record_reports_api_errors() {
        let (base, _, server) = serve(
            400,
            r#"{"result":null,"success":false,"errors":[{"code":81044,"message":"Record does not exist"}]}"#,
        );

        let err = api(base)
            .update_record(ZONE, "abc", "8.8.8.8".parse().unwrap())
            .unwrap_err();

        match err {
            ApiError::Status {
                status,
                ref details,
                ..
            } => {
                assert_eq!(status, 400);
                assert!(details.contains("Record does not exist"), "{details}");
            }
            other => panic!("unexpected error: {other}"),
        }
        server.join().unwrap();
    }

    #[test]
    fn create_record_sends_the_expected_body() {
        let (base, requests, server) =
            serve(200, r#"{"result":{"id":"abc"},"success":true,"errors":[]}"#);

        api(base)
            .create_record(
                ZONE,
                RECORD_NAME,
                RecordType::Aaaa,
                "2001:db8::1".parse().unwrap(),
            )
            .unwrap();

        let request = one_request(&requests);
        assert!(
            request.starts_with(&format!("POST /zones/{ZONE}/dns_records ")),
            "{request}"
        );
        assert!(request.contains(r#""type":"AAAA""#), "{request}");
        assert!(
            request.contains(r#""name":"home.example.com""#),
            "{request}"
        );
        assert!(request.contains(r#""content":"2001:db8::1""#), "{request}");
        assert!(request.contains(r#""ttl":1"#), "{request}");
        assert!(request.contains(r#""proxied":false"#), "{request}");
        server.join().unwrap();
    }

    #[test]
    fn reports_unreachable_endpoints() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        drop(listener);

        let err = api(base)
            .find_records(ZONE, RECORD_NAME, RecordType::A)
            .unwrap_err();

        assert!(matches!(err, ApiError::Request { .. }), "{err}");
    }

    #[test]
    fn rejects_record_ids_with_path_characters() {
        let err = api("http://127.0.0.1:1".to_owned())
            .update_record(ZONE, "../admin", "8.8.8.8".parse().unwrap())
            .unwrap_err();

        assert!(matches!(err, ApiError::Malformed { .. }), "{err}");
    }

    #[test]
    fn error_messages_do_not_contain_the_token() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        drop(listener);

        let token = "super-secret-token";
        let err = HttpApi::with_base_url(token, base)
            .find_records(ZONE, RECORD_NAME, RecordType::A)
            .unwrap_err();

        assert!(!err.to_string().contains(token), "{err}");
    }
}
