use axum::body::Body;
use axum::http::{HeaderMap, Method, StatusCode};
use axum::response::{IntoResponse, Response};
use futures_util::StreamExt;
use reqwest::Client;
use std::error::Error;

/// Build the upstream HTTP request (shared between streaming and buffered forward).
/// Returns `Ok(request)` or an error `Response` for unsupported method.
fn build_upstream_request(
    client: &Client,
    method: &Method,
    upstream_url: &str,
    path: &str,
    headers: &HeaderMap,
    body: &[u8],
) -> Result<reqwest::Request, Box<Response>> {
    let url = format!("{}{}", upstream_url.trim_end_matches('/'), path);

    let mut request_builder = match *method {
        Method::GET => client.get(&url),
        Method::POST => client.post(&url),
        Method::PUT => client.put(&url),
        Method::DELETE => client.delete(&url),
        Method::PATCH => client.patch(&url),
        Method::HEAD => client.head(&url),
        Method::OPTIONS => client.request(reqwest::Method::OPTIONS, &url),
        _ => {
            let reqwest_method = match reqwest::Method::from_bytes(method.as_str().as_bytes()) {
                Ok(m) => m,
                Err(_) => {
                    return Err(Box::new(
                        (StatusCode::METHOD_NOT_ALLOWED, "Unsupported HTTP method").into_response(),
                    ));
                }
            };
            client.request(reqwest_method, &url)
        }
    };

    for (name, value) in headers.iter() {
        let name_lower = name.as_str();
        if name_lower.eq_ignore_ascii_case("host") || name_lower.eq_ignore_ascii_case("connection")
        {
            continue;
        }
        if let Ok(value_str) = value.to_str() {
            request_builder = request_builder.header(name.as_str(), value_str);
        }
    }

    if !body.is_empty() {
        request_builder = request_builder.body(body.to_vec());
    }

    request_builder.build().map_err(|_| {
        Box::new((StatusCode::BAD_GATEWAY, "Failed to build upstream request").into_response())
    })
}

fn copy_response_headers(upstream: &reqwest::Response) -> HeaderMap {
    let mut response_headers = HeaderMap::new();
    for (name, value) in upstream.headers().iter() {
        if let Ok(header_name) = axum::http::HeaderName::from_bytes(name.as_str().as_bytes()) {
            if let Ok(header_value) = axum::http::HeaderValue::from_bytes(value.as_bytes()) {
                response_headers.insert(header_name, header_value);
            }
        }
    }
    response_headers
}

/// Forward a request to the upstream server and stream the response body.
/// Use this when the response body does not need to be buffered (e.g. no response validation).
pub async fn forward_request_streaming(
    client: &Client,
    method: Method,
    upstream_url: &str,
    path: &str,
    headers: HeaderMap,
    body: Vec<u8>,
) -> Response {
    let request = match build_upstream_request(client, &method, upstream_url, path, &headers, &body)
    {
        Ok(r) => r,
        Err(resp) => return *resp,
    };

    match client.execute(request).await {
        Ok(upstream_response) => {
            let status = upstream_response.status();
            let response_headers = copy_response_headers(&upstream_response);
            let stream = upstream_response
                .bytes_stream()
                .map(|result| result.map_err(|e| -> Box<dyn Error + Send + Sync> { Box::new(e) }));
            let body = Body::from_stream(stream);
            let mut response = Response::new(body);
            if let Ok(axum_status) = StatusCode::from_u16(status.as_u16()) {
                *response.status_mut() = axum_status;
            }
            *response.headers_mut() = response_headers;
            response
        }
        Err(err) => {
            if err.is_timeout() {
                (StatusCode::GATEWAY_TIMEOUT, "Upstream request timeout").into_response()
            } else if err.is_connect() {
                (StatusCode::BAD_GATEWAY, "Failed to connect to upstream").into_response()
            } else {
                (StatusCode::BAD_GATEWAY, "Upstream request failed").into_response()
            }
        }
    }
}

/// Forward a request to the upstream server and buffer the full response body.
/// Use only when the caller needs to read the body (e.g. OpenAPI response validation).
pub async fn forward_request_buffered(
    client: &Client,
    method: Method,
    upstream_url: &str,
    path: &str,
    headers: HeaderMap,
    body: Vec<u8>,
) -> Response {
    let request = match build_upstream_request(client, &method, upstream_url, path, &headers, &body)
    {
        Ok(r) => r,
        Err(resp) => return *resp,
    };

    match client.execute(request).await {
        Ok(upstream_response) => {
            let status = upstream_response.status();
            let response_headers = copy_response_headers(&upstream_response);
            match upstream_response.bytes().await {
                Ok(body_bytes) => {
                    let mut response = Response::new(Body::from(body_bytes.to_vec()));
                    if let Ok(axum_status) = StatusCode::from_u16(status.as_u16()) {
                        *response.status_mut() = axum_status;
                    }
                    *response.headers_mut() = response_headers;
                    response
                }
                Err(_) => (
                    StatusCode::BAD_GATEWAY,
                    "Failed to read upstream response body",
                )
                    .into_response(),
            }
        }
        Err(err) => {
            if err.is_timeout() {
                (StatusCode::GATEWAY_TIMEOUT, "Upstream request timeout").into_response()
            } else if err.is_connect() {
                (StatusCode::BAD_GATEWAY, "Failed to connect to upstream").into_response()
            } else {
                (StatusCode::BAD_GATEWAY, "Upstream request failed").into_response()
            }
        }
    }
}
