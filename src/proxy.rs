use axum::body::Body;
use axum::http::{HeaderMap, Method, StatusCode};
use axum::response::{IntoResponse, Response};
use reqwest::Client;

/// Forward a request to the upstream server
pub async fn forward_request(
    client: &Client,
    method: Method,
    upstream_url: &str,
    path: &str,
    headers: HeaderMap,
    body: Vec<u8>,
) -> Response {
    // Build the full upstream URL
    let url = format!("{}{}", upstream_url.trim_end_matches('/'), path);

    // Create the request builder
    let mut request_builder = match method {
        Method::GET => client.get(&url),
        Method::POST => client.post(&url),
        Method::PUT => client.put(&url),
        Method::DELETE => client.delete(&url),
        Method::PATCH => client.patch(&url),
        Method::HEAD => client.head(&url),
        Method::OPTIONS => {
            // For OPTIONS, use request() method
            client.request(reqwest::Method::OPTIONS, &url)
        }
        _ => {
            // For other methods, try to convert
            let reqwest_method = match reqwest::Method::from_bytes(method.as_str().as_bytes()) {
                Ok(m) => m,
                Err(_) => {
                    return (StatusCode::METHOD_NOT_ALLOWED, "Unsupported HTTP method")
                        .into_response();
                }
            };
            client.request(reqwest_method, &url)
        }
    };

    // Add headers to the request (skip certain headers like Host, Connection)
    for (name, value) in headers.iter() {
        let name_str = name.as_str().to_lowercase();
        // Skip headers that shouldn't be forwarded
        if name_str == "host" || name_str == "connection" {
            continue;
        }
        if let Ok(value_str) = value.to_str() {
            request_builder = request_builder.header(name.as_str(), value_str);
        }
    }

    // Add body if present
    if !body.is_empty() {
        request_builder = request_builder.body(body);
    }

    // Send the request
    match request_builder.send().await {
        Ok(upstream_response) => {
            // Extract status code
            let status = upstream_response.status();

            // Extract headers
            let mut response_headers = HeaderMap::new();
            for (name, value) in upstream_response.headers().iter() {
                if let Ok(header_name) =
                    axum::http::HeaderName::from_bytes(name.as_str().as_bytes())
                {
                    if let Ok(header_value) = axum::http::HeaderValue::from_bytes(value.as_bytes())
                    {
                        response_headers.insert(header_name, header_value);
                    }
                }
            }

            // Extract body
            match upstream_response.bytes().await {
                Ok(body_bytes) => {
                    let mut response = Response::new(Body::from(body_bytes.to_vec()));
                    // Convert reqwest::StatusCode to axum::http::StatusCode
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
            // Handle connection errors
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
